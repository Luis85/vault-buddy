use std::path::Path;

use super::tasks_root_for;
use crate::services::{app_config, ServicePaths};
use crate::{capture_config, capture_paths, tasks};

/// Read-only enumeration of a vault's list folders (empty ones included, so a
/// just-created list appears before its first task). Unknown vault / unsafe
/// or missing folder / escape → empty, never an error (mirrors list_tasks).
pub fn list_task_lists(paths: &ServicePaths, id: &str) -> Vec<String> {
    let Ok((vault_path, root)) = tasks_root_for(paths, id) else {
        return Vec::new();
    };
    if root.exists() {
        if let Err(e) = capture_paths::assert_root_inside_vault(&vault_path, &root) {
            log::warn!("list_task_lists: tasks folder resolves outside the vault: {e}");
            return Vec::new();
        }
    }
    tasks::task_lists(&root)
}

/// Create a list folder in a vault's tasks root. Write-strict: the name is
/// validated (single segment, no leading dot) and containment is asserted
/// before AND after creation. Returns the created list's relative name.
pub fn create_task_list(paths: &ServicePaths, id: &str, name: &str) -> Result<String, String> {
    let (vault_path, root) = tasks_root_for(paths, id)?;
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

/// Move a task file into another list's folder (the tasks domain's file
/// move). The core layer re-validates source containment and lands on
/// `rename_noreplace` + suffix retry; this layer adds the vault-level root
/// assert every task write shares. Returns the landed absolute path (which
/// may carry a collision suffix).
pub fn move_task_to_list(
    paths: &ServicePaths,
    id: &str,
    task_path: &str,
    list: &str,
) -> Result<String, String> {
    let (vault_path, root) = tasks_root_for(paths, id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    let landed = tasks::move_task_to_list(&root, Path::new(task_path), list)?;
    // Stamp a missing ID on the landed file when the vault opts in — a move is
    // a structural edit like a field edit / reorder (only a status toggle is
    // excluded), and `update_task` (the OTHER edit path) already stamps. This
    // runs on the LANDED path, so a still-QUEUED transcription/rename can't be
    // affected. Best-effort: the move already mutated the vault, so a stamp
    // failure degrades to a warning rather than failing the move and reverting
    // the UI (audio-first discipline, borrowed from the capture domain).
    let cfg = capture_config::vault_config(&app_config(paths), id);
    if let Some(property) =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name())
    {
        let generated = tasks::new_task_id();
        if let Err(e) = tasks::update_task_fields(&root, &landed, &[], &[(property, &generated)]) {
            log::warn!("move_task_to_list: could not stamp task id: {e}");
        }
    }
    Ok(landed.to_string_lossy().into_owned())
}

/// Rename a list folder (see `tasks::rename_task_list`). Adds the vault-level
/// root assert every list write shares. Returns the new relative list name.
pub fn rename_task_list(
    paths: &ServicePaths,
    id: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let (vault_path, root) = tasks_root_for(paths, id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    tasks::rename_task_list(&root, from, to)
}

/// Delete a list folder (see `tasks::delete_task_list`). Returns the outcome.
pub fn delete_task_list(
    paths: &ServicePaths,
    id: &str,
    list: &str,
) -> Result<tasks::DeleteListOutcome, String> {
    let (vault_path, root) = tasks_root_for(paths, id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    tasks::delete_task_list(&root, list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::fixture;
    use crate::services::{add_task, list_tasks};

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
        )
        .unwrap();
        let landed = move_task_to_list(&paths, "deadbeef01234567", &created.path, "Inbox").unwrap();
        assert!(std::path::Path::new(&landed).exists());
        let listed = list_tasks(&paths, "deadbeef01234567");
        assert_eq!(listed[0].list, "Inbox");
        assert!(move_task_to_list(&paths, "unknown", &landed, "Inbox").is_err());
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
        )
        .unwrap();
        assert!(created.id.is_none());
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            r#"{ "vaults": { "deadbeef01234567": { "taskIdEnabled": true, "taskIdProperty": "uid" } } }"#,
        )
        .unwrap();
        let landed = move_task_to_list(&paths, "deadbeef01234567", &created.path, "Inbox").unwrap();
        let body = std::fs::read_to_string(&landed).unwrap();
        let id = body
            .lines()
            .find(|l| l.starts_with("uid: "))
            .expect("id stamped on move")
            .trim_start_matches("uid: ");
        assert_eq!(id.len(), 8);
        // list_tasks now surfaces the backfilled id for the moved task.
        assert_eq!(
            list_tasks(&paths, "deadbeef01234567")[0].id.as_deref(),
            Some(id)
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
        )
        .unwrap();
        let landed = move_task_to_list(&paths, "deadbeef01234567", &created.path, "Inbox").unwrap();
        assert!(!std::fs::read_to_string(&landed)
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
}
