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
//!
//! **The seam.** Every pure decision lives in the sibling `decide` module,
//! the filesystem walk and the retry loop live here. The split is what the
//! doc above already described; GAP-147 (this file at exactly 800/800
//! nonblank lines) is what forced it to become two files. `lib.rs` is
//! unchanged: `mod screen_recovery;` resolves a directory module identically.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Manager};
use vault_buddy_screen::staging;

use crate::capture_guard::CaptureGuard;

mod decide;
use decide::{classify, is_stale_at, part_holds_footage, should_postpone, Entry};
// Listing a promoted stem in its capture's sidecar (Task 53) -- its own file
// because this one sits at the 800-line cap.
mod stems;
use stems::list_recovered_stem;

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
        stems: Vec::new(),
        webcam: None,
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
    // A published webcam file counts as a staged NAME here: a capture staged
    // before F25 under a base ending ".webcam" has its video classified as a
    // companion, and its sidecar must not then read as orphaned.
    let staged = bases(|e| match e {
        Entry::Staged(b) | Entry::Companion(b) => Some(b.as_str()),
        _ => None,
    });
    let sidecars = bases(|e| match e {
        Entry::Sidecar(b) => Some(b.as_str()),
        _ => None,
    });

    // Stems promoted in this pass, listed in their capture's sidecar once
    // every capture this pass promotes has one (the loop order is the
    // directory's, not the capture's-before-its-stems).
    let mut stems = Vec::new();
    for f in &found {
        if !is_stale_at(f.modified, now, stale_after) {
            // Not yet stale: it may be a live capture's own file. Leave it
            // and come back.
            sweep.pending += 1;
            continue;
        }
        match &f.entry {
            Entry::Part(base) => promote_or_delete_part(f, dir, base, &mut sweep),
            // Abandoned by definition: nothing has written an export temp
            // since Task 59 retired the phase-5 export, so this one is an
            // older build's leftover — and untouched for the staleness
            // window on top of that.
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
            Entry::WebcamPart(_) => {
                promote_or_delete_companion_part(f, &mut sweep);
            }
            Entry::StemPart(base, index) => {
                if promote_or_delete_companion_part(f, &mut sweep) {
                    stems.push((base.as_str(), index.as_str()));
                }
            }
            // Its capture's own file, never a capture: nothing to decide.
            Entry::Companion(_) => {}
            Entry::Foreign => unreachable!("scan_dir drops Foreign entries"),
        }
    }
    for (base, index) in stems {
        list_recovered_stem(dir, base, index, &mut sweep);
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

/// `true` when `f` is a part worth promoting; otherwise it has already been
/// dealt with — deleted when proven empty, left pending when unreadable.
fn part_is_footage(f: &Found, sweep: &mut Sweep) -> bool {
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
        return false;
    };
    if !part_holds_footage(&prefix) {
        delete(
            f,
            "a part with no playable footage",
            RecoveryAction::DeletedEmptyPart,
            sweep,
        );
        return false;
    }
    true
}

/// A webcam or stem part (F-22, F24) is promoted to its OWN published name —
/// the part's name without the leading dot and `.part`, which its capture
/// owns (`staging_files::capture_file_names`; a stem once
/// `list_recovered_stem` has added it to the sidecar) — never to a free
/// capture name: the name IS its link to the capture, so there is no
/// ` (N)` to fall back to. A taken name is left alone (`rename_noreplace`)
/// and not counted pending: no later pass would answer differently.
/// `true` when the part was promoted.
fn promote_or_delete_companion_part(f: &Found, sweep: &mut Sweep) -> bool {
    let published = f
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix('.'))
        .and_then(|n| n.strip_suffix(".part"))
        .map(|n| f.path.with_file_name(n));
    let Some(to) = published else { return false };
    if !part_is_footage(f, sweep) {
        return false;
    }
    match vault_buddy_core::capture_paths::rename_noreplace(&f.path, &to) {
        Ok(()) => {
            log::info!("screen-recovery: recovered {}", to.display());
            sweep.actions.push(RecoveryAction::Promoted(to));
            true
        }
        Err(e) => {
            log::warn!(
                "screen-recovery: could not promote {} to {}: {e}",
                f.path.display(),
                to.display()
            );
            false
        }
    }
}

fn promote_or_delete_part(f: &Found, dir: &Path, base: &str, sweep: &mut Sweep) {
    if !part_is_footage(f, sweep) {
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
                // `CaptureGuard::active()` takes its mutex, answers and
                // drops it, so this needs no lock-ordering rule to remember
                // (the posture AGENTS.md records for the guard itself).
                let active = app.state::<CaptureGuard>().active();
                if should_postpone(active) {
                    log::info!("screen-recovery: postponed while a capture is active");
                    return true; // pending → retry
                }
                if !dir.is_dir() {
                    return false; // nothing has ever been staged
                }
                // Final review M6: listing a recovered stem rewrites a
                // published capture's sidecar, the file an editor open pins
                // under this same lock (the outermost editor lock; nothing
                // else is held with it here).
                let editor = app.state::<crate::editor::EditorState>();
                let _open = vault_buddy_core::sync_util::lock_ignoring_poison(&editor.open);
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
    use super::decide::fixtures::*;
    use super::*;
    use vault_buddy_screen::staging::EXPORT_PART_INFIX;

    // Final review M6: the sweep read-modify-writes a PUBLISHED capture's
    // sidecar when it lists a recovered stem, and `editor_open_staged`
    // writes the pin into that same sidecar under `EditorState::open`. Two
    // writers of one file must share that lock, or one of them loses the
    // other's field.
    #[test]
    fn the_sweep_pass_runs_under_the_editors_open_lock() {
        use crate::structural_scan::{fn_body, offset_of, production_code};
        let code = production_code(include_str!("mod.rs"));
        let body = fn_body(&code, "pub fn run_screen_recovery(");
        assert!(body.contains("app.state::<crate::editor::EditorState>()"));
        let locked = offset_of(body, "lock_ignoring_poison(&editor.open)");
        let swept = offset_of(body, "sweep_staging_dir(");
        assert!(locked < swept, "the pass must hold `open` while it sweeps");
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

    // F-22: a webcam part left by a crash is PROMOTED with its capture (to
    // `<base>.webcam.mp4`, so discard/clear/the project store find it), an
    // empty one is swept like any empty part, and neither -- nor the
    // published webcam file -- is ever mistaken for a capture of its own.
    #[test]
    fn a_stale_webcam_part_is_promoted_beside_its_capture_never_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let (part, mp4, json) = names(dir.path(), BASE);
        std::fs::write(&part, footage()).unwrap();
        let webcam_part = dir.path().join(staging::webcam_part_file_name(BASE));
        std::fs::write(&webcam_part, footage()).unwrap();
        let empty_part = dir.path().join(staging::webcam_part_file_name(KEPT));
        std::fs::write(&empty_part, header_only()).unwrap();

        let sweep = stale(dir.path(), much_later());

        let webcam = dir.path().join(staging::webcam_file_name(BASE));
        assert!(webcam.is_file(), "the webcam footage was lost: {sweep:?}");
        assert!(!webcam_part.exists());
        assert!(sweep.actions.contains(&RecoveryAction::Promoted(webcam)));
        assert!(mp4.is_file() && json.is_file(), "the capture itself too");
        assert!(!empty_part.exists(), "an empty webcam part is swept");
        let bogus = format!("{BASE}.webcam");
        assert!(
            !dir.path().join(staging::sidecar_file_name(&bogus)).exists(),
            "the webcam track was handed a sidecar of its own"
        );
        // A second pass finds the published webcam file and leaves it be.
        let again = stale(dir.path(), much_later());
        assert!(again.actions.is_empty(), "{again:?}");
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
        let src = include_str!("mod.rs");
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

    // The pure `should_postpone` test in `decide` cannot see this: a
    // perfectly correct predicate handed a hardcoded `None` postpones
    // nothing, and the run loop needs a live `AppHandle`, so there is no
    // behavioural seam. Pinned structurally instead — the
    // `capture_exclusion` precedent.
    #[test]
    fn the_recovery_pass_asks_the_capture_guard() {
        let src = include_str!("mod.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix");
        assert!(
            src.contains("let active = app.state::<CaptureGuard>().active();")
                && src.contains("should_postpone(active)"),
            "the sweep no longer asks whether a capture is writing here"
        );
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
