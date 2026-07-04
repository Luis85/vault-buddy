use chrono::Local;
use std::path::Path;
use std::sync::Mutex;
use vault_buddy_core::{daily_note_uri, discovery, uri};

/// Physical pixels the frontend subtracted from the window position while
/// the panel is open (so it can unfold toward free screen space). The quit
/// path adds it back before persisting the position — otherwise a quit with
/// the panel open would save the shifted point and the buddy would respawn
/// away from where the user parked it.
#[derive(Default)]
pub struct PanelOffset(pub Mutex<(i32, i32)>);

#[tauri::command]
pub fn set_panel_offset(state: tauri::State<PanelOffset>, x: i32, y: i32) {
    *state.0.lock().unwrap() = (x, y);
}

/// Native context menu for the buddy. The collapsed window is far too small
/// to host an HTML menu; the OS popup renders outside the window bounds and
/// matches the tray menu. Item events are handled in `lib.rs`.
#[tauri::command]
pub fn show_buddy_menu(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};

    let hide = MenuItem::with_id(&app, "buddy-hide", "Hide to tray", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let separator = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(&app, "buddy-quit", "Quit Vault Buddy", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&hide, &separator, &quit]).map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

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
