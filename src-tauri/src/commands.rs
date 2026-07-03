use chrono::Local;
use std::path::Path;
use vault_buddy_core::{daily_note_uri, discovery, uri};

fn find_vault(id: &str) -> Result<discovery::Vault, String> {
    discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("vault not found: {id}"))
}

#[tauri::command]
pub fn list_vaults() -> Vec<discovery::Vault> {
    discovery::discover_vaults()
}

#[tauri::command]
pub fn open_vault(id: String) -> Result<(), String> {
    let vault = find_vault(&id)?;
    // Address the vault by ID, not name — names can collide across vaults.
    uri::launch(&uri::open_vault_uri(&vault.id))
}

#[tauri::command]
pub fn open_daily_note(id: String) -> Result<(), String> {
    let vault = find_vault(&id)?;
    let today = Local::now().date_naive();
    let target = daily_note_uri(&vault.id, Path::new(&vault.path), today);
    uri::launch(&target)
}
