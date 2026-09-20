//! Startup recovery for the screen-capture STAGING directory (spec §10,
//! docs/Gaps.md GAP-115).
//!
//! Until now there was no screen recovery at all: `capture/src/recovery.rs`
//! sweeps vault roots, not staging, so a `.mp4.part` orphaned by a crash — or
//! by the ready-timeout path (GAP-110) — sat there forever, invisible to
//! every surface in the app and growing by one file per crash.
//!
//! **This module deletes files, so its shape is deliberate.** Every decision
//! is a pure function (`classify`, `part_holds_footage`, `is_stale_at`,
//! `should_postpone`) tested on Linux; the filesystem half is a thin
//! `read_dir` walk that only carries those decisions out. Four rules, each
//! copied from the audio and import recoveries because it prevented a real
//! failure there. Three are documented at the code that enforces them —
//! **(1) ownership filter first** (`owned`), **(2) never follow a symlink**
//! (`scan_dir`), **(3) postponed while a capture is active, rescheduled while
//! work is pending** (`should_postpone`, which READS `CaptureGuard::active()`
//! and never claims it: `screen_commands.rs` pins exactly one
//! `release(CaptureKind::Screen)` there and zero in the worker, so a claim
//! here would be a second site). The fourth is **(4) named constants, not
//! inlined literals** — the audio side's staleness window is a bare
//! `Duration::from_secs(60)` at a call site, while `STALE_AFTER` below is the
//! same 60 s *on purpose* (spec §10 wants one staleness rule for the whole
//! app) but named, so the next reader can see it is shared and not a
//! coincidence.
//!
//! A spawn failure LOGS and continues (`run_import_recovery`'s posture),
//! never `.expect`-panics (`capture_commands::run_recovery`'s, which the
//! ledger names as the worse precedent).
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Manager};
use vault_buddy_screen::{mp4_boxes, staging};

use crate::capture_guard::{CaptureGuard, CaptureKind};
use crate::editor_commands::is_safe_base;

/// How old a file must be before recovery will touch it. **The same 60 s the
/// audio sweep gets**, deliberately: spec §10 asks for one staleness rule
/// across the app, and two windows that drifted apart would mean "stale" meant
/// two different things in two janitors sweeping for the same crash.
const STALE_AFTER: Duration = Duration::from_secs(60);

/// Retry cadence while work is still pending (a postponed pass, or a fresh
/// orphan). The audio and import recoveries' 90 s.
const RETRY_EVERY: Duration = Duration::from_secs(90);

/// Bound on the retries. 960 × 90 s ≈ **24 hours**, after which a
/// permanently-fresh anomaly stops costing a sleeping thread forever.
const MAX_RETRIES: u32 = 960;

/// How much of a `.part` the footage sniff reads (the audio side's
/// `FRAME_SNIFF_LEN` trick). A staged capture can be gigabytes and
/// `mp4_boxes::scan` takes a `&[u8]`, so reading it whole to answer a question
/// about its HEAD would pull a multi-hour recording into memory. Both boxes
/// the answer depends on sit near the head of an MF-written fMP4: `moov` is
/// the up-front index and the first `moof` precedes the first `mdat`. The
/// deliberate limit: a `.part` whose `moov`/first `moof` sat beyond this
/// prefix would read as empty and be deleted — a shape `sink.rs` cannot make.
const PART_SNIFF_LEN: u64 = 1024 * 1024;

/// The infix an export temp carries (`.<base>.export.mp4.part`), so an
/// abandoned one is never mistaken for a capture and PROMOTED. Imported,
/// never respelled: two literals is how a temp quietly stops being swept.
use vault_buddy_screen::staging::EXPORT_PART_INFIX;

/// What a name in the staging directory is, decided by name alone:
/// `.<base>.mp4.part` (a capture being written), `.<base>.export.mp4.part`
/// (an export being written), `<base>.mp4` (a published staged capture),
/// `<base>.json` (its sidecar) — and everything else, which is **never
/// touched**, whatever it looks like.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Part(String),
    ExportTemp(String),
    Staged(String),
    Sidecar(String),
    Foreign,
}

/// The ownership filter, and the only place a name becomes an action. The
/// export-temp shape is checked BEFORE the plain part shape because
/// `base_from_part(".Demo.export.mp4.part")` answers `Some("Demo.export")` — a
/// perfectly safe base — so a plain-part-first order would classify every
/// export temp as a promotable capture. The cost: a capture whose base
/// genuinely ends in `.export` has its orphaned `.part` deleted as an export
/// temp instead of promoted. The two are indistinguishable by name, and the
/// other order would promote half-written transcodes for everyone.
fn classify(file_name: &str) -> Entry {
    if let Some(base) = staging::base_from_part(file_name) {
        if let Some(stem) = base.strip_suffix(EXPORT_PART_INFIX) {
            return owned(stem, Entry::ExportTemp);
        }
        return owned(&base, Entry::Part);
    }
    if let Some(stem) = file_name.strip_suffix(".mp4") {
        return owned(stem, Entry::Staged);
    }
    if let Some(stem) = file_name.strip_suffix(".json") {
        return owned(stem, Entry::Sidecar);
    }
    Entry::Foreign
}

/// Rule 1, the ownership filter: a name is ours only if it round-trips through
/// the `staging` helpers AND passes BOTH checks below.
/// **`is_capture_base` is not in the plan and is load-bearing.**
/// `is_safe_base` answers "could this text safely become a path", a very
/// different question from "did WE write this": a user's `.download.mp4.part`
/// — the precise file `capture::recovery`'s own foreign-part test exists to
/// protect — has a perfectly safe base and was DELETED by an
/// is_safe_base-only filter (caught by
/// `foreign_files_survive_a_sweep_that_demonstrably_ran`). Every base this app
/// stages is `capture_paths::base_name` + `staging::reserve_base`, so the
/// audio side's `YYYY-MM-DD HHmm <title>` check applies verbatim.
/// `is_safe_base` stays in front of the path join as defence in depth.
fn owned(stem: &str, make: fn(String) -> Entry) -> Entry {
    if is_safe_base(stem) && vault_buddy_core::capture_paths::is_capture_base(stem) {
        make(stem.to_string())
    } else {
        Entry::Foreign
    }
}

/// Does this `.part` prefix hold footage worth promoting? BOTH halves are
/// required: fragments with no `moov` have no index, so no player can decode
/// them, and a `moov` with no `moof` is a bare header — promoting it would
/// offer the user a zero-length "recording" that opens to nothing.
fn part_holds_footage(prefix: &[u8]) -> bool {
    let scan = mp4_boxes::scan(prefix);
    scan.has_moov() && scan.fragment_count() > 0
}

/// Rule 3: never sweep while EITHER domain holds the guard — audio means the
/// app is writing elsewhere, screen means a `.part` right here is live.
fn should_postpone(active: Option<CaptureKind>) -> bool {
    active.is_some()
}

/// Pure staleness, so the clock cases are testable without real mtimes — the
/// `capture::recovery::is_stale_at` precedent including its skew branch: a
/// live file's mtime tracks "now", so small skew reads as fresh, while a gap
/// beyond the window means a clock jump stranded an orphan.
fn is_stale_at(modified: SystemTime, now: SystemTime, stale_after: Duration) -> bool {
    match now.duration_since(modified) {
        Ok(age) => age >= stale_after,
        Err(e) => e.duration() >= stale_after,
    }
}

fn read_prefix(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// A recovered capture's `recorded_at`, from the file's own mtime rather than
/// blank: the file system still knows roughly when, and an empty string
/// renders as no date at all in the resume list.
fn recorded_at_from(modified: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(modified).to_rfc3339()
}

/// Everything recovery knows about a capture it did not see recorded.
/// `vault_id`, the dimensions and the duration are empty/zero because nothing
/// on disk records them once the sidecar is gone, and inventing values is
/// worse than admitting the gap. The `recovered` marker rides through
/// `StagedSidecar`'s flattened catch-all rather than adding a field.
fn minimal_sidecar(base: &str, modified: SystemTime) -> staging::StagedSidecar {
    let mut extra = serde_json::Map::new();
    extra.insert("recovered".to_string(), serde_json::Value::Bool(true));
    staging::StagedSidecar {
        base: base.to_string(),
        vault_id: String::new(),
        source_title: base.to_string(),
        source_kind: "screen".to_string(),
        inputs: Vec::new(),
        duration_ms: 0,
        paused_ms: 0,
        width: 0,
        height: 0,
        recorded_at: recorded_at_from(modified),
        timeline: None,
        extra,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RecoveryAction {
    Promoted(PathBuf),
    DeletedEmptyPart(PathBuf),
    DeletedExportTemp(PathBuf),
    DeletedOrphanSidecar(PathBuf),
    WroteSidecar(PathBuf),
}

#[derive(Debug, Default)]
struct Sweep {
    actions: Vec<RecoveryAction>,
    /// Files left because they were not yet stale. Non-zero keeps the thread
    /// retrying (the `run_import_recovery` shape).
    pending: usize,
}

/// One regular file in the staging directory. Its metadata comes from
/// `symlink_metadata` on the leaf, so nothing here was learned by following
/// a link.
struct Found {
    path: PathBuf,
    entry: Entry,
    modified: SystemTime,
}

fn scan_dir(dir: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        // Rule 2: file_type() reads the dirent and does NOT follow, so a
        // symlink is never a regular file here and is skipped. One named
        // exactly like ours would otherwise be unlinked or, worse, promoted
        // by renaming the LINK over a real capture.
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let entry_kind = classify(&name);
        if entry_kind == Entry::Foreign {
            continue;
        }
        // Belt and braces: the leaf's own metadata, again without
        // following, in case a filesystem left the dirent's type unknown.
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        out.push(Found {
            path: entry.path(),
            entry: entry_kind,
            modified,
        });
    }
    out
}

/// Rename `from` onto the first free `<base>.mp4` in `dir`, never replacing.
/// `rename_noreplace` is the arbiter, as in `capture::recovery`:
/// `std::fs::rename` REPLACES its destination on every platform, so one
/// created between the free check and the move would clobber a real capture.
/// `staging::reserve_base` is deliberately NOT used — it also needs the
/// `.part` name free, and the `.part` being promoted is what makes it taken,
/// so every promotion would land on a needless ` (2)`.
fn promote_into_free_name(from: &Path, dir: &Path, base: &str) -> std::io::Result<PathBuf> {
    for n in 1..10_000u32 {
        let candidate = if n == 1 {
            base.to_string()
        } else {
            format!("{base} ({n})")
        };
        let mp4 = dir.join(staging::mp4_file_name(&candidate));
        let sidecar = dir.join(staging::sidecar_file_name(&candidate));
        if mp4.exists() || sidecar.exists() {
            continue;
        }
        match vault_buddy_core::capture_paths::rename_noreplace(from, &mp4) {
            Ok(()) => return Ok(mp4),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Some Windows API paths report a taken destination as
            // PermissionDenied rather than AlreadyExists.
            Err(_) if mp4.exists() => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free staged name",
    ))
}

/// The whole sweep, as a function of a directory and a clock, so every row of
/// spec §10's recovery table is testable with a tempdir and no running app.
fn sweep_staging_dir(dir: &Path, now: SystemTime, stale_after: Duration) -> Sweep {
    let found = scan_dir(dir);
    let mut sweep = Sweep::default();

    // The snapshot the "orphan" rules are decided against, taken BEFORE any
    // action -- so a capture promoted in this same pass (which writes both its
    // .mp4 and its sidecar) is invisible to them and cannot be re-judged.
    let bases = |want: fn(&Entry) -> Option<&str>| -> std::collections::HashSet<&str> {
        found.iter().filter_map(|f| want(&f.entry)).collect()
    };
    let staged = bases(|e| match e {
        Entry::Staged(b) => Some(b.as_str()),
        _ => None,
    });
    let sidecars = bases(|e| match e {
        Entry::Sidecar(b) => Some(b.as_str()),
        _ => None,
    });

    for f in &found {
        if !is_stale_at(f.modified, now, stale_after) {
            // Not yet stale: it may be a live capture's own file, or an
            // export mid-write. Leave it and come back.
            sweep.pending += 1;
            continue;
        }
        match &f.entry {
            Entry::Part(base) => promote_or_delete_part(f, dir, base, &mut sweep),
            // Litter by definition: the staged capture it came from is
            // still on disk, so nothing is lost by removing the temp.
            Entry::ExportTemp(_) => delete(
                f,
                "abandoned export temp",
                RecoveryAction::DeletedExportTemp,
                &mut sweep,
            ),
            Entry::Staged(base) => {
                if sidecars.contains(base.as_str()) {
                    continue;
                }
                // Footage with no sidecar is reachable again the moment it
                // has one — so write one rather than delete the recording.
                write_minimal_sidecar(dir, base, f.modified, &mut sweep);
            }
            Entry::Sidecar(base) => {
                if staged.contains(base.as_str()) {
                    continue;
                }
                delete(
                    f,
                    "orphaned sidecar",
                    RecoveryAction::DeletedOrphanSidecar,
                    &mut sweep,
                )
            }
            Entry::Foreign => unreachable!("scan_dir drops Foreign entries"),
        }
    }
    sweep
}

/// Remove one of our own files and record it; `what` names it in the log, so
/// a sweep is auditable after the fact.
fn delete(f: &Found, what: &str, action: fn(PathBuf) -> RecoveryAction, sweep: &mut Sweep) {
    log::info!("screen-recovery: removing {what} {}", f.path.display());
    match std::fs::remove_file(&f.path) {
        Ok(()) => sweep.actions.push(action(f.path.clone())),
        Err(e) => log::warn!(
            "screen-recovery: could not remove {}: {e}",
            f.path.display()
        ),
    }
}

fn promote_or_delete_part(f: &Found, dir: &Path, base: &str, sweep: &mut Sweep) {
    // A read failure (permissions, AV lock, transient I/O) must NOT look like
    // "no footage" — the audio side's rule, and what keeps deletion reserved
    // for a file proven empty. Leave it for a later pass. (Unexercised: the
    // container runs as root, where chmod 000 is not a read failure.)
    let Ok(prefix) = read_prefix(&f.path, PART_SNIFF_LEN) else {
        log::warn!(
            "screen-recovery: cannot read {}, leaving it for a later pass",
            f.path.display()
        );
        sweep.pending += 1;
        return;
    };
    if !part_holds_footage(&prefix) {
        delete(
            f,
            "a part with no playable footage",
            RecoveryAction::DeletedEmptyPart,
            sweep,
        );
        return;
    }
    match promote_into_free_name(&f.path, dir, base) {
        Ok(mp4) => {
            log::info!("screen-recovery: recovered {}", mp4.display());
            let landed = mp4
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| base.to_string());
            write_minimal_sidecar(dir, &landed, f.modified, sweep);
            sweep.actions.push(RecoveryAction::Promoted(mp4));
        }
        Err(e) => log::warn!(
            "screen-recovery: could not promote {}: {e}",
            f.path.display()
        ),
    }
}

fn write_minimal_sidecar(dir: &Path, base: &str, modified: SystemTime, sweep: &mut Sweep) {
    match staging::write_sidecar(dir, base, &minimal_sidecar(base, modified)) {
        Ok(path) => sweep.actions.push(RecoveryAction::WroteSidecar(path)),
        Err(e) => log::warn!("screen-recovery: could not write a sidecar for {base:?}: {e}"),
    }
}

/// Startup janitor for the screen-capture staging directory. One named
/// background thread; a pass that leaves nothing pending ends it.
pub fn run_screen_recovery(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("screen-recovery".into())
        .spawn(move || {
            let Ok(local) = app.path().app_local_data_dir() else {
                log::warn!("screen-recovery: could not resolve the local data directory");
                return;
            };
            let dir = staging::staging_dir(&local);
            let pass = || -> bool {
                if should_postpone(app.state::<CaptureGuard>().active()) {
                    log::info!("screen-recovery: postponed while a capture is active");
                    return true; // pending → retry
                }
                if !dir.is_dir() {
                    return false; // nothing has ever been staged
                }
                let sweep = sweep_staging_dir(&dir, SystemTime::now(), STALE_AFTER);
                if !sweep.actions.is_empty() {
                    log::info!("screen-recovery: {} action(s)", sweep.actions.len());
                }
                sweep.pending > 0
            };
            for _ in 0..MAX_RETRIES {
                if !pass() {
                    return;
                }
                std::thread::sleep(RETRY_EVERY);
            }
            log::warn!("screen-recovery: gave up after max passes with work still pending");
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("screen-recovery: could not spawn thread: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One top-level ISO-BMFF box: 4-byte big-endian size, 4-character type,
    /// payload. `mp4_boxes::scan` walks exactly this.
    fn bx(kind: &str, payload: &[u8]) -> Vec<u8> {
        let size = (8 + payload.len()) as u32;
        let mut out = size.to_be_bytes().to_vec();
        out.extend_from_slice(kind.as_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A `.part` a player could open: an index plus one closed fragment.
    fn footage() -> Vec<u8> {
        let mut v = header_only();
        v.extend(bx("moof", b"...."));
        v.extend(bx("mdat", b"framedata"));
        v
    }

    /// A bare fMP4 header: what a capture that died before its first
    /// fragment closed leaves behind.
    fn header_only() -> Vec<u8> {
        let mut v = bx("ftyp", b"isom");
        v.extend(bx("moov", b"...."));
        v
    }

    const BASE: &str = "2026-09-20 1432 Demo";
    const OTHER: &str = "2026-09-20 1500 Other";
    const ORPHAN: &str = "2026-09-20 1330 Orphan";
    const KEPT: &str = "2026-09-20 1340 Kept";

    #[test]
    fn only_our_own_names_are_recognised() {
        let b = |e: fn(String) -> Entry| e(BASE.into());
        assert_eq!(classify(".2026-09-20 1432 Demo.mp4.part"), b(Entry::Part));
        assert_eq!(classify("2026-09-20 1432 Demo.mp4"), b(Entry::Staged));
        assert_eq!(classify("2026-09-20 1432 Demo.json"), b(Entry::Sidecar));
        // Minted by the helper the export worker itself uses.
        assert_eq!(
            classify(&staging::export_part_file_name(BASE)),
            b(Entry::ExportTemp)
        );
        // Not ours by extension or shape. Every one has been a real file
        // in somebody's temp directory; none may be deleted.
        for foreign in [
            "notes.txt",
            "Demo.mkv",
            "thumbs.db",
            ".DS_Store",
            "Demo.mp4.bak",
            "report.json.bak",
        ] {
            assert_eq!(classify(foreign), Entry::Foreign, "{foreign} was claimed");
        }
    }

    // Rows where `is_capture_base` is the SOLE decider: every base below is
    // SAFE, so `is_safe_base` alone lets all of them through (see `owned`).
    #[test]
    fn a_file_that_is_not_named_like_one_of_our_captures_is_never_claimed() {
        for foreign in [
            ".download.mp4.part",
            ".notes.export.mp4.part",
            "Demo.mp4",
            "Demo.json",
            "2026-09-20 Demo.mp4", // a date but no HHmm
            "2026-09-20 1432.mp4", // a prefix but no title, so no trailing space
        ] {
            assert_eq!(classify(foreign), Entry::Foreign, "{foreign} was claimed");
        }
    }

    // An unsafe base must never be ours, or recovery becomes the one path
    // acting on a name the guarded commands refuse.
    #[test]
    fn an_unsafe_base_is_foreign_even_in_our_own_name_shape() {
        // The plan's three rows: true, but each trips BOTH halves of the
        // filter, so alone they stay green with `is_safe_base` gone (M1).
        assert_eq!(classify("../obsidian/obsidian.json"), Entry::Foreign);
        assert_eq!(classify("COM1.mp4"), Entry::Foreign);
        assert_eq!(classify(".mp4"), Entry::Foreign);
        // Rows where `is_safe_base` is the SOLE decider: every base here is
        // capture-shaped, so it sails through `is_capture_base`.
        for unsafe_shaped in [
            "2026-09-20 1432 ../../obsidian/obsidian.json",
            ".2026-09-20 1432 a\\b.mp4.part",
            "2026-09-20 1432 Demo .mp4", // trailing space: Windows strips it
            "2026-09-20 1432 x:ads.mp4", // an NTFS alternate-data-stream marker
            "2026-09-20 1432 Demo\u{7}.mp4", // a control character
        ] {
            assert_eq!(
                classify(unsafe_shaped),
                Entry::Foreign,
                "{unsafe_shaped:?} was claimed"
            );
        }
    }

    #[test]
    fn a_part_with_no_index_holds_no_footage_and_a_fragmented_one_does() {
        assert!(part_holds_footage(&footage()));
        assert!(!part_holds_footage(&bx("ftyp", b"isom")));
        assert!(!part_holds_footage(&[]));
        // Fragments but NO initialization index: no player can decode it,
        // so `has_moov()` is not redundant with the fragment count
        // (mutation M5 drops it and must redden here).
        let mut fragments_only = bx("ftyp", b"isom");
        fragments_only.extend(bx("moof", b"...."));
        assert!(!part_holds_footage(&fragments_only));
    }

    // A moov with NO moof is a bare header: nothing recoverable, and
    // promoting it would offer the user a zero-length "recording".
    #[test]
    fn a_part_with_an_index_but_no_fragment_holds_no_footage() {
        assert!(!part_holds_footage(&header_only()));
    }

    #[test]
    fn recovery_is_postponed_while_a_capture_is_running() {
        assert!(should_postpone(Some(CaptureKind::Screen)));
        assert!(should_postpone(Some(CaptureKind::Audio)));
        assert!(!should_postpone(None));
    }

    #[test]
    fn staleness_decision_handles_clock_skew() {
        let now = SystemTime::now();
        let hour = Duration::from_secs(3600);
        assert!(is_stale_at(now - hour, now, STALE_AFTER));
        assert!(!is_stale_at(now - Duration::from_secs(5), now, STALE_AFTER));
        // Slightly ahead (coarse fs timestamps): fresh, because a LIVE
        // capture's mtime tracks "now". Far ahead: a clock jump stranded it,
        // so it must age in rather than wait for the wall clock.
        assert!(!is_stale_at(now + Duration::from_secs(5), now, STALE_AFTER));
        assert!(is_stale_at(now + hour, now, STALE_AFTER));
    }

    // The one staleness rule spec 10 asks for: a drift here would have two
    // janitors sweeping the same crash disagree about what "stale" means.
    #[test]
    fn the_staleness_window_matches_the_audio_recoverys() {
        assert_eq!(STALE_AFTER, Duration::from_secs(60));
        assert_eq!(RETRY_EVERY, Duration::from_secs(90));
        // ~24 h, as the constant's doc claims.
        assert_eq!(RETRY_EVERY * MAX_RETRIES, Duration::from_secs(24 * 60 * 60));
    }

    /// The three names a base owns in the staging directory.
    fn names(d: &Path, b: &str) -> (PathBuf, PathBuf, PathBuf) {
        (
            d.join(staging::part_file_name(b)),
            d.join(staging::mp4_file_name(b)),
            d.join(staging::sidecar_file_name(b)),
        )
    }

    fn stale(dir: &Path, now: SystemTime) -> Sweep {
        sweep_staging_dir(dir, now, Duration::from_secs(60))
    }

    /// A `now` far enough ahead that everything a test writes is stale.
    fn much_later() -> SystemTime {
        SystemTime::now() + Duration::from_secs(3600)
    }

    #[test]
    fn a_stale_part_with_footage_is_promoted_and_gains_a_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let (part, mp4, json) = names(dir.path(), BASE);
        std::fs::write(&part, footage()).unwrap();

        let sweep = stale(dir.path(), much_later());

        assert!(mp4.is_file(), "the footage was promoted: {sweep:?}");
        assert!(!part.exists(), "the part was consumed");
        let read = staging::read_sidecar(&json).expect("a readable sidecar");
        assert_eq!(read.base, BASE);
        // A recovered capture says so, so the UI never claims a recorded
        // duration it does not have.
        assert_eq!(
            read.extra.get("recovered"),
            Some(&serde_json::Value::Bool(true))
        );
        assert!(sweep.actions.contains(&RecoveryAction::Promoted(mp4)));
    }

    #[test]
    fn a_stale_part_with_no_footage_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (part, mp4, _) = names(dir.path(), BASE);
        std::fs::write(&part, header_only()).unwrap();

        let sweep = stale(dir.path(), much_later());

        assert!(!part.exists(), "an empty part is not kept forever");
        assert!(
            !mp4.exists(),
            "and is certainly not promoted to a recording"
        );
        assert!(sweep
            .actions
            .contains(&RecoveryAction::DeletedEmptyPart(part)));
    }

    #[test]
    fn a_fresh_part_is_left_alone_and_keeps_the_pass_pending() {
        let dir = tempfile::tempdir().unwrap();
        let (part, ..) = names(dir.path(), BASE);
        // No footage at all: under the staleness rule it must STILL survive,
        // because a fresh part may be the live capture's own file.
        std::fs::write(&part, b"").unwrap();

        let sweep = stale(dir.path(), SystemTime::now());

        assert!(part.exists(), "a live capture's file is never swept");
        assert!(sweep.actions.is_empty(), "{sweep:?}");
        assert_eq!(sweep.pending, 1, "and the pass reschedules itself");
    }

    // The hazard this module is built around: "the foreign file survived"
    // passes for free if the walk never reached the directory. So this one
    // carries a POSITIVE CONTROL, a real orphan promoted in the same pass.
    #[test]
    fn foreign_files_survive_a_sweep_that_demonstrably_ran() {
        let dir = tempfile::tempdir().unwrap();
        let foreigners = [
            "notes.txt",
            "Demo.mkv",
            "thumbs.db",
            ".DS_Store",
            "Demo.mp4.bak",
            "report.json.bak",
            ".hidden.mp4",
            ".hidden.json",
            ".download.mp4.part",
        ];
        for (i, name) in foreigners.iter().enumerate() {
            std::fs::write(dir.path().join(name), format!("user data {i}")).unwrap();
        }
        let (part, mp4, _) = names(dir.path(), BASE); // the control
        std::fs::write(part, footage()).unwrap();

        let sweep = stale(dir.path(), much_later());

        // POSITIVE CONTROL: without this, the survival assertions below
        // would pass on a sweep that never reached this directory.
        assert!(mp4.is_file(), "the sweep did not run: {sweep:?}");
        for (i, name) in foreigners.iter().enumerate() {
            let path = dir.path().join(name);
            assert!(path.is_file(), "{name} was deleted");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                format!("user data {i}"),
                "{name} was modified"
            );
        }
    }

    #[test]
    fn an_orphan_sidecar_is_deleted_and_one_with_its_capture_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, orphan) = names(dir.path(), ORPHAN);
        let (_, kept_mp4, kept) = names(dir.path(), KEPT);
        std::fs::write(&orphan, "{}").unwrap();
        std::fs::write(&kept, "{}").unwrap();
        std::fs::write(kept_mp4, footage()).unwrap();

        let sweep = stale(dir.path(), much_later());

        assert!(!orphan.exists(), "a sidecar naming nothing is litter");
        // A sidecar whose capture is right there survives, unrewritten.
        assert!(kept.exists(), "{sweep:?}");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "{}");
    }

    #[test]
    fn a_staged_capture_with_no_sidecar_gains_one_rather_than_losing_its_footage() {
        let dir = tempfile::tempdir().unwrap();
        let (_, mp4, json) = names(dir.path(), BASE);
        std::fs::write(&mp4, footage()).unwrap();

        stale(dir.path(), much_later());

        assert!(mp4.is_file(), "footage is NEVER deleted to tidy up");
        // And the capture becomes reachable again.
        assert_eq!(
            staging::read_sidecar(&json).map(|s| s.base),
            Some(BASE.to_string())
        );
    }

    #[test]
    fn a_stale_export_temp_is_deleted_and_never_promoted() {
        let dir = tempfile::tempdir().unwrap();
        // A half-written transcode WITH footage in it, so the only thing
        // keeping it from being promoted into a "recording" the user is
        // offered is classify's export-infix arm.
        let (temp, promoted, _) = names(dir.path(), &format!("{BASE}{EXPORT_PART_INFIX}"));
        std::fs::write(&temp, footage()).unwrap();
        // The staged capture it came from is still there and must survive.
        let (_, source, source_json) = names(dir.path(), BASE);
        std::fs::write(&source, footage()).unwrap();
        std::fs::write(source_json, "{}").unwrap();

        let sweep = stale(dir.path(), much_later());

        assert!(!temp.exists(), "an abandoned export temp is litter");
        assert!(
            !promoted.exists(),
            "a half-written transcode must never be offered as a recording"
        );
        assert!(source.is_file(), "the staged capture survives: {sweep:?}");
    }

    // MUTATION FINDING: the behavioural test below does NOT pin the
    // never-clobber MOVE -- swapping `rename_noreplace` for the REPLACING
    // `std::fs::rename` left all 20 tests green, because the free-name check
    // upstream satisfies the assertion single-threadedly. That is this
    // branch's canonical failure shape. The race `rename_noreplace` closes is
    // a destination created BETWEEN that check and the move, which no
    // single-process test can stage, so the move is pinned structurally
    // instead -- the `capture_exclusion` / `is_safe_base` precedent.
    #[test]
    fn the_promotion_move_is_non_replacing() {
        let src = include_str!("screen_recovery.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(production.contains("capture_paths::rename_noreplace(from, &mp4)"));
        // std::fs::rename REPLACES its destination on every platform.
        assert!(!production.contains("std::fs::rename("));
    }

    // What the free-name search itself buys: an orphan whose base is already
    // taken lands on a suffix rather than failing or overwriting.
    #[test]
    fn a_promotion_onto_a_taken_name_lands_on_a_free_one_instead() {
        let dir = tempfile::tempdir().unwrap();
        let (part, taken, _) = names(dir.path(), BASE);
        std::fs::write(&taken, b"an earlier recovery").unwrap();
        std::fs::write(part, footage()).unwrap();

        stale(dir.path(), much_later());

        assert_eq!(
            std::fs::read_to_string(&taken).unwrap(),
            "an earlier recovery",
            "the existing capture was overwritten"
        );
        let (_, landed, landed_json) = names(dir.path(), &format!("{BASE} (2)"));
        assert!(landed.is_file(), "the orphan landed on a free name instead");
        assert!(landed_json.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_wearing_our_name_is_never_followed_or_deleted() {
        // The failure: a symlink called `.<base>.mp4.part` pointing outside
        // staging. Following it lets recovery unlink -- or RENAME -- somebody
        // else's file. (Mutation-proven: making scan_dir follow reddens this.)
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("precious.mp4");
        std::fs::write(&target, footage()).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let (link, promoted, _) = names(dir.path(), BASE);
        std::os::unix::fs::symlink(&target, &link).unwrap();
        // Positive control again: a real orphan in the same directory, so a
        // sweep that silently did nothing cannot pass this test.
        let (other_part, other_mp4, _) = names(dir.path(), OTHER);
        std::fs::write(other_part, footage()).unwrap();

        let sweep = stale(dir.path(), much_later());

        assert!(
            other_mp4.is_file(),
            "POSITIVE CONTROL: the sweep ran: {sweep:?}"
        );
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "the symlink itself was removed"
        );
        assert!(target.is_file(), "the link's target was touched");
        assert_eq!(std::fs::read(&target).unwrap(), footage());
        assert!(!promoted.exists(), "the link was promoted into a capture");
    }

    // `remove_file` on a directory fails, but a rename would MOVE it, so the
    // regular-file gate is what keeps this harmless.
    #[test]
    fn a_directory_wearing_our_name_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (impostor, ..) = names(dir.path(), BASE);
        std::fs::create_dir(&impostor).unwrap();
        std::fs::write(impostor.join("inside.txt"), "user data").unwrap();

        let sweep = stale(dir.path(), much_later());

        assert!(impostor.is_dir(), "{sweep:?}");
        assert!(impostor.join("inside.txt").is_file());
        assert!(sweep.actions.is_empty(), "{sweep:?}");
    }

    #[test]
    fn a_missing_staging_directory_is_not_an_error() {
        let sweep = stale(Path::new("/nonexistent/never-staged"), much_later());
        assert!(sweep.actions.is_empty());
        assert_eq!(sweep.pending, 0, "nothing to come back for");
    }

    #[test]
    fn a_recovered_sidecar_records_the_files_own_mtime() {
        // Not "now": the recording happened before the crash, and the file
        // system still knows roughly when.
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let s = minimal_sidecar(BASE, when);
        // Two days accepted because the render is timezone-dependent.
        let at = &s.recorded_at;
        assert!(
            at.starts_with("2023-11-14") || at.starts_with("2023-11-15"),
            "{at:?}"
        );
        // An unknown duration is admitted, not invented.
        assert_eq!(s.duration_ms, 0);
        assert!(s.vault_id.is_empty());
    }
}
