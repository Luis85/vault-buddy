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

impl PanelOffset {
    // All access goes through lock_ignoring_poison: if any thread ever
    // panics while holding this lock, `.lock().unwrap()` on the main thread
    // would abort the whole app (a panic across the WebView2 FFI boundary on
    // Windows). Recovering the poisoned guard degrades to the last value.
    pub fn set(&self, value: (i32, i32)) {
        *vault_buddy_core::sync_util::lock_ignoring_poison(&self.0) = value;
    }

    /// Read and zero the offset in one locked step (used by the restore path
    /// so the close handler and the quit path can't double-add).
    pub fn take(&self) -> (i32, i32) {
        std::mem::take(&mut *vault_buddy_core::sync_util::lock_ignoring_poison(
            &self.0,
        ))
    }

    pub fn get(&self) -> (i32, i32) {
        *vault_buddy_core::sync_util::lock_ignoring_poison(&self.0)
    }
}

#[tauri::command]
pub fn set_panel_offset(state: tauri::State<PanelOffset>, x: i32, y: i32) {
    state.set((x, y));
}

/// Called right before the updater installs and restarts: that path exits
/// the process without the normal close/quit hooks, so restore the
/// unshifted home position first — otherwise installing with the panel
/// open at a screen edge would persist the shifted point for next launch.
///
/// Must stay a SYNCHRONOUS command: it runs on the main thread, where
/// `save_window_state` (which takes the window-state plugin's cache lock and
/// then reads window geometry) is serialized against the plugin's own
/// main-thread Moved listener. Marking it `async` would move it to the
/// runtime thread pool and re-open the off-main cache-lock-vs-Moved deadlock
/// this codebase fixed — see `window_upkeep_tick`.
#[tauri::command]
pub fn prepare_update_install(app: tauri::AppHandle) {
    use tauri::Manager;
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};
    crate::tray::restore_home_position(&app);
    // the installer exits without window destruction, which is what the
    // window-state plugin saves on — persist explicitly, like the quit path.
    // Logged on both sides: this save silently failing is exactly how a
    // buddy loses its position across an update, and the process is dead
    // moments later — the log line is the only evidence that survives.
    if let Some(pos) = app
        .get_webview_window("main")
        .and_then(|w| w.outer_position().ok())
    {
        log::info!("update install: saving window position {},{}", pos.x, pos.y);
    }
    if let Err(e) = app.save_window_state(StateFlags::POSITION) {
        log::error!("update install: saving window state failed: {e}");
    }
    // The updater kills the process via std::process::exit — stamp clean
    // now or every update would false-positive as a crash next launch.
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
/// `show_buddy_menu`/`set_window_geometry`, which call main-thread-only Win32
/// APIs), so the button re-check happens on the input-owning thread right
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

/// Applies position and size in one native call. The frontend used to issue
/// setPosition and setSize as two IPC round-trips, and the intermediate
/// geometry got painted — the buddy visibly flashed to a corner whenever the
/// panel opened with a shifted placement. Folding them into one IPC command
/// removed that, but `set_position` + `set_size` are still two event-loop
/// iterations, and on a shifted open (buddy near the bottom/right edge) the
/// loop composited the moved-but-not-yet-resized window between them — the
/// buddy flashed to the shifted-up/left corner for a frame before the resize
/// and layout caught up. On Windows a single `SetWindowPos` moves and resizes
/// atomically (one WM_WINDOWPOSCHANGED), so only the final geometry is ever
/// painted.
#[tauri::command]
pub fn set_window_geometry(
    window: tauri::WebviewWindow,
    x: i32,
    y: i32,
    width: f64,
    height: f64,
) -> Result<(), String> {
    #[cfg(windows)]
    return set_window_geometry_atomic(&window, x, y, width, height);

    // Off-Windows the shell crate isn't shipped (no webkit2gtk build here);
    // this path only keeps the command compiling and the IPC contract testable.
    #[cfg(not(windows))]
    {
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())
    }
}

/// Moves and resizes the window in a single atomic `SetWindowPos`. Runs on the
/// main (window-owning) thread via `run_on_main_thread`: mutating window
/// geometry off-main is exactly the cross-thread window poke the metronome
/// design forbids. The channel makes the command block until the move lands,
/// preserving the old `set_position`/`set_size` semantics — the next panel
/// transition reads `outerPosition` and must observe this geometry, not the
/// pre-move one.
#[cfg(windows)]
fn set_window_geometry_atomic(
    window: &tauri::WebviewWindow,
    x: i32,
    y: i32,
    width: f64,
    height: f64,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    // SetWindowPos takes physical pixels; the frontend passes a physical
    // position but a logical size, so scale the size the way `set_size` would.
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let cx = (width * scale).round() as i32;
    let cy = (height * scale).round() as i32;
    // Capture the HWND as an integer so the closure stays Send.
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .app_handle()
        .run_on_main_thread(move || {
            // SAFETY: `hwnd` is this process's live main window. SWP_NOZORDER
            // and SWP_NOACTIVATE leave z-order and focus untouched, so this
            // only moves and resizes.
            let ok = unsafe {
                SetWindowPos(
                    hwnd as _,
                    std::ptr::null_mut(),
                    x,
                    y,
                    cx,
                    cy,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            let _ = tx.send(ok != 0);
        })
        .map_err(|e| e.to_string())?;
    match rx.recv() {
        Ok(true) => Ok(()),
        Ok(false) => Err("SetWindowPos failed".into()),
        Err(_) => Err("window closed before geometry could be applied".into()),
    }
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
