//! The ninth sanctioned vault write: where an exported screen capture and
//! its companion note land, and how they get there without ever clobbering.
//!
//! Lives in `core`, not in the screen crate, because the ONE collision
//! suffix scheme (`capture_paths::candidate`) is `pub(crate)` here and its
//! own doc requires every name-minting site to go through it.
//!
//! Mirrors the audio domain exactly in discipline — pairwise reservation,
//! `rename_noreplace` with suffix retry, the note written AFTER the video
//! commits and named from where the video actually landed.

use crate::capture_paths::{candidate, rename_noreplace};
use std::path::{Path, PathBuf};

/// How many times `commit_screen_capture` may lose the move race before it
/// gives up. Only a genuine collision between the reservation and the move
/// costs an attempt, so any real workload uses one or two; the ceiling exists
/// so a predicate bug can never spin silently forever.
const MAX_COMMIT_ATTEMPTS: u32 = 10_000;

/// The pair of names an export reserves before it writes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCaptureNames {
    pub base: String,
    pub final_mp4: PathBuf,
    pub note_md: PathBuf,
}

/// The exported video's file name for a base — the string the companion
/// note embeds, so it has exactly one definition.
pub fn mp4_file_name(base: &str) -> String {
    format!("{base}.mp4")
}

/// PAIRWISE reservation: a base is usable only when BOTH the `.mp4` and the
/// `.md` are free.
///
/// Reserving the two independently would let the video land on `Demo.mp4`
/// while the note took `Demo (2).md`, so the note would embed a file that is
/// not the one beside it. The audio domain reserves four names for the same
/// reason; a screen capture has only two.
pub fn reserve_screen_names(dir: &Path, base: &str) -> ScreenCaptureNames {
    for attempt in 1u32.. {
        let candidate_base = candidate(base, attempt);
        let final_mp4 = dir.join(mp4_file_name(&candidate_base));
        let note_md = dir.join(format!("{candidate_base}.md"));
        if !final_mp4.exists() && !note_md.exists() {
            return ScreenCaptureNames {
                base: candidate_base,
                final_mp4,
                note_md,
            };
        }
    }
    unreachable!("suffix search always terminates")
}

/// The commit-time recheck. Identical to `reserve_screen_names` today — a
/// screen capture stages OUTSIDE the vault, so unlike the audio domain there
/// is no in-progress `.part` sitting in this directory to exclude. It is a
/// separate function anyway so `commit_screen_capture` reads the same as its
/// audio twin and so the two can diverge later without a caller change.
pub fn reserve_final_screen(dir: &Path, base: &str) -> (PathBuf, PathBuf) {
    let names = reserve_screen_names(dir, base);
    (names.final_mp4, names.note_md)
}

/// Move `from` onto `to` without ever replacing it, ACROSS VOLUMES.
///
/// **This is the one way the screen domain differs from the audio one, and
/// it is not a refinement.** Every other sanctioned vault write finalizes a
/// temp that already sits in the destination DIRECTORY, so
/// `rename_noreplace` — a `hard_link` + `remove_file`, falling back to
/// `MoveFileExW` with flags `0` — is exactly right and its own doc says so
/// ("same-directory move (all callers), no copy fallback needed"). An export
/// is the first write in this app whose source and destination are on
/// different filesystems BY DESIGN: the temp is staged under
/// `%LOCALAPPDATA%` (spec §10 — an unapproved capture is not knowledge and
/// must not sit in a vault) while the vault is wherever the user keeps it.
///
/// A cross-volume `hard_link` reports `CrossesDevices` (EXDEV / os error 18;
/// `ERROR_NOT_SAME_DEVICE` on Windows), which `hard_link_error_is_decisive`
/// correctly treats as "try the fallback" — but that fallback is a
/// non-replacing MOVE, which fails across volumes too. Measured here, on two
/// real filesystems: both `hard_link` and `rename` return EXDEV and the
/// destination is never created. Without this function a user whose vault
/// lives on any drive other than the one holding `%LOCALAPPDATA%` could
/// never save a capture at all — every export would fail at the last step,
/// permanently, with an "Invalid cross-device link" the UI would have to
/// show them.
///
/// The copy is EXCLUSIVE-CREATE, so it is never-clobber by construction
/// rather than by a preceding `exists()` check: a destination that appeared
/// since the reservation fails with `AlreadyExists`, which is precisely the
/// signal `commit_screen_capture`'s suffix retry keys on. A copy that fails
/// part-way removes its own partial destination — leaving it would both
/// litter the vault and make every retry read as a fresh collision.
pub(crate) fn move_or_copy_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    move_or_copy_with(from, to, rename_noreplace)
}

/// The decision, with the move injected so BOTH arms are testable on one
/// filesystem. A cross-device failure cannot be provoked in CI (the runner
/// has one volume), and a fallback that is only reachable on a machine
/// nobody tests on is a fallback nobody knows works.
fn move_or_copy_with(
    from: &Path,
    to: &Path,
    mv: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match mv(from, to) {
        Ok(()) => Ok(()),
        // A taken destination is the collision signal, not a reason to
        // copy: copying here would be the clobber this whole module exists
        // to make impossible.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        // Anything else is "this move could not happen HERE" — most often a
        // volume boundary. There is deliberately NO `to.exists()` arm in
        // front of this: the copy's own `create_new` is the arbiter, the
        // same doctrine `commit_screen_capture` states one function down
        // ("THE MOVE IS THE ARBITER, not the `exists()` check"). An
        // `exists()` guard here would be both racy AND — because it fires
        // first — able to hide a copy that had stopped being
        // never-clobber, which is exactly what it did: swapping
        // `create_new` for a truncating open left the whole suite green.
        Err(_) => copy_noreplace(from, to),
    }
}

/// Exclusive-create `to`, stream `from` into it, fsync, then drop `from`.
///
/// A failed `remove_file` of the source is a WARNING, exactly as in
/// `rename_noreplace`: the bytes are already safely at `to`, so returning an
/// error here would send the suffix-retry loop round again against a
/// destination that now exists — a fresh "collision" on every attempt.
fn copy_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut source = std::fs::File::open(from)?;
    let mut dest = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    if let Err(e) = std::io::copy(&mut source, &mut dest).and_then(|_| dest.sync_all()) {
        drop(dest);
        // Our own half-written file. Leaving it would litter the vault AND
        // make the destination look taken to every later attempt.
        let _ = std::fs::remove_file(to);
        return Err(e);
    }
    drop(dest);
    if let Err(e) = std::fs::remove_file(from) {
        log::warn!(
            "screen export: copied {} to {} but could not remove the source ({e}); \
             the staging sweep will collect it",
            from.display(),
            to.display()
        );
    }
    Ok(())
}

/// Move the exported temp into the vault under `base`, never replacing.
///
/// THE MOVE IS THE ARBITER, not the `exists()` check: a destination created
/// by a sync client between the reservation and the rename fails with
/// `AlreadyExists`, advances the suffix and retries. The returned note path
/// is derived from where the VIDEO actually landed, so the note can never
/// name a suffix the video does not have.
///
/// A `rename_noreplace` that linked the destination but could not remove the
/// source returns `Ok(())` by design — do NOT "fix" that into an error here,
/// or this loop spins forever: the destination now exists, so every retry
/// reads as a fresh collision.
///
/// The retry is BOUNDED, deliberately diverging from the audio twin
/// (`capture::recovery::rename_into_reserved`, an unbounded `loop`). That one
/// is safe only because its `reserve_final` predicate provably covers the
/// `.mp3` the move targets, so a collision always advances the suffix. This
/// path is new, and in Task 7 it runs on the named `screen-export` worker
/// thread: any edit that lets the reservation predicate stop covering the
/// move target turns the retry into an infinite spin that wedges the export
/// slot with no error, no log line and no way out. A ceiling turns that into
/// a diagnosable failure and costs nothing — a retry happens only after a
/// move actually lost a race, never once per existing suffix
/// (`reserve_screen_names` walks a whole run of taken names inside a single
/// call). It also keeps the test suite from HANGING under mutation, which
/// this branch's verification method depends on: dropping the `.mp4` half of
/// the pairwise predicate made `committing_never_overwrites_an_existing_file`
/// spin forever instead of going red.
pub fn commit_screen_capture(
    from: &Path,
    dir: &Path,
    base: &str,
) -> Result<(PathBuf, PathBuf), String> {
    for _ in 0..MAX_COMMIT_ATTEMPTS {
        let (mp4, note) = reserve_final_screen(dir, base);
        match move_or_copy_noreplace(from, &mp4) {
            Ok(()) => return Ok((mp4, note)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Some Windows API paths report a taken destination as
            // PermissionDenied rather than AlreadyExists.
            Err(_) if mp4.exists() => continue,
            Err(e) => {
                return Err(format!(
                    "the exported video could not be moved into the vault: {e}"
                ))
            }
        }
    }
    Err(format!(
        "the exported video could not be moved into the vault: gave up after \
         {MAX_COMMIT_ATTEMPTS} attempts to find a free name for {base:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A mover that always reports a cross-volume failure, the exact error
    /// `hard_link` and `rename` both return between two filesystems
    /// (EXDEV / os error 18, measured; `ERROR_NOT_SAME_DEVICE` on Windows).
    fn always_cross_device(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from_raw_os_error(18))
    }

    // THE cross-volume case, which is the ORDINARY one for an export: the
    // temp is staged under %LOCALAPPDATA% and the vault is wherever the
    // user keeps it. Without the copy fallback every export onto another
    // drive fails at the last step, permanently.
    #[test]
    fn a_move_that_cannot_cross_volumes_falls_back_to_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("temp.mp4");
        let to = dir.path().join("Demo.mp4");
        fs::write(&from, b"footage").unwrap();

        move_or_copy_with(&from, &to, always_cross_device).expect("the copy fallback runs");

        assert_eq!(
            fs::read(&to).unwrap(),
            b"footage",
            "the bytes did not arrive"
        );
        assert!(!from.exists(), "the source survived a completed move");
    }

    // ...and the fallback is still NEVER-CLOBBER. `create_new` is what makes
    // that structural rather than a check somebody has to remember; a copy
    // that truncated an existing file here would destroy a user's note or
    // recording with no error at all.
    #[test]
    fn the_copy_fallback_refuses_an_existing_destination_and_leaves_it_intact() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("temp.mp4");
        let to = dir.path().join("Demo.mp4");
        fs::write(&from, b"footage").unwrap();
        fs::write(&to, b"THE USER'S OWN FILE").unwrap();

        let err = move_or_copy_with(&from, &to, always_cross_device)
            .expect_err("an existing destination must never be written through");

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&to).unwrap(), b"THE USER'S OWN FILE");
        assert!(from.exists(), "the source was dropped without landing");
    }

    // The exclusive create is what makes the fallback never-clobber, and it
    // is asserted DIRECTLY rather than only through `move_or_copy_with`: a
    // guard in front of the copy would answer this assertion without the
    // copy itself being safe at all.
    #[test]
    fn the_copy_itself_refuses_to_write_through_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("temp.mp4");
        let to = dir.path().join("Demo.mp4");
        fs::write(&from, b"footage").unwrap();
        fs::write(&to, b"THE USER'S OWN FILE").unwrap();

        let err = copy_noreplace(&from, &to).expect_err("create_new must refuse");

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&to).unwrap(), b"THE USER'S OWN FILE");
        assert!(from.exists(), "the source was dropped without landing");
    }

    // A decisive AlreadyExists is the suffix-retry signal and must be
    // PASSED THROUGH, never answered with a copy. Copying there is exactly
    // the clobber the move refused.
    #[test]
    fn a_move_that_reports_a_taken_destination_is_never_answered_with_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("temp.mp4");
        let to = dir.path().join("Demo.mp4");
        fs::write(&from, b"footage").unwrap();
        // NOTE: `to` deliberately does NOT exist, so the only thing that can
        // stop a copy here is honouring the mover's own verdict.
        let err = move_or_copy_with(&from, &to, |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "taken",
            ))
        })
        .expect_err("AlreadyExists must propagate");

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(!to.exists(), "a copy ran despite a decisive collision");
    }

    // The ordinary same-volume path must still take the MOVE, not the copy:
    // a copy of a multi-gigabyte recording where a rename would do is
    // minutes of disk I/O nobody asked for.
    #[test]
    fn a_move_that_succeeds_is_not_followed_by_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("temp.mp4");
        let to = dir.path().join("Demo.mp4");
        fs::write(&from, b"footage").unwrap();
        let calls = std::cell::Cell::new(0u32);
        move_or_copy_with(&from, &to, |a, b| {
            calls.set(calls.get() + 1);
            rename_noreplace(a, b)
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(fs::read(&to).unwrap(), b"footage");
        assert!(!from.exists());
    }

    fn touch(dir: &std::path::Path, name: &str) {
        fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn a_free_base_reserves_itself_unsuffixed() {
        let dir = tempfile::tempdir().unwrap();
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo");
        assert_eq!(names.final_mp4, dir.path().join("2026-09-20 1432 Demo.mp4"));
        assert_eq!(names.note_md, dir.path().join("2026-09-20 1432 Demo.md"));
    }

    // PAIRWISE. A taken .md alone must still push the .mp4 onto a suffix, or
    // the note ends up beside a video it does not name.
    #[test]
    fn a_taken_note_alone_pushes_the_video_onto_the_next_suffix() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "2026-09-20 1432 Demo.md");
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo (2)");
        assert_eq!(
            names.final_mp4,
            dir.path().join("2026-09-20 1432 Demo (2).mp4")
        );
        assert_eq!(
            names.note_md,
            dir.path().join("2026-09-20 1432 Demo (2).md")
        );
    }

    #[test]
    fn a_taken_video_alone_pushes_the_note_onto_the_next_suffix() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "2026-09-20 1432 Demo.mp4");
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo (2)");
    }

    #[test]
    fn reservation_walks_past_a_run_of_taken_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "2026-09-20 1432 Demo.mp4");
        touch(dir.path(), "2026-09-20 1432 Demo (2).md");
        touch(dir.path(), "2026-09-20 1432 Demo (3).mp4");
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo (4)");
    }

    #[test]
    fn committing_into_a_free_directory_lands_the_plain_base_and_moves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("staged.mp4");
        fs::write(&src, b"video-bytes").unwrap();
        let out = tempfile::tempdir().unwrap();

        let (mp4, note) = commit_screen_capture(&src, out.path(), "2026-09-20 1432 Demo").unwrap();

        assert_eq!(mp4, out.path().join("2026-09-20 1432 Demo.mp4"));
        assert_eq!(note, out.path().join("2026-09-20 1432 Demo.md"));
        assert_eq!(fs::read(&mp4).unwrap(), b"video-bytes");
        assert!(!src.exists(), "the staged temp was left behind");
    }

    // THE property this whole module exists for: an existing user file is
    // never overwritten, whatever its name collides with.
    #[test]
    fn committing_never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("staged.mp4");
        fs::write(&src, b"new-video").unwrap();
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("2026-09-20 1432 Demo.mp4"), b"PRECIOUS").unwrap();

        let (mp4, note) = commit_screen_capture(&src, out.path(), "2026-09-20 1432 Demo").unwrap();

        assert_eq!(mp4, out.path().join("2026-09-20 1432 Demo (2).mp4"));
        assert_eq!(note, out.path().join("2026-09-20 1432 Demo (2).md"));
        assert_eq!(
            fs::read(out.path().join("2026-09-20 1432 Demo.mp4")).unwrap(),
            b"PRECIOUS",
            "the pre-existing file was overwritten"
        );
        assert_eq!(fs::read(&mp4).unwrap(), b"new-video");
    }

    // M3/M5 GUARD — the retry loop's ONLY exercise.
    //
    // Every other test here reserves a name that is still free when the move
    // runs, so the `exists()` check — not the move — is what keeps the
    // pre-existing file. Swapping `rename_noreplace` for `std::fs::rename`,
    // or turning the `AlreadyExists` retry into an error, leaves all of them
    // green: the loop body never runs twice. Only a real collision BETWEEN
    // the reservation and the move exercises the rule the module doc states,
    // THE MOVE IS THE ARBITER, and threads racing one base stage exactly
    // that. Correct code passes under every interleaving — `rename_noreplace`
    // never replaces and each retry mints a fresh suffix — so this can only
    // go red when the property is genuinely broken.
    #[test]
    fn concurrent_commits_on_one_base_never_lose_a_file() {
        const N: usize = 8;
        let staging = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let srcs: Vec<std::path::PathBuf> = (0..N)
            .map(|i| {
                let p = staging.path().join(format!("staged-{i}.mp4"));
                fs::write(&p, format!("payload-{i}").as_bytes()).unwrap();
                p
            })
            .collect();

        let barrier = std::sync::Barrier::new(N);
        let landed: Vec<std::path::PathBuf> = std::thread::scope(|scope| {
            let handles: Vec<_> = srcs
                .iter()
                .map(|src| {
                    let barrier = &barrier;
                    let out = out.path();
                    scope.spawn(move || {
                        // Release every thread together so several reserve the
                        // same free base before any of them moves onto it.
                        barrier.wait();
                        commit_screen_capture(src, out, "Clip").unwrap().0
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        let mut distinct = landed.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), N, "two commits landed on the same path");

        let mut payloads: Vec<String> = distinct
            .iter()
            .map(|p| String::from_utf8(fs::read(p).unwrap()).unwrap())
            .collect();
        payloads.sort();
        let mut expected: Vec<String> = (0..N).map(|i| format!("payload-{i}")).collect();
        expected.sort();
        assert_eq!(payloads, expected, "a committed file was overwritten");
    }

    // The note name comes from WHERE THE VIDEO LANDED, not from the base the
    // caller asked for. A note named from the request would embed a file
    // that is not beside it as soon as the video took a suffix.
    #[test]
    fn the_note_path_is_derived_from_the_landed_video_not_the_requested_base() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("staged.mp4");
        fs::write(&src, b"v").unwrap();
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("Clip.mp4"), b"taken").unwrap();
        fs::write(out.path().join("Clip (2).mp4"), b"taken too").unwrap();

        let (mp4, note) = commit_screen_capture(&src, out.path(), "Clip").unwrap();

        assert_eq!(mp4.file_stem().unwrap(), "Clip (3)");
        assert_eq!(note.file_stem().unwrap(), "Clip (3)");
        assert_eq!(mp4.parent(), note.parent());
    }

    #[test]
    fn committing_a_missing_source_is_an_error_not_a_panic_or_an_empty_file() {
        let out = tempfile::tempdir().unwrap();
        let err = commit_screen_capture(
            std::path::Path::new("/definitely/not/here.mp4"),
            out.path(),
            "Clip",
        )
        .unwrap_err();
        assert!(
            err.contains("could not be moved"),
            "unexpected message: {err}"
        );
        assert!(!out.path().join("Clip.mp4").exists());
    }

    #[test]
    fn the_stop_time_recheck_ignores_nothing_but_still_pairs_both_extensions() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "Clip.md");
        let (mp4, note) = reserve_final_screen(dir.path(), "Clip");
        assert_eq!(mp4, dir.path().join("Clip (2).mp4"));
        assert_eq!(note, dir.path().join("Clip (2).md"));
    }

    #[test]
    fn the_video_file_name_is_the_base_plus_mp4() {
        assert_eq!(
            mp4_file_name("2026-09-20 1432 Demo"),
            "2026-09-20 1432 Demo.mp4"
        );
    }
}
