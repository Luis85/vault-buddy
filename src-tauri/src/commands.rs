use chrono::Local;
use std::path::Path;
use vault_buddy_core::{daily_note_uri, discovery, process, uri};

/// Called right before the updater installs and restarts: that path exits
/// the process without the normal close/quit hooks, so make sure the panel
/// is closed and the buddy position is persisted first.
///
/// Must stay a SYNCHRONOUS command: it runs on the main thread, where
/// `save_window_state` (which takes the window-state plugin's cache lock and
/// then reads window geometry) is serialized against the plugin's own
/// main-thread Moved listener. Marking it `async` would move it to the
/// runtime thread pool and re-open the off-main cache-lock-vs-Moved deadlock
/// this codebase fixed — see `window_upkeep_tick`.
#[tauri::command]
pub fn prepare_update_install(app: tauri::AppHandle) {
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};
    // The buddy window never shifts, so there is no home position to restore —
    // just make sure the panel is closed and persist the buddy position.
    close_panel(app.clone());
    if let Err(e) = app.save_window_state(StateFlags::POSITION) {
        log::error!("update install: saving window state failed: {e}");
    }
    log::info!("clean shutdown (update install)");
    crate::diagnostics::mark_clean_shutdown();
}

/// Enters the OS window-move loop for the buddy. A Rust-side chokepoint
/// instead of the raw `startDragging()` JS API because a drag request can
/// go stale in transit: a fast flick releases the button while the IPC is
/// still in flight, and the runtime would post the synthetic
/// WM_NCLBUTTONDOWN anyway — Windows then runs a "sticky" move loop with
/// no button held, gluing the buddy to the cursor and eating the next real
/// press. Being a synchronous command it runs on the main thread (like
/// `show_buddy_menu`, which calls main-thread-only Win32 APIs), so the
/// button re-check happens on the input-owning thread right
/// before the move loop is entered. Returns whether the drag actually
/// started so the frontend can retract its blur suppression when a stale
/// request is dropped. `pointer_type` is the webview's pointer kind: the
/// button re-check is mouse-only, because a touch/pen contact reports
/// buttons=1 to the webview yet need not surface as WM_LBUTTONDOWN, so a
/// GetKeyState re-check would wrongly drop every touch/pen drag.
#[tauri::command]
pub fn start_buddy_drag(
    window: tauri::WebviewWindow,
    pointer_type: String,
) -> Result<bool, String> {
    #[cfg(windows)]
    if pointer_type == "mouse" && !primary_button_down() {
        log::info!("buddy drag dropped: primary button already released");
        return Ok(false);
    }
    #[cfg(not(windows))]
    let _ = &pointer_type;
    window.start_dragging().map_err(|e| e.to_string())?;
    Ok(true)
}

/// True while the primary (swap-aware "logical") mouse button is physically
/// held, as of the last message this thread processed. Used to drop stale
/// drag requests and to keep the upkeep tick away from a window the user is
/// mid-drag. Must be called on the main (input-owning) thread — `GetKeyState`
/// reflects that thread's input queue. Windows-only: both callers guard the
/// call with `cfg(windows)`, so no cross-platform stub is needed.
#[cfg(windows)]
pub(crate) fn primary_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_LBUTTON};
    // High-order bit set => down. GetKeyState follows SwapMouseButton, so
    // VK_LBUTTON tracks whichever physical button is the primary — matching
    // the webview's own notion of "button 0".
    (unsafe { GetKeyState(VK_LBUTTON as i32) } as u16 & 0x8000) != 0
}

/// Show/hide the panel window. A sync command, so it runs on the main thread
/// (where window show/hide and the placement getters are valid). Positioned
/// while still hidden, then shown — the panel window never resizes and is
/// only moved, so there is no WebView2 stale-frame flash. Opening hides the
/// greeting bubble.
#[tauri::command]
pub fn toggle_panel(app: tauri::AppHandle) {
    use tauri::Manager;
    let Some(panel) = app.get_webview_window("panel") else {
        log::warn!("toggle_panel: no panel window");
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    position_panel(&app);
    if let Err(e) = panel.show() {
        log::warn!("toggle_panel: show failed: {e}");
    }
    let _ = panel.set_focus();
    if let Some(bubble) = app.get_webview_window("bubble") {
        let _ = bubble.hide();
    }
}

/// Hide the panel window. Idempotent; called by Escape, drag start, a launched
/// vault action, and the updater.
#[tauri::command]
pub fn close_panel(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
}

/// Hide the greeting bubble window. Idempotent; called by the bubble's own
/// auto-dismiss timer (Task 10) — `toggle_panel` also hides it when the panel
/// opens.
#[tauri::command]
pub fn close_bubble(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(bubble) = app.get_webview_window("bubble") {
        let _ = bubble.hide();
    }
}

/// Move the (hidden) panel window beside the buddy, respecting screen edges.
/// Best-effort: any missing window/monitor info leaves the panel where it was.
pub(crate) fn position_panel(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{panel_position, Rect};
    let (Some(buddy), Some(panel)) = (
        app.get_webview_window("main"),
        app.get_webview_window("panel"),
    ) else {
        return;
    };
    let (Ok(bpos), Ok(bsize), Ok(psize)) = (
        buddy.outer_position(),
        buddy.outer_size(),
        panel.outer_size(),
    ) else {
        return;
    };
    let buddy_rect = Rect {
        x: bpos.x,
        y: bpos.y,
        w: bsize.width as i32,
        h: bsize.height as i32,
    };
    let work = buddy.current_monitor().ok().flatten().map(|m| {
        // The taskbar-excluding work area, NOT full monitor bounds: a panel
        // clamped to full bounds can draw behind the taskbar for a buddy parked
        // lower-middle (only a bottom-edge buddy bottom-aligns clear of it).
        let wa = m.work_area();
        Rect {
            x: wa.position.x,
            y: wa.position.y,
            w: wa.size.width as i32,
            h: wa.size.height as i32,
        }
    });
    let point = panel_position(buddy_rect, work, psize.width as i32, psize.height as i32);
    if let Err(e) = panel.set_position(tauri::PhysicalPosition::new(point.x, point.y)) {
        log::warn!("position_panel: set_position failed: {e}");
    }
}

/// Position + show the greeting bubble beside the buddy on launch. Best-effort.
pub(crate) fn show_bubble(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{panel_position, Rect};
    let (Some(buddy), Some(bubble)) = (
        app.get_webview_window("main"),
        app.get_webview_window("bubble"),
    ) else {
        return;
    };
    let (Ok(bpos), Ok(bsize), Ok(size)) = (
        buddy.outer_position(),
        buddy.outer_size(),
        bubble.outer_size(),
    ) else {
        return;
    };
    let buddy_rect = Rect {
        x: bpos.x,
        y: bpos.y,
        w: bsize.width as i32,
        h: bsize.height as i32,
    };
    let work = buddy.current_monitor().ok().flatten().map(|m| {
        // The taskbar-excluding work area, NOT full monitor bounds: a panel
        // clamped to full bounds can draw behind the taskbar for a buddy parked
        // lower-middle (only a bottom-edge buddy bottom-aligns clear of it).
        let wa = m.work_area();
        Rect {
            x: wa.position.x,
            y: wa.position.y,
            w: wa.size.width as i32,
            h: wa.size.height as i32,
        }
    });
    let point = panel_position(buddy_rect, work, size.width as i32, size.height as i32);
    let _ = bubble.set_position(tauri::PhysicalPosition::new(point.x, point.y));
    let _ = bubble.show();
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
    // Hide stays enabled even while recording: hide_buddy guards it
    // downstream and silently no-ops mid-capture (the buddy is the
    // recording indicator and must stay visible).
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

/// Reveal the app log folder (holding `vault-buddy.log` and `crash.log`) in
/// the OS file manager, so a user can attach logs after a crash.
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) {
    crate::diagnostics::open_log_dir(&app);
}

/// The frontend calls this when an update install fails after
/// prepare_update_install stamped a clean shutdown — the app keeps
/// running, so crash detection must come back on.
#[tauri::command]
pub fn rearm_crash_detection() {
    log::warn!("update install failed after shutdown prep — re-arming crash detection");
    crate::diagnostics::rearm_running_marker();
}
