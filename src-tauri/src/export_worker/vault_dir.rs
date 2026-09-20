//! The VAULT DIRECTORY an export writes into: create it contained, measure
//! whether it can hold the result, and — when the save does not happen —
//! put the vault back exactly as it was found.
//!
//! Split out of `mod.rs` for size (the fix that added the rollback took that
//! file to 905 nonblank against the repo's 800-line Rust cap, and
//! `scripts/loc-baseline.json` is shrink-only). The seam is a real one:
//! everything here is about the DIRECTORY — the one thing an export touches
//! in a user's vault before it has any bytes to put there — while `mod.rs`
//! owns the export's sequence and the commit. `export_worker` is a directory
//! module, so `lib.rs` is unchanged.

use std::path::{Path, PathBuf};

use vault_buddy_core::capture_paths::assert_path_inside_vault;
use vault_buddy_core::screen_capture_config::{export_size_estimate_bytes, export_space_shortfall};
use vault_buddy_core::timeline::Timeline;
use vault_buddy_core::vault_config::VaultCaptureConfig;
use vault_buddy_screen::disk;
use vault_buddy_screen::ffmpeg_args::EncodeSettings;

/// Refuse a save that would fill a disk — on BOTH volumes it touches.
///
/// The temp is written into staging (`%LOCALAPPDATA%`) and only then lands
/// in the vault, which is very often a different drive. Measuring one of
/// them is measuring the wrong one half the time.
pub(super) fn check_free_space(
    timeline: &Timeline,
    settings: &EncodeSettings,
    cfg: &VaultCaptureConfig,
    dirs: &[&Path],
) -> Result<(), String> {
    let needed = export_size_estimate_bytes(
        timeline.output_duration_ms(),
        settings.width,
        settings.height,
        cfg.screen_fps,
        cfg.screen_quality,
    );
    let shortfall = dirs
        .iter()
        .filter_map(|dir| export_space_shortfall(needed, disk::free_bytes(dir)))
        .max();
    match shortfall {
        // An UNMEASURABLE volume yields None and never refuses: a failed
        // probe must not read as "no space left".
        None => Ok(()),
        Some(short) => Err(format!(
            "There is not enough free space to save this capture — about {} more is needed.",
            human_mib(short)
        )),
    }
}

fn human_mib(bytes: u64) -> String {
    let mib = bytes.div_ceil(1024 * 1024);
    if mib >= 1024 {
        format!("{:.1} GB", mib as f64 / 1024.0)
    } else {
        format!("{mib} MB")
    }
}

/// The dated directory, asserted inside the vault BEFORE and AFTER creation.
///
/// PRE stops `create_dir_all` following a pre-existing symlink or junction
/// and building our tree outside the vault; POST closes the window in which
/// one is swapped in underneath us. The audio path checks only afterwards;
/// this is the document-import discipline, which is stronger.
///
/// Returns the directories this call CREATED, deepest first, so a save that
/// then does not happen can put the vault back exactly as it found it —
/// `rollback_export_dir` below.
pub(super) fn prepare_export_dir(vault_path: &Path, dir: &Path) -> Result<Vec<PathBuf>, String> {
    prepare_export_dir_confirmed(vault_path, dir, &assert_path_inside_vault)
}

/// `prepare_export_dir` with its containment check as a parameter.
///
/// The POST check fires only when a symlink or junction is swapped in
/// between the PRE check and `create_dir_all` — a real TOCTOU window that
/// cannot be produced on demand from a test, and the one case that can leak
/// directories OUTSIDE the vault. So the check is a seam: a test drives the
/// second call into failing and asserts the tree is gone, which is the only
/// way that arm is executed anywhere.
fn prepare_export_dir_confirmed(
    vault_path: &Path,
    dir: &Path,
    confirm: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<Vec<PathBuf>, String> {
    confirm(vault_path, dir)?;
    let created = missing_ancestors(vault_path, dir);
    // ONE arm out for both remaining failures, because both can fire with
    // part of the tree already on disk. `create_dir_all` makes the parents
    // and then fails on a leaf it cannot make; the POST check fires only
    // when something was swapped in while we were creating, which is the
    // one case where what we leave behind can be OUTSIDE the vault. Each
    // used to return on its own `?` with `created` dropped on the floor.
    match create_and_confirm(vault_path, dir, confirm) {
        Ok(()) => Ok(created),
        Err(e) => {
            rollback_export_dir(&created);
            Err(e)
        }
    }
}

/// The two steps that can fail with part of the tree already created.
/// Separate so `prepare_export_dir_confirmed` has exactly one place to roll
/// back from, rather than one per `?`.
fn create_and_confirm(
    vault_path: &Path,
    dir: &Path,
    confirm: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Could not create the folder for this capture: {e}"))?;
    confirm(vault_path, dir)
}

/// The ancestors of `dir` that do not exist YET, deepest first, stopping at
/// the vault root.
///
/// Sampled BEFORE `create_dir_all`, which is the only moment the answer is
/// knowable: afterwards every one of them exists and nothing distinguishes
/// the folder this export made from the folder the user has had since March.
fn missing_ancestors(vault_path: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for ancestor in dir.ancestors() {
        if ancestor == vault_path || ancestor.exists() {
            break;
        }
        out.push(ancestor.to_path_buf());
    }
    out
}

/// Put the vault back: remove the directories THIS export created, and only
/// while they are still empty.
///
/// **`remove_dir`, never `remove_dir_all`.** A directory that is not empty
/// holds something this export did not put there — a capture a concurrent
/// save landed, a note the user dropped in, an unrelated file — and the
/// error `remove_dir` returns for it IS the guard, not an afterthought. A
/// directory that already existed is never in `created` in the first place,
/// so it cannot be reached from here at all.
///
/// Deepest first (`2026/09` before `2026`), and the first one that will not
/// go stops the walk: every shallower directory now holds it, so every
/// further `remove_dir` would fail anyway and logging each would be noise.
/// A directory that is not THERE is the one exception and is skipped rather
/// than stopping the walk — `created` is sampled before `create_dir_all`,
/// so a create that failed part way leaves entries here that were never
/// made, and they are the deepest ones. Stopping on the first of those
/// would keep every directory that really was created.
///
/// Best-effort and never an error: the user's save already failed, and a
/// folder that outlives it is litter, not loss.
pub(super) fn rollback_export_dir(created: &[PathBuf]) {
    for dir in created {
        // Nothing there is not the same as "not empty", and only the second
        // is the guard. `created` is sampled BEFORE `create_dir_all`, so a
        // create that failed part way leaves entries here that were never
        // made — and they are the DEEPEST ones, so stopping on the first of
        // them would keep every directory that really was created.
        if !dir.exists() {
            log::info!(
                "screen export: {} was never created; continuing",
                dir.display()
            );
            continue;
        }
        match std::fs::remove_dir(dir) {
            Ok(()) => log::info!(
                "screen export: removed {}, empty after a save that did not happen",
                dir.display()
            ),
            Err(e) => {
                log::info!("screen export: keeping {}: {e}", dir.display());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // The PRE-creation containment check, behaviourally: a symlinked
    // captures folder must be refused WITHOUT our directory tree being
    // built at the other end of it.
    #[test]
    fn a_symlinked_capture_folder_is_refused_before_anything_is_created() {
        let vault = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = vault.path().join("Screen Captures");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        #[cfg(not(unix))]
        std::os::windows::fs::symlink_dir(outside.path(), &link).unwrap();

        let dir = link.join("2026").join("09");
        let err = prepare_export_dir(vault.path(), &dir).expect_err("an escape must be refused");
        assert!(err.contains("outside the vault"), "{err}");
        assert!(
            !outside.path().join("2026").exists(),
            "create_dir_all ran through the symlink before the check"
        );
    }

    // REGRESSION (fix wave): every way out of an export that is NOT a save
    // — the user pressing Cancel (spec §14's ordinary exit), a disk-space
    // refusal, an ffmpeg failure, a commit failure — used to leave an empty
    // `Screen Captures/YYYY/MM` in the user's notes forever. Nothing in this
    // file, `export_commands.rs` or `screen/src/export.rs` removed a
    // directory at all.
    #[test]
    fn a_save_that_does_not_happen_leaves_no_empty_folder_in_the_vault() {
        let vault = tempfile::tempdir().unwrap();
        let dir = vault.path().join("Screen Captures").join("2026").join("09");
        let created = prepare_export_dir(vault.path(), &dir).unwrap();

        // ...and here the export refuses, is cancelled, or fails to commit.
        rollback_export_dir(&created);

        // Whatever reached the vault must be gone again.
        let stray: Vec<_> = fs::read_dir(vault.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(
            stray.is_empty(),
            "a save that never happened left this behind: {stray:?}"
        );
    }

    // The other half of the rule, and the half a "the vault is empty again"
    // fixture cannot see: a rollback that simply deleted the tree would pass
    // the test above too. A folder the user already had is NEVER in
    // `created`, so it survives — proven with a file in it, so the survival
    // cannot be an accident of `remove_dir` refusing a non-empty directory.
    #[test]
    fn a_rollback_never_removes_a_folder_the_user_already_had() {
        let vault = tempfile::tempdir().unwrap();
        let root = vault.path().join("Screen Captures");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("last week.mp4"), b"the user's own").unwrap();

        let dir = root.join("2026").join("09");
        let created = prepare_export_dir(vault.path(), &dir).unwrap();
        assert_eq!(created, vec![dir.clone(), root.join("2026")]);

        rollback_export_dir(&created);
        assert!(!dir.exists(), "the dated folder this export made survived");
        assert!(!root.join("2026").exists(), "the year folder survived");
        assert!(root.is_dir(), "the user's own folder was removed");
        assert_eq!(
            fs::read(root.join("last week.mp4")).unwrap(),
            b"the user's own"
        );
    }

    // `remove_dir`, never `remove_dir_all`: anything in the folder is
    // something this export did not put there — a concurrent save's capture,
    // a note the user dropped in — and the error `remove_dir` returns for it
    // IS the guard.
    #[test]
    fn a_rollback_keeps_a_folder_that_is_no_longer_empty() {
        let vault = tempfile::tempdir().unwrap();
        let dir = vault.path().join("Screen Captures").join("2026").join("09");
        let created = prepare_export_dir(vault.path(), &dir).unwrap();
        fs::write(dir.join("somebody else.mp4"), b"not ours").unwrap();

        rollback_export_dir(&created);
        assert!(dir.is_dir(), "a folder with a file in it was removed");
        assert_eq!(
            fs::read(dir.join("somebody else.mp4")).unwrap(),
            b"not ours"
        );
        // ...and the walk stopped there rather than reporting on ancestors
        // that now cannot go either.
        assert!(dir.parent().unwrap().is_dir());
    }

    #[test]
    fn an_ordinary_dated_folder_inside_the_vault_is_created() {
        let vault = tempfile::tempdir().unwrap();
        let dir = vault.path().join("Screen Captures").join("2026").join("09");
        prepare_export_dir(vault.path(), &dir).unwrap();
        assert!(dir.is_dir());
    }
    // The POST containment assert is the ONE refusal that can fire after
    // `create_dir_all` has already built part of the tree -- and the one
    // whose whole reason for existing is a symlink swapped in underneath
    // us, so what it leaves behind can be OUTSIDE the vault. It returned on
    // `?` with `created` dropped on the floor, leaking every directory this
    // export had just made, for a save that was refused precisely because
    // something was wrong with that path.
    //
    // Driven through the `confirm` seam because the race cannot be produced
    // on demand: PRE canonicalises the nearest EXISTING ancestor, so every
    // symlink a test can plant is refused before anything is created. This
    // is the only execution of that arm anywhere.
    #[test]
    fn a_containment_failure_after_creation_removes_what_it_created() {
        let vault = tempfile::tempdir().expect("tempdir");
        let root = vault.path().join("Screen Captures");
        fs::create_dir_all(&root).expect("the capture root already exists");
        let dir = root.join("2026").join("09");

        let calls = std::cell::Cell::new(0u32);
        let confirm = |v: &Path, d: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                // PRE: genuinely inside, so the create really happens.
                assert_path_inside_vault(v, d)
            } else {
                // POST: a junction was swapped in while we were creating.
                Err("Path escapes the vault".to_string())
            }
        };

        let err = prepare_export_dir_confirmed(vault.path(), &dir, &confirm)
            .expect_err("a post-create containment failure must refuse");
        assert!(err.contains("escapes"), "unexpected error: {err}");
        assert_eq!(calls.get(), 2, "both containment checks must run");
        assert!(
            !root.join("2026").exists(),
            "the refused export leaked {} -- directories it created for a save that \
             never happened, on the one path where they can be outside the vault",
            root.join("2026").display()
        );
        assert!(
            root.exists(),
            "the pre-existing capture root is not this export's to remove"
        );
    }

    // The other post-`missing_ancestors` failure: `create_dir_all` itself.
    // It is the likelier partial-create -- the leaf fails and its parents
    // are already on disk -- and it left them behind for the same reason.
    // Forced with a leaf longer than NAME_MAX rather than with permissions,
    // so the test says nothing about who is running it.
    #[test]
    fn a_create_that_fails_leaves_no_new_directories_behind() {
        let vault = tempfile::tempdir().expect("tempdir");
        let root = vault.path().join("Screen Captures");
        fs::create_dir_all(&root).expect("the capture root already exists");
        let dir = root.join("2026").join("09").join("x".repeat(300));

        let err = prepare_export_dir(vault.path(), &dir).expect_err("an impossible leaf must fail");
        assert!(err.contains("Could not create"), "unexpected error: {err}");
        assert!(
            !root.join("2026").exists(),
            "the failed create leaked {}",
            root.join("2026").display()
        );
        assert!(root.exists(), "the pre-existing capture root must survive");
    }

    // The dated directory is asserted inside the vault BEFORE create_dir_all
    // (so a pre-existing symlink is not followed) and AFTER it (closing the
    // swap-in race). The PRE half also has a behavioural test above; the
    // POST half is driven through the `confirm` seam, because the race it
    // closes cannot be provoked deterministically -- but only this can see
    // that the seam is WIRED to the real check rather than to a stub, and
    // that the two calls really straddle the create.
    #[test]
    fn the_export_directory_is_asserted_inside_the_vault_before_and_after_creation() {
        let src = include_str!("vault_dir.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix");
        let body_of = |sig: &str| -> &str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
            let end = src[start..]
                .find("\n}\n")
                .map(|i| start + i)
                .unwrap_or_else(|| panic!("{sig} must end"));
            &src[start..end]
        };

        // The seam carries the REAL containment check in production.
        assert!(
            body_of("pub(super) fn prepare_export_dir(").contains("&assert_path_inside_vault"),
            "prepare_export_dir must hand the real containment check to the seam"
        );

        // PRE: before anything is sampled or created.
        let pre = body_of("fn prepare_export_dir_confirmed(");
        let first_confirm = pre
            .find("confirm(vault_path, dir)?;")
            .expect("the PRE check");
        let create_call = pre.find("create_and_confirm(").expect("the create step");
        assert!(
            first_confirm < create_call,
            "no containment assertion before the create"
        );

        // POST: after create_dir_all, inside the one step that can fail
        // with part of the tree on disk.
        let post = body_of("fn create_and_confirm(");
        let create = post.find("create_dir_all(").expect("create_dir_all");
        let second_confirm = post
            .find("confirm(vault_path, dir)")
            .expect("the POST check");
        assert!(
            create < second_confirm,
            "no containment assertion after create_dir_all"
        );
    }

    // `remove_dir`, never `remove_dir_all` -- pinned here as well as
    // behaviourally, because the behavioural test only sees the ONE
    // directory it staged, while this sees the whole module.
    #[test]
    fn nothing_in_the_vault_directory_lifecycle_removes_recursively() {
        let src = include_str!("vault_dir.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix");
        assert!(
            !src.contains("remove_dir_all("),
            "a recursive remove reached the vault write path"
        );
    }
}
