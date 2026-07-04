use chrono::Local;
use std::path::Path;
use std::sync::Mutex;
use vault_buddy_core::{daily_note_uri, discovery, process, uri};

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

/// Called right before the updater installs and restarts: that path exits
/// the process without the normal close/quit hooks, so restore the
/// unshifted home position first — otherwise installing with the panel
/// open at a screen edge would persist the shifted point for next launch.
#[tauri::command]
pub fn prepare_update_install(app: tauri::AppHandle) {
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};
    crate::tray::restore_home_position(&app);
    // the installer exits without window destruction, which is what the
    // window-state plugin saves on — persist explicitly, like the quit path
    let _ = app.save_window_state(StateFlags::POSITION);
}

/// Applies position and size in one native call. The frontend used to issue
/// setPosition and setSize as two IPC round-trips, and the intermediate
/// geometry got painted — the buddy visibly flashed to a corner whenever the
/// panel opened with a shifted placement.
#[tauri::command]
pub fn set_window_geometry(
    window: tauri::WebviewWindow,
    x: i32,
    y: i32,
    width: f64,
    height: f64,
) -> Result<(), String> {
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())
}

/// Native context menu for the buddy. The collapsed window is far too small
/// to host an HTML menu; the OS popup renders outside the window bounds and
/// matches the tray menu. Item events are handled in `lib.rs`. `animated`
/// and `dragging` reflect the frontend's current settings and drive the
/// checkmarks.
#[tauri::command]
pub fn show_buddy_menu(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    animated: bool,
    dragging: bool,
) -> Result<(), String> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};

    let animation = CheckMenuItem::with_id(
        &app,
        "buddy-animation",
        "Animation",
        true,
        animated,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let dragging = CheckMenuItem::with_id(
        &app,
        "buddy-dragging",
        "Dragging",
        true,
        dragging,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let separator = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
    let hide = MenuItem::with_id(&app, "buddy-hide", "Hide to tray", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(&app, "buddy-quit", "Quit Vault Buddy", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&animation, &dragging, &separator, &hide, &quit])
        .map_err(|e| e.to_string())?;
    // Win32 popup menus require the owning window to be foreground —
    // without this the menu is delayed or silently ignored until the user
    // left-clicks the (unfocused) buddy first.
    let _ = window.set_focus();
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
    let mut vaults = discovery::discover_vaults();
    // obsidian.json keeps `open: true` across a full Obsidian quit (that's
    // how Obsidian restores vaults on relaunch) — only trust the flags
    // while an Obsidian process actually exists, or the "Open now" group
    // shows vaults that were merely open last session.
    if !process::obsidian_running() {
        for vault in &mut vaults {
            vault.open = false;
        }
    }
    vaults
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
