//! The PURE decision half of staging recovery: what a name IS, whether a
//! `.part` holds anything worth keeping, whether a file is old enough to
//! touch, and whether the sweep may run at all.
//!
//! Split out of `screen_recovery.rs` (docs/Gaps.md GAP-147: that file sat at
//! exactly 800/800 nonblank lines, so the next line added to it breached the
//! Rust cap) along the seam its own module doc already named. **Nothing here
//! touches the filesystem**, which is the point: this module deletes
//! people's files by proxy, and every judgement that leads to a deletion is
//! a function of its arguments and tested as one. `mod.rs` keeps the
//! `read_dir` walk that carries those judgements out.
//!
//! This is a directory module's sibling, so `lib.rs` is unchanged —
//! `mod screen_recovery;` resolves `screen_recovery/mod.rs` identically.

use std::time::{Duration, SystemTime};

use vault_buddy_screen::{mp4_boxes, staging};

use crate::capture_guard::CaptureKind;
use crate::editor_commands::is_safe_base;
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
pub(super) enum Entry {
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
/// export temp as a promotable capture.
///
/// That ordering used to COST footage: a capture whose base genuinely ended in
/// `.export` had its orphaned `.part` deleted as an export temp rather than
/// promoted, skipping `part_holds_footage` entirely — so a crash while
/// recording a window titled e.g. "Build.export" destroyed exactly what this
/// sweep exists to rescue. **The two are no longer indistinguishable by name.**
/// They only ever were because `staging_title::sanitize_title` permitted a
/// title to END in the marker; it now disambiguates that one ending, and a capture base
/// is that sanitized title behind a `YYYY-MM-DD HHmm ` prefix, so this app
/// cannot mint a base ending in `EXPORT_PART_INFIX` at all. The fix is at the
/// source, where identity is decided, instead of arbitrated here.
///
/// It is not retroactive: a capture staged under such a base BEFORE that fix
/// keeps it, and its orphaned `.part` is still swept as an export temp. Only
/// new captures are covered.
pub(super) fn classify(file_name: &str) -> Entry {
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
pub(super) fn owned(stem: &str, make: fn(String) -> Entry) -> Entry {
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
pub(super) fn part_holds_footage(prefix: &[u8]) -> bool {
    let scan = mp4_boxes::scan(prefix);
    scan.has_moov() && scan.fragment_count() > 0
}

/// Rule 3: never sweep while the app is WRITING into staging. Three ways it
/// can be, and each has to be asked separately.
///
/// `CaptureGuard` covers two of them — audio means the app is writing
/// elsewhere, screen means a `.part` right here is live. **`exporting` is
/// the third, and it was missing.** An export writes
/// `.<base>.export.mp4.part` into this very directory, and it does NOT hold
/// the capture guard (it only READS it, because a claim would need a second
/// `release(CaptureKind::Screen)` site and a structural test pins that at
/// exactly one — GAP-141). So the live temp was protected by the 60 s
/// staleness window alone, and that window does not hold: `reencode_args`
/// builds `trim`/`atrim` + `concat` with no `-ss`, so ffmpeg decodes from
/// zero and emits no output packets until the first KEPT span. An edited
/// export of a long recording that keeps only late footage writes its header
/// at T0 and then nothing for minutes — its mtime never advances, it reads
/// stale, and `sweep_staging_dir` classifies it `ExportTemp` and deletes it.
/// The recovery thread is alive for exactly that long, because a `pending`
/// file keeps it retrying every 90 s for up to 24 h.
///
/// On Windows the unlink itself then FAILS — ffmpeg's output handle is
/// opened through the CRT without `FILE_SHARE_DELETE`, so both
/// `DeleteFileW` and the `FileDispositionInfoEx` path `std::fs::remove_file`
/// prefers return a sharing violation — and `delete` degrades to a
/// `log::warn!`. That is luck, not a guard, and it is reasoned from the
/// Win32 sharing rules rather than measured (no CI runner here is Windows).
/// The guard below is the actual rule.
///
/// One process-wide `ExportState` reservation, so a bool is enough: any live
/// export means somebody is writing a temp in this directory, and no sweep
/// in this pass may run.
pub(super) fn should_postpone(active: Option<CaptureKind>, exporting: bool) -> bool {
    active.is_some() || exporting
}

/// Pure staleness, so the clock cases are testable without real mtimes — the
/// `capture::recovery::is_stale_at` precedent including its skew branch: a
/// live file's mtime tracks "now", so small skew reads as fresh, while a gap
/// beyond the window means a clock jump stranded an orphan.
pub(super) fn is_stale_at(modified: SystemTime, now: SystemTime, stale_after: Duration) -> bool {
    match now.duration_since(modified) {
        Ok(age) => age >= stale_after,
        Err(e) => e.duration() >= stale_after,
    }
}

#[cfg(test)]
pub(super) mod fixtures {
    /// One top-level ISO-BMFF box: 4-byte big-endian size, 4-character type,
    /// payload. `mp4_boxes::scan` walks exactly this.
    pub(in crate::screen_recovery) fn bx(kind: &str, payload: &[u8]) -> Vec<u8> {
        let size = (8 + payload.len()) as u32;
        let mut out = size.to_be_bytes().to_vec();
        out.extend_from_slice(kind.as_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A `.part` a player could open: an index plus one closed fragment.
    pub(in crate::screen_recovery) fn footage() -> Vec<u8> {
        let mut v = header_only();
        v.extend(bx("moof", b"...."));
        v.extend(bx("mdat", b"framedata"));
        v
    }

    /// A bare fMP4 header: what a capture that died before its first
    /// fragment closed leaves behind.
    pub(in crate::screen_recovery) fn header_only() -> Vec<u8> {
        let mut v = bx("ftyp", b"isom");
        v.extend(bx("moov", b"...."));
        v
    }

    pub(in crate::screen_recovery) const BASE: &str = "2026-09-20 1432 Demo";
    // Unix-only because its one user is: the symlink test in mod.rs is
    // cfg(unix). Compiled everywhere, it is dead code on Windows and
    // `clippy -D warnings` fails there while Linux CI stays green.
    #[cfg(unix)]
    pub(in crate::screen_recovery) const OTHER: &str = "2026-09-20 1500 Other";
    pub(in crate::screen_recovery) const ORPHAN: &str = "2026-09-20 1330 Orphan";
    pub(in crate::screen_recovery) const KEPT: &str = "2026-09-20 1340 Kept";
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::screen_recovery::STALE_AFTER;

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

    // REGRESSION (silent footage loss): `classify` must check the export
    // shape first, and that used to mean an orphaned `.part` from a capture
    // of a window titled "Build.export" was DELETED as an abandoned
    // transcode -- skipping `part_holds_footage`, the one check standing
    // between the sweep and real footage.
    //
    // The guarantee now lives in `staging_title::sanitize_title`, one crate
    // away, so pin the two halves together here: nothing else spans them, and a
    // "simplification" of that disambiguation would otherwise redden
    // nothing on this side.
    #[test]
    fn a_window_title_ending_in_the_export_marker_still_stages_as_a_capture() {
        let base = format!(
            "2026-09-20 1432 {}",
            vault_buddy_screen::staging_title::sanitize_title("Build.export")
        );
        assert_eq!(
            classify(&staging::part_file_name(&base)),
            Entry::Part(base.clone()),
            "an orphaned capture part was resolved as an export temp and would \
             have been deleted without ever being checked for footage"
        );
        // ...and that capture's OWN export temp is still recognised as one,
        // so the disambiguation buys the capture nothing at the transcode's
        // expense.
        assert_eq!(
            classify(&staging::export_part_file_name(&base)),
            Entry::ExportTemp(base)
        );
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
        assert!(should_postpone(Some(CaptureKind::Screen), false));
        assert!(should_postpone(Some(CaptureKind::Audio), false));
        assert!(!should_postpone(None, false));
    }

    // REGRESSION (fix wave): the sweep knew about `CaptureGuard` and nothing
    // about `ExportState`, so a LIVE export's `.<base>.export.mp4.part` was
    // protected by the 60 s staleness window alone. `reencode_args` builds
    // `trim`/`atrim` + `concat` with no `-ss`, so ffmpeg decodes from zero
    // and emits nothing until the first kept span: an edited export keeping
    // only late footage of a long recording writes its header at T0 and then
    // nothing for minutes, its mtime never advances, and it reads stale.
    // The recovery thread is alive for exactly that long, because `pending`
    // stays non-zero.
    #[test]
    fn recovery_is_postponed_while_an_export_is_running() {
        assert!(should_postpone(None, true));
        // ...and an export plus a capture is still a postponement, not a
        // cancellation of one by the other.
        assert!(should_postpone(Some(CaptureKind::Audio), true));
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
}
