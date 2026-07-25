//! Relocating tasks between (or out of) list folders: `move_task_to_list`
//! and the list-delete verb built on it, `delete_task_list`, plus its
//! outcome/error types.
//!
//! Split out of `lists.rs` (now `lists/mod.rs`) purely for the crate's
//! nonblank-LOC cap — a pure move, not a rewrite. `move_task_to_list` and
//! `delete_task_list` are `pub fn` (and `DeleteListOutcome`/`DeleteListError`
//! are `pub struct`s) exactly as before, re-exported from `lists`'s own
//! namespace (see the `use relocate::{...}` line in `lists/mod.rs`), so
//! `tasks::move_task_to_list` / `tasks::delete_task_list` still resolve
//! identically for every existing caller — this split changed no call site
//! anywhere in the crate.

use std::path::{Path, PathBuf};

/// Move a task file into another list's folder, keeping its basename. The
/// source is canonicalized and must live inside the canonical root (the
/// `update_task_fields` gate); the target list is validated lexically
/// (relative, no `..`/absolute components — multi-segment allowed, existing
/// nested lists are real targets), created if a just-deleted folder needs
/// resurrecting (lists are folders), and containment-asserted before and
/// after. The landing uses `rename_noreplace` + the shared ` (N)` suffix
/// scheme — a collision never clobbers the occupant. Moving a task to the
/// list it is already in is a no-op `Ok`. Returns the landed path.
pub fn move_task_to_list(root: &Path, path: &Path, list: &str) -> Result<PathBuf, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    // Re-read the frontmatter and refuse a file that is no longer a
    // `type: Task` document, mirroring the status/field writers (set_fields
    // rejects a non-task). A list-only editor save skips update_task, so
    // without this a note edited outside the app to drop `type: Task` — or a
    // file swapped in at this path — could still be moved around the tasks
    // folder as if the stale task write still applied.
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    if !super::super::doc::is_task(&content) {
        return Err(
            "This file is no longer a task document — reopen the list to refresh.".to_string(),
        );
    }
    // Lexical gate on the target list — rejected before any filesystem access.
    let normalized = super::normalize_list_rel(list)?;
    let target_dir = if normalized.is_empty() {
        canon_root.clone()
    } else {
        canon_root.join(&normalized)
    };
    crate::capture_paths::assert_path_inside_vault(&canon_root, &target_dir)?;
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Could not create the list folder: {e}"))?;
    let canon_target_dir = std::fs::canonicalize(&target_dir)
        .map_err(|e| format!("Cannot resolve the list folder: {e}"))?;
    if !canon_target_dir.starts_with(&canon_root) {
        return Err("List folder resolves outside the tasks folder".to_string());
    }
    if canon_path.parent() == Some(canon_target_dir.as_path()) {
        return Ok(canon_path); // already in that list — nothing to move
    }
    let stem = canon_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = canon_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for attempt in 1u32.. {
        let candidate = canon_target_dir.join(format!(
            "{}{ext}",
            crate::capture_paths::candidate(&stem, attempt)
        ));
        match crate::capture_paths::rename_noreplace(&canon_path, &candidate) {
            Ok(()) => {
                // `rename_noreplace`'s hard-link path is deliberately lenient:
                // if it links to the destination but can't unlink the source
                // (a Windows AV/indexer holding it open), it returns Ok and
                // leaves the source behind — right for capture, where a stray
                // `.part` is later re-finalized as a `(recovered)` duplicate
                // and no audio is ever lost. A task move can't tolerate that:
                // the same document at both the old and new path would surface
                // as a DUPLICATE task in both lists on the next scan. So treat
                // a surviving source as a FAILED move — roll back the copy we
                // just linked into the destination (same inode; removing it
                // leaves the original intact) and error, so the file stays at
                // exactly one path and the caller doesn't adopt the new one
                // (Codex, PR #53 re-review).
                if canon_path.exists() {
                    if let Err(e) = std::fs::remove_file(&candidate) {
                        log::warn!(
                            "move_task_to_list: source {canon_path:?} survived the move and the \
                             rolled-back copy {candidate:?} could not be removed ({e})"
                        );
                    }
                    return Err(
                        "Could not move the task: the original file could not be removed"
                            .to_string(),
                    );
                }
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not move the task: {e}")),
        }
    }
    unreachable!("suffix search always terminates")
}

/// Outcome of deleting a list: how many of its own tasks were moved to the
/// tasks root, whether the (now-empty) folder was removed, and their landed
/// paths — the service layer repairs each one's own stale `parent` link there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteListOutcome {
    pub moved: usize,
    pub folder_removed: bool,
    pub landed: Vec<PathBuf>,
}

/// A `delete_task_list` failure, carrying whatever it ALREADY relocated
/// before the failure (GAP-64's partial-failure window): a later move in the
/// loop, or the final folder removal, can fail after earlier direct tasks
/// already landed at the tasks root. Plain `String` would discard those
/// landed paths the instant `?` returns — and they are exactly what the
/// SERVICE layer needs to repair each landed child's own stale `parent` link
/// (`services::tasks::lists::delete_task_list`, which needs the real vault
/// root `repair_parent_link` requires — see that call site's own doc comment
/// for why this stays a two-crate handoff instead of core repairing inline).
/// `landed` is empty for a failure before any move ran (an invalid list name,
/// a missing folder, an escaping path) — nothing to repair yet in that case.
#[derive(Debug)]
pub struct DeleteListError {
    pub message: String,
    pub landed: Vec<PathBuf>,
}

impl std::fmt::Display for DeleteListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Every early-exit `?` in `delete_task_list` (name validation, canonicalize,
/// containment, "list no longer exists") fires BEFORE any move runs, so it
/// carries no landed paths — this is what lets those call sites keep using
/// plain `?` instead of hand-wrapping each one.
impl From<String> for DeleteListError {
    fn from(message: String) -> Self {
        Self {
            message,
            landed: Vec::new(),
        }
    }
}

/// Delete a list: move its OWN direct `type: Task` files to the tasks root
/// (No list), then remove the folder if it is now empty. A folder still
/// holding nested sub-lists or foreign (non-task) files is kept — those are
/// never moved or deleted.
///
/// `id_property` is the vault's configured task-id key (or `None` when IDs are
/// off), threaded down exactly like `list_tasks`. A relocation to No list is a
/// structural move — the same category `services::move_task_to_list` stamps —
/// so a legacy task without an id picks one up here too; only a status
/// toggle/archive is excluded. The stamp is best-effort (a failure warns, never
/// fails the delete) and never overwrites an existing id. Unlike the move/edit
/// paths it returns no per-task id: the frontend reloads the task list after a
/// delete regardless (GAP-64), so the reload surfaces the fresh ids.
///
/// The error side is `DeleteListError`, not `String` — see its own doc
/// comment. Every early `?` below converts through `From<String>` (empty
/// `landed`, since nothing has moved yet at that point); only the move loop
/// and the folder removal construct one with the paths ALREADY relocated.
pub fn delete_task_list(
    root: &Path,
    list: &str,
    id_property: Option<&str>,
) -> Result<DeleteListOutcome, DeleteListError> {
    let rel = super::normalize_list_rel(list)?;
    if rel.is_empty() {
        return Err("The tasks root is not a list and cannot be deleted."
            .to_string()
            .into());
    }
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let list_dir = canon_root.join(&rel);
    crate::capture_paths::assert_path_inside_vault(&canon_root, &list_dir)?;
    if !list_dir.is_dir() {
        return Err("That list no longer exists — reopen the list to refresh."
            .to_string()
            .into());
    }
    // Collect the direct task files first (don't mutate while iterating).
    let mut task_files: Vec<PathBuf> = Vec::new();
    for (path, ft, name) in crate::vault_walk::dir_entries(&list_dir) {
        if ft.is_file() && name.ends_with(".md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if super::super::doc::is_task(&content) {
                    task_files.push(path);
                }
            }
        }
    }
    let mut moved = 0;
    let mut landed_paths = Vec::with_capacity(task_files.len());
    // Partial-failure semantics (GAP-64): if the Nth move fails, files
    // 1..N-1 already relocated to the tasks root — `moved` is discarded and
    // the caller gets an opaque Err with no signal the vault was partially
    // mutated. No data loss (every moved file rode move_task_to_list's
    // never-clobber rails), but "Err ⇒ nothing happened" is FALSE here;
    // callers must refresh the task list after a delete regardless of
    // Ok/Err. Verbatim from the design plan — do not change without
    // updating GAP-64. The Err DOES now carry `landed_paths` collected so
    // far (cloned at the failure point only), so the service layer can still
    // repair each already-relocated child's own stale `parent` link before
    // propagating the failure — see `DeleteListError`'s doc comment.
    for f in &task_files {
        // to No list; rails already never-clobber. The relocation is a
        // structural move, so the shared best-effort backfill stamps a missing
        // id on the LANDED file (warn-only on failure — the move already
        // mutated the vault). The id is discarded (GAP-64, the frontend
        // reloads regardless); the landed PATH is kept — the service layer
        // repairs this file's own stale `parent` link there (DeleteListOutcome).
        let landed = move_task_to_list(&canon_root, f, "").map_err(|message| DeleteListError {
            message,
            landed: landed_paths.clone(),
        })?;
        super::super::disk::backfill_task_id(&canon_root, &landed, id_property);
        moved += 1;
        landed_paths.push(landed);
    }
    // Remove only if empty; a folder with sub-lists / foreign files stays.
    // Only DirectoryNotEmpty IS that deliberate keep — collapsing every error
    // into folderRemoved:false swallowed real failures (PermissionDenied, a
    // Windows AV/indexer lock on the now-empty dir), leaving a phantom
    // "deleted" empty list with no error shown. Those propagate; per GAP-64
    // the Err still does NOT mean nothing happened (the tasks above already
    // moved — callers reload regardless). NotFound counts as removed: the
    // folder being gone is the outcome, whoever got there first (the
    // delete_transcription_model "path is clear" precedent).
    let folder_removed = match std::fs::remove_dir(&list_dir) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => false,
        Err(e) => {
            // Every direct task already relocated successfully by this
            // point (the loop above only reaches here after finishing
            // clean) — `landed_paths` carries the COMPLETE set, so the
            // service layer can still repair every one of them before
            // propagating this failure (`DeleteListError`'s doc comment).
            return Err(DeleteListError {
                message: format!(
                    "Moved {moved} task(s) to No list, but the list folder could not be removed: {e}"
                ),
                landed: landed_paths,
            });
        }
    };
    Ok(DeleteListOutcome {
        moved,
        folder_removed,
        landed: landed_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(name), body).unwrap();
    }

    const TASK: &str = "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\n---\n";

    #[test]
    fn move_task_between_lists_keeps_basename() {
        // Expectations are built from the CANONICAL root: on Windows,
        // canonicalize returns the \\?\ form, so a lexical root.join(...)
        // would never compare equal to the landed (canonical) path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root, "2026-07-08-t.md", TASK);
        let canon_root = std::fs::canonicalize(&root).unwrap();
        let landed = move_task_to_list(&root, &root.join("2026-07-08-t.md"), "Inbox").unwrap();
        assert_eq!(landed, canon_root.join("Inbox").join("2026-07-08-t.md"));
        assert!(landed.exists());
        assert!(!root.join("2026-07-08-t.md").exists());
        // And back to the root via "".
        let back = move_task_to_list(&root, &landed, "").unwrap();
        assert_eq!(back, canon_root.join("2026-07-08-t.md"));
    }

    #[test]
    fn move_task_same_list_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "t.md", TASK);
        let p = root.join("Inbox").join("t.md");
        let landed = move_task_to_list(&root, &p, "Inbox").unwrap();
        assert_eq!(landed, std::fs::canonicalize(&p).unwrap());
        assert!(p.exists());
    }

    #[test]
    fn move_task_collision_lands_suffixed_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root, "t.md", TASK);
        write(&root.join("Inbox"), "t.md", "occupant");
        let landed = move_task_to_list(&root, &root.join("t.md"), "Inbox").unwrap();
        assert_eq!(landed.file_name().unwrap(), "t (2).md");
        // The occupant is untouched.
        assert_eq!(
            std::fs::read_to_string(root.join("Inbox").join("t.md")).unwrap(),
            "occupant"
        );
    }

    #[test]
    fn move_task_recreates_a_deleted_list_folder() {
        // Lists are folders; moving into one that vanished resurrects it,
        // exactly like add_task recreates the tasks root.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root, "t.md", TASK);
        assert!(!root.join("Someday").exists());
        let landed = move_task_to_list(&root, &root.join("t.md"), "Someday").unwrap();
        assert!(landed.exists());
        assert!(root.join("Someday").is_dir());
    }

    #[test]
    fn move_task_rejects_source_outside_root_and_escaping_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, TASK).unwrap();
        assert!(move_task_to_list(&root, &outside, "Inbox").is_err());
        write(&root, "t.md", TASK);
        assert!(move_task_to_list(&root, &root.join("t.md"), "../x").is_err());
        assert!(move_task_to_list(&root, &root.join("t.md"), "/abs").is_err());
        // A dot-prefixed target would land the task in a walk-skipped folder.
        assert!(move_task_to_list(&root, &root.join("t.md"), ".hidden").is_err());
        assert!(move_task_to_list(&root, &root.join("t.md"), "Work/.hidden").is_err());
    }

    #[test]
    fn move_task_rejects_a_file_that_is_no_longer_a_task() {
        // A list-only editor save skips update_task, so the move must re-read
        // frontmatter and refuse a file edited (outside the app) to drop
        // `type: Task` — mirroring the status/field writers (Codex, PR #53
        // re-review). The file stays put, never moved under the tasks folder.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(
            &root,
            "note.md",
            "---\ntype: Note\ntitle: \"Not a task\"\n---\n",
        );
        assert!(move_task_to_list(&root, &root.join("note.md"), "Inbox").is_err());
        assert!(root.join("note.md").exists(), "the file must not be moved");
        assert!(!root.join("Inbox").join("note.md").exists());
        // A real task still moves.
        write(&root, "t.md", TASK);
        assert!(move_task_to_list(&root, &root.join("t.md"), "Inbox").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn move_task_rejects_symlinked_source_escaping_root() {
        // Canonicalization (not lexical starts_with) must catch a source that
        // is a symlink out of the tasks root — the move would otherwise pull
        // an outside file into the vault (and delete it at its real home).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let real = dir.path().join("elsewhere.md");
        std::fs::write(&real, TASK).unwrap();
        let link = root.join("linked.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(move_task_to_list(&root, &link, "Inbox").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn move_task_fails_and_rolls_back_when_source_cannot_be_removed() {
        // rename_noreplace's hard-link path is lenient: it leaves the source
        // behind when it can't unlink it (right for capture — a stray file is
        // re-finalized as a `(recovered)` duplicate, never lost). A task move
        // can't tolerate a surviving source: the same document would show in
        // BOTH lists on the next scan. The move must detect it, roll back the
        // linked copy, and fail (Codex, PR #53 re-review). Force the remove
        // failure by making the SOURCE folder read-only — on Unix, unlinking a
        // file needs write on its parent dir (creating the hard link only
        // needs write on the TARGET dir, which stays writable). Root bypasses
        // DAC, so probe and skip under root; CI's rust-core runs non-root and
        // exercises the assertions.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(root.join("Done")).unwrap(); // target must pre-exist
        let src = root.join("t.md");
        std::fs::write(&src, TASK).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
        // If a write into the now-read-only dir still succeeds, perms are being
        // bypassed (root) and the wall this test relies on doesn't hold — skip.
        let bypassed = std::fs::write(root.join(".probe"), b"x").is_ok();
        if bypassed {
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let res = move_task_to_list(&root, &src, "Done");
        // Restore write BEFORE asserting so tempdir cleanup always succeeds.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(res.is_err(), "a surviving source must fail the move");
        assert!(src.exists(), "the original must remain in place");
        assert!(
            !root.join("Done").join("t.md").exists(),
            "the linked copy must be rolled back — no duplicate task"
        );
    }

    #[test]
    fn delete_task_list_moves_tasks_to_root_then_removes_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "a.md", TASK);
        write(&root.join("Inbox"), "b.md", TASK);
        let out = delete_task_list(&root, "Inbox", None).unwrap();
        assert_eq!(out.moved, 2);
        assert!(out.folder_removed);
        assert!(!root.join("Inbox").exists());
        assert!(root.join("a.md").exists() && root.join("b.md").exists());
    }

    #[test]
    fn delete_task_list_keeps_a_folder_with_sublists_or_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Proj"), "t.md", TASK);
        write(&root.join("Proj/Sub"), "s.md", TASK); // nested sub-list
        std::fs::write(root.join("Proj").join("notes.txt"), "keep me").unwrap(); // foreign
        let out = delete_task_list(&root, "Proj", None).unwrap();
        assert_eq!(out.moved, 1); // only Proj's own direct task
        assert!(!out.folder_removed);
        assert!(root.join("Proj").exists()); // kept — not empty
        assert!(root.join("Proj").join("notes.txt").exists()); // foreign untouched
        assert!(root.join("Proj/Sub").join("s.md").exists()); // sub-list untouched
        assert!(root.join("t.md").exists()); // the moved task landed at the root
    }

    #[cfg(unix)]
    #[test]
    fn delete_task_list_propagates_an_unexpected_removal_failure() {
        // remove_dir failing on a now-EMPTY folder (PermissionDenied, a
        // Windows AV/indexer lock) is NOT the deliberate kept-folder outcome —
        // returning Ok{folderRemoved:false} there swallowed the error, so the
        // UI closed silently while a phantom "deleted" empty list lingered.
        // Only DirectoryNotEmpty means "kept"; anything else must surface.
        // Force EACCES by making the PARENT read-only (removing a dir needs
        // write on its parent); the list itself is empty so no moves are
        // attempted first. Root bypasses DAC — probe and skip under root
        // (the move rollback test's pattern); CI's rust-core runs non-root.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(root.join("Empty")).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
        let bypassed = std::fs::write(root.join(".probe"), b"x").is_ok();
        if bypassed {
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let res = delete_task_list(&root, "Empty", None);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(res.is_err(), "an unexpected removal failure must propagate");
        assert!(root.join("Empty").is_dir(), "the folder is still there");
    }

    #[test]
    fn delete_task_list_stamps_missing_ids_when_enabled() {
        // A delete-list relocates the list's tasks to No list — a structural
        // move, so a legacy task (no id) in an id-enabled vault must pick up its
        // stable id here too, exactly like a drag/editor move (Codex, PR #59).
        // An existing id is never overwritten.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "a.md", TASK); // legacy: no id
        write(
            &root.join("Inbox"),
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ncreated: 2026-07-08\ntask-id: keep1234\n---\n",
        );
        let out = delete_task_list(&root, "Inbox", Some("task-id")).unwrap();
        assert_eq!(out.moved, 2);
        assert!(out.folder_removed);
        // The legacy task now carries a freshly-stamped 8-char id at the root.
        let a = std::fs::read_to_string(root.join("a.md")).unwrap();
        let id = a
            .lines()
            .find_map(|l| l.strip_prefix("task-id: "))
            .expect("legacy task stamped on delete-move");
        assert_eq!(id.len(), 8);
        // The pre-existing id is untouched (never overwritten).
        assert!(std::fs::read_to_string(root.join("b.md"))
            .unwrap()
            .contains("task-id: keep1234\n"));
    }

    #[test]
    fn delete_task_list_stamps_nothing_when_id_property_is_none() {
        // IDs off (None) → a delete-move never introduces one, the same posture
        // as add_task/move with generation disabled.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "a.md", TASK);
        delete_task_list(&root, "Inbox", None).unwrap();
        assert!(!std::fs::read_to_string(root.join("a.md"))
            .unwrap()
            .contains("task-id"));
    }
}
