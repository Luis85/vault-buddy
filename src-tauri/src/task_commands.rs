use std::path::Path;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery};

use crate::capture_commands::ConfigWriteLock;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksConfigDto {
    pub tasks_folder: Option<String>,
}

/// The vault's configured tasks folder (or None → the frontend shows the
/// default "Tasks"). Unknown vaults return None — never an error.
#[tauri::command]
pub fn get_tasks_config(id: String) -> TasksConfigDto {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    TasksConfigDto {
        tasks_folder: cfg.tasks_folder,
    }
}

/// Persist the vault's tasks folder. Validates the folder stays inside the
/// vault BEFORE writing (an invalid folder is an inline error, nothing is
/// saved), serialized behind ConfigWriteLock so a concurrent per-vault write
/// isn't lost. Read-modify-write preserves the vault's other config.
#[tauri::command]
pub fn set_tasks_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    tasks_folder: Option<String>,
) -> Result<(), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let folder = tasks_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    if let Some(folder) = &folder {
        capture_paths::safe_recording_root(Path::new(&vault.path), folder)?;
    }
    let _guard = lock_ignoring_poison(&lock.0);
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.tasks_folder = folder;
    capture_config::update_vault_config(&id, value)
}
