use std::path::Path;

use super::{assert_root_if_exists, tasks_root_for};
use crate::services::ServicePaths;
use crate::{capture_paths, tasks};

/// Read-only enumeration of a vault's list folders (empty ones included, so a
/// just-created list appears before its first task). Unknown vault / unsafe
/// or missing folder / escape → empty, never an error (mirrors list_tasks).
pub fn list_task_lists(paths: &ServicePaths, id: &str) -> Vec<String> {
    let Ok((vault_path, root, _)) = tasks_root_for(paths, id) else {
        return Vec::new();
    };
    if let Err(e) = assert_root_if_exists(&vault_path, &root) {
        log::warn!("list_task_lists: tasks folder resolves outside the vault: {e}");
        return Vec::new();
    }
    tasks::task_lists(&root)
}

/// Create a list folder in a vault's tasks root. Write-strict: the name is
/// validated (single segment, no leading dot) and containment is asserted
/// before AND after creation. Returns the created list's relative name.
pub fn create_task_list(paths: &ServicePaths, id: &str, name: &str) -> Result<String, String> {
    let (vault_path, root, _) = tasks_root_for(paths, id)?;
    if !vault_path.is_dir() {
        log::warn!(
            "create_task_list: vault folder missing: {}",
            vault_path.display()
        );
        return Err("Vault folder not found — was it moved or deleted?".to_string());
    }
    capture_paths::assert_path_inside_vault(&vault_path, &root)?;
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not create tasks folder: {e}"))?;
    capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    tasks::create_task_list(&root, name)
}

/// The result of a task move: the landed absolute path (which may carry a
/// ` (N)` collision suffix the caller must adopt) plus the task's id when the
/// vault opts in — the freshly-backfilled value or the existing one, `None`
/// when ids are off. The id lets the drag / editor-move callers reflect a
/// just-stamped id without a reload, the same reason `update_task` returns it.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedTask {
    pub path: String,
    pub id: Option<String>,
}

/// Move a task file into another list's folder (the tasks domain's file
/// move). The core layer re-validates source containment and lands on
/// `rename_noreplace` + suffix retry; this layer adds the vault-level root
/// assert every task write shares.
pub fn move_task_to_list(
    paths: &ServicePaths,
    id: &str,
    task_path: &str,
    list: &str,
) -> Result<MovedTask, String> {
    let (vault_path, root, cfg) = tasks_root_for(paths, id)?;
    assert_root_if_exists(&vault_path, &root)?;
    let landed = tasks::move_task_to_list(&root, Path::new(task_path), list)?;
    // Stamp a missing ID on the landed file when the vault opts in — a move is
    // a structural edit like a field edit / reorder (only a status toggle is
    // excluded), and `update_task` (the OTHER edit path) already stamps. This
    // runs on the LANDED path, so a still-QUEUED transcription/rename can't be
    // affected. The shared backfill is best-effort (warn-only — the move
    // already mutated the vault; audio-first discipline) and returns the
    // effective id, freshly stamped or already present, riding back in
    // MovedTask.
    let id_property =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
    let moved = MovedTask {
        path: landed.to_string_lossy().into_owned(),
        id: tasks::backfill_task_id(&root, &landed, id_property),
    };
    // A markdown-fallback `parent` link is resolved relative to the note
    // holding it, so a child that just changed depth points at nothing even
    // though its parent never moved. Recompose it here — the one file this move
    // already rewrites — best-effort/warn-only like the backfill above, and
    // only written when the link actually differs (see `repair_parent_link`).
    super::parent::repair_parent_link(&vault_path, &root, &landed, &cfg);
    Ok(moved)
}

/// Rename a list folder (see `tasks::rename_task_list`). Adds the vault-level
/// root assert every list write shares. Returns the new relative list name.
pub fn rename_task_list(
    paths: &ServicePaths,
    id: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let (vault_path, root, _) = tasks_root_for(paths, id)?;
    assert_root_if_exists(&vault_path, &root)?;
    tasks::rename_task_list(&root, from, to)
}

/// Delete a list folder (see `tasks::delete_task_list`). Returns the outcome.
/// The vault's id property is threaded down so a legacy task relocated to No
/// list is stamped like any other structural move — the same gate
/// `move_task_to_list` and `add_task` share; the core stamps best-effort.
pub fn delete_task_list(
    paths: &ServicePaths,
    id: &str,
    list: &str,
) -> Result<tasks::DeleteListOutcome, String> {
    let (vault_path, root, cfg) = tasks_root_for(paths, id)?;
    assert_root_if_exists(&vault_path, &root)?;
    let id_property =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
    // Same bounded, single-file repair `move_task_to_list` performs on its own
    // landed file — reached here too, because the core delete loop relocates
    // the list's tasks through the exact same rename rails. This is the
    // SERVICE layer (not core's lists.rs) on purpose: `repair_parent_link`
    // needs the real vault root, and `root.parent()` is wrong for a nested
    // `tasksFolder` (the trap its own doc comment names). Never the unbounded
    // "refresh every child of a moved PARENT" batch this design declines.
    //
    // Matched explicitly rather than `?`: core's delete can fail AFTER
    // already relocating some (or all) of the list's direct tasks (GAP-64) —
    // a later move, or the final folder removal, failing does not undo the
    // moves that already landed. `DeleteListError::landed` carries exactly
    // those paths, so the repair must run on the Err arm too, BEFORE the
    // failure propagates — not only on success, or an already-landed
    // child's stale fallback `parent` link (depth-relative, so a relocation
    // always changes it) would be left broken with nothing to ever fix it.
    match tasks::delete_task_list(&root, list, id_property) {
        Ok(outcome) => {
            for landed in &outcome.landed {
                super::parent::repair_parent_link(&vault_path, &root, landed, &cfg);
            }
            Ok(outcome)
        }
        Err(e) => {
            for landed in &e.landed {
                super::parent::repair_parent_link(&vault_path, &root, landed, &cfg);
            }
            Err(e.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::fixture;
    use crate::services::{add_task, list_tasks, set_task_parent};

    #[test]
    fn task_list_services_enumerate_create_and_move() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        // Nothing yet — and an unknown vault is best-effort empty.
        assert!(list_task_lists(&paths, "deadbeef01234567").is_empty());
        assert!(list_task_lists(&paths, "unknown").is_empty());
        // Create validates (write-strict) and creates the folder.
        assert!(create_task_list(&paths, "deadbeef01234567", "a/b").is_err());
        assert_eq!(
            create_task_list(&paths, "deadbeef01234567", " Inbox ").unwrap(),
            "Inbox"
        );
        assert!(vault.join("Tasks").join("Inbox").is_dir());
        assert_eq!(list_task_lists(&paths, "deadbeef01234567"), vec!["Inbox"]);
        // Move returns the landed absolute path and the list derives from it.
        let created = add_task(
            &paths,
            "deadbeef01234567",
            "Buy milk",
            "2026-07-09",
            None,
            None,
            &[],
            Some(""),
            None,
            None,
        )
        .unwrap()
        .task;
        let moved = move_task_to_list(&paths, "deadbeef01234567", &created.path, "Inbox").unwrap();
        assert!(std::path::Path::new(&moved.path).exists());
        let listed = list_tasks(&paths, "deadbeef01234567");
        assert_eq!(listed[0].list, "Inbox");
        assert!(move_task_to_list(&paths, "unknown", &moved.path, "Inbox").is_err());
    }

    #[test]
    fn move_task_to_list_stamps_a_missing_id_when_enabled() {
        // A task created while IDs were off carries none; enabling IDs and then
        // MOVING it must backfill one (a move is a structural edit, like a field
        // edit — only a status toggle is excluded), so a legacy task picks up a
        // stable ID the first time it is reorganized.
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        let created = add_task(
            &paths,
            "deadbeef01234567",
            "Buy milk",
            "2026-07-09",
            None,
            None,
            &[],
            Some(""),
            None,
            None,
        )
        .unwrap()
        .task;
        assert!(created.id.is_none());
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "uid" } } }"#,
        )
        .unwrap();
        let moved = move_task_to_list(&paths, "deadbeef01234567", &created.path, "Inbox").unwrap();
        // The move RETURNS the freshly-stamped id (so the UI can reflect it)...
        let id = moved.id.clone().expect("id stamped on move");
        assert_eq!(id.len(), 8);
        // ...and it's the id that actually landed on disk and that list_tasks reads.
        assert!(std::fs::read_to_string(&moved.path)
            .unwrap()
            .contains(&format!("uid: {id}\n")));
        assert_eq!(
            list_tasks(&paths, "deadbeef01234567")[0].id.as_deref(),
            Some(id.as_str())
        );
    }

    #[test]
    fn move_task_to_list_writes_no_id_when_disabled() {
        // IDs off (default config): a move never introduces one.
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        let created = add_task(
            &paths,
            "deadbeef01234567",
            "Buy milk",
            "2026-07-09",
            None,
            None,
            &[],
            Some(""),
            None,
            None,
        )
        .unwrap()
        .task;
        let moved = move_task_to_list(&paths, "deadbeef01234567", &created.path, "Inbox").unwrap();
        assert!(moved.id.is_none());
        assert!(!std::fs::read_to_string(&moved.path)
            .unwrap()
            .contains("task-id"));
    }

    #[test]
    fn rename_and_delete_lists_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        add_task(
            &paths,
            "deadbeef01234567",
            "A",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Inbox"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            rename_task_list(&paths, "deadbeef01234567", "Inbox", "Later").unwrap(),
            "Later"
        );
        assert!(vault.join("Tasks").join("Later").is_dir());
        let out = delete_task_list(&paths, "deadbeef01234567", "Later").unwrap();
        assert_eq!(out.moved, 1);
        assert!(out.folder_removed);
        assert!(list_tasks(&paths, "deadbeef01234567")
            .iter()
            .all(|t| t.list.is_empty()));
        assert!(rename_task_list(&paths, "unknown", "a", "b").is_err());
    }

    #[test]
    fn delete_task_list_stamps_moved_tasks_when_ids_enabled() {
        // Deleting a list relocates its tasks to No list — a structural move, so
        // a legacy task (created while ids were off) must be backfilled with an
        // id, exactly like a drag/editor move. The frontend reloads after a
        // delete, so list_tasks is what surfaces the fresh id (Codex, PR #59).
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        let created = add_task(
            &paths,
            "deadbeef01234567",
            "A",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Inbox"),
            None,
            None,
        )
        .unwrap()
        .task;
        assert!(created.id.is_none()); // ids were off at create
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "uid" } } }"#,
        )
        .unwrap();
        let out = delete_task_list(&paths, "deadbeef01234567", "Inbox").unwrap();
        assert_eq!(out.moved, 1);
        let listed = list_tasks(&paths, "deadbeef01234567");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].list.is_empty()); // moved to No list
        assert!(
            listed[0].id.as_ref().is_some_and(|s| s.len() == 8),
            "the relocated legacy task must be stamped, got {:?}",
            listed[0].id
        );
    }

    #[test]
    fn delete_task_list_repairs_a_relocated_childs_fallback_link() {
        // move_task_to_list already repairs the ONE file it lands (Codex P2,
        // PR #77, design spec §7): a markdown-fallback `parent` link is
        // resolved relative to the note's OWN directory, so a child that
        // changes depth points at nothing even though its parent never moved.
        // delete_task_list relocates a list's tasks to No list through the
        // exact same rails and used to skip the repair entirely — reached by
        // a different write path than move.
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true } } }"#,
        )
        .unwrap();
        // `Project#1` forces the markdown-fallback form — `#` has no
        // wikilink escape, so the composer must percent-encode it.
        let parent = add_task(
            &paths,
            "deadbeef01234567",
            "P",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Project#1"),
            None,
            None,
        )
        .unwrap()
        .task;
        let child = add_task(
            &paths,
            "deadbeef01234567",
            "C",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Deep/Sub"),
            None,
            None,
        )
        .unwrap()
        .task;
        set_task_parent(
            &paths,
            "deadbeef01234567",
            Path::new(&child.path),
            Some(Path::new(&parent.path)),
        )
        .unwrap();
        let before = std::fs::read_to_string(&child.path).unwrap();
        assert!(
            before.contains("](../../../Tasks/Project%231/"),
            "the pre-delete link is three levels deep, got {before}"
        );
        let out = delete_task_list(&paths, "deadbeef01234567", "Deep/Sub").unwrap();
        assert_eq!(out.moved, 1);
        let landed = list_tasks(&paths, "deadbeef01234567")
            .into_iter()
            .find(|t| t.title == "C")
            .expect("the relocated child is still listed");
        assert_eq!(landed.list, "", "the child landed in No list");
        let after = std::fs::read_to_string(&landed.path).unwrap();
        // One `../` now: the child sits at <vault>/Tasks/<file>.md, and a
        // markdown destination resolves from the note's OWN directory.
        assert!(
            after.contains("](../Tasks/Project%231/"),
            "the repaired link must recompose to one level, got {after}"
        );
    }

    #[test]
    fn delete_task_list_repairs_a_relocated_childs_link_under_a_nested_tasks_folder() {
        // tasks root = <vault>/Notes/Tasks, so vault_root != tasks_root.parent()
        // — the trap parent::repair_parent_link's doc comment names by name.
        // The SERVICE layer must thread the real vault_path it already
        // resolved via tasks_root_for, never derive one inside core's
        // lists.rs.
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "tasksFolder": "Notes/Tasks" } } }"#,
        )
        .unwrap();
        let parent = add_task(
            &paths,
            "deadbeef01234567",
            "P",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Project#1"),
            None,
            None,
        )
        .unwrap()
        .task;
        let child = add_task(
            &paths,
            "deadbeef01234567",
            "C",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Deep"),
            None,
            None,
        )
        .unwrap()
        .task;
        set_task_parent(
            &paths,
            "deadbeef01234567",
            Path::new(&child.path),
            Some(Path::new(&parent.path)),
        )
        .unwrap();
        delete_task_list(&paths, "deadbeef01234567", "Deep").unwrap();
        let landed = list_tasks(&paths, "deadbeef01234567")
            .into_iter()
            .find(|t| t.title == "C")
            .expect("the relocated child is still listed");
        let after = std::fs::read_to_string(&landed.path).unwrap();
        // Vault-relative, `Notes/` included: root.parent() would have
        // dropped it and emitted `../Tasks/Project%231/...` — one segment
        // short and pointing nowhere real.
        assert!(
            after.contains("](../../Notes/Tasks/Project%231/"),
            "link must be vault-relative, got {after}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_task_list_repairs_a_landed_child_despite_a_propagated_removal_failure() {
        // GAP-64's OTHER partial-failure arm (the sibling test above pins the
        // move-loop's own `?`): core's delete can relocate EVERY direct task
        // successfully and still return Err — the final `remove_dir` on the
        // now-empty list folder can fail on its own (permissions, a Windows
        // AV/indexer lock). Before this fix, the service layer's `?` on
        // core's call skipped the repair loop entirely on ANY Err, so an
        // already-landed child kept a fallback `parent` link computed for
        // its OLD, deeper location even though the move itself succeeded.
        //
        // Nest the list ("Team/Inbox") so the list's OWN parent ("Team") can
        // be made unwritable WITHOUT blocking the moves themselves:
        // relocating a task needs write on Team/Inbox (to unlink it) and on
        // the tasks root (to hard-link it in) — neither is an operation on
        // Team's own directory entries. Only removing the "Inbox" ENTRY
        // from "Team" touches Team's own entries. A plain chmod can't
        // express "writable one level down, not here" AND survive root
        // (DAC is bypassed for root, which is what runs this suite here —
        // see the chmod-based tests elsewhere in this crate); the ext4
        // immutable flag (`chattr +i`) is enforced independently of DAC —
        // confirmed empirically for this task (see the task report) — so it
        // blocks root's own rmdir too, without a setpriv dance. Still
        // probed and skipped if unsupported (a non-ext4 temp filesystem, or
        // `chattr` unavailable), mirroring this crate's chmod-based
        // probe-and-skip tests.
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true } } }"#,
        )
        .unwrap();
        let root = tasks_root_for(&paths, "deadbeef01234567").unwrap().1;
        // `Project#1` forces the markdown-fallback link — `#` has no
        // wikilink escape, so the composer must percent-encode it (the same
        // forcing device the sibling repair tests above use).
        let parent = add_task(
            &paths,
            "deadbeef01234567",
            "P",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Project#1"),
            None,
            None,
        )
        .unwrap()
        .task;
        let child = add_task(
            &paths,
            "deadbeef01234567",
            "C",
            "2026-07-09",
            None,
            None,
            &[],
            Some("Team/Inbox"),
            None,
            None,
        )
        .unwrap()
        .task;
        set_task_parent(
            &paths,
            "deadbeef01234567",
            Path::new(&child.path),
            Some(Path::new(&parent.path)),
        )
        .unwrap();
        let before = std::fs::read_to_string(&child.path).unwrap();
        assert!(
            before.contains("](../../../Tasks/Project%231/"),
            "the pre-delete link is three levels deep, got {before}"
        );

        let team = root.join("Team");
        let set_immutable = Command::new("chattr").arg("+i").arg(&team).status();
        let chattr_ok = matches!(&set_immutable, Ok(s) if s.success());
        // Probe: creating a new entry DIRECTLY under Team must now be
        // blocked too (the same directory-entry-write the final rmdir
        // needs) — if it isn't, the flag had no effect here and this test
        // cannot exercise anything.
        let bypassed = !chattr_ok || std::fs::write(team.join(".probe"), b"x").is_ok();
        if bypassed {
            let _ = Command::new("chattr").arg("-i").arg(&team).status();
            let _ = std::fs::remove_file(team.join(".probe"));
            eprintln!(
                "SKIPPED delete_task_list_repairs_a_landed_child_despite_a_propagated_removal_failure: \
                 chattr +i had no effect here (unsupported filesystem or missing chattr)"
            );
            return;
        }

        let err = delete_task_list(&paths, "deadbeef01234567", "Team/Inbox").unwrap_err();
        // Restore BEFORE asserting so tempdir cleanup always succeeds either way.
        let _ = Command::new("chattr").arg("-i").arg(&team).status();
        assert!(
            err.contains("could not be removed"),
            "the removal failure must still propagate, got {err}"
        );

        // The move already landed (No list) despite the propagated error...
        let landed = list_tasks(&paths, "deadbeef01234567")
            .into_iter()
            .find(|t| t.title == "C")
            .expect("the relocated child is still listed");
        assert_eq!(landed.list, "", "the child landed in No list");
        // ...and its stale fallback link was recomposed for its NEW
        // (shallower) location, not left pointing three levels deep at a
        // folder that no longer contains it.
        let after = std::fs::read_to_string(&landed.path).unwrap();
        assert!(
            after.contains("](../Tasks/Project%231/"),
            "the repaired link must recompose to the child's new depth despite \
             the propagated error, got {after}"
        );
    }
}
