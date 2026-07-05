use chrono::Local;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use vault_buddy_core::{daily_note_uri, discovery, process, uri};

/// The buddy's current view direction, mirrored from the frontend `settings`
/// store via `set_buddy_facing`. Read on the main thread when placing the
/// greeting bubble so it opens on the side the buddy faces. A plain atomic (no
/// lock): a stale read at worst opens the bubble on the wrong side for one
/// placement, which the next reposition corrects — so it can never interfere
/// with the drag-path window work the way a lock could.
static BUDDY_FACES_RIGHT: AtomicBool = AtomicBool::new(true);

fn buddy_facing() -> vault_buddy_core::companion_placement::Side {
    use vault_buddy_core::companion_placement::Side;
    if BUDDY_FACES_RIGHT.load(Ordering::Relaxed) {
        Side::Right
    } else {
        Side::Left
    }
}

/// Mirror the buddy's view direction from the frontend so the greeting bubble
/// opens on the side the buddy faces. Called by the buddy window on mount and
/// whenever the facing setting changes. Anything other than "left" is treated
/// as right (the sprite's drawn default).
#[tauri::command]
pub fn set_buddy_facing(facing: String) {
    BUDDY_FACES_RIGHT.store(facing != "left", Ordering::Relaxed);
}

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
    use tauri::{Emitter, Manager};
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
    // Tell the panel webview it was just revealed: it re-runs vault discovery
    // and picks its view here (see PanelRoot). A precise "opened" signal —
    // unlike window focus, which also fires on a mere refocus and would re-run
    // discovery on every alt-tab and reset the view mid-use.
    let _ = app.emit("panel-shown", ());
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

/// Pull the greeting bubble toward the buddy by this fraction of the buddy
/// window's width, overlapping the window's transparent padding so the bubble
/// sits snug against the character instead of floating a full buddy-width away.
/// A fraction rather than a fixed px so it scales with display DPI. Cosmetic —
/// a manual Windows check is the place to tune it. The panel uses 0.0.
const BUBBLE_TUCK_FRAC: f64 = 0.20;

/// Top-left AND the resolved anchor for a companion window (panel or bubble)
/// placed beside the buddy on the `prefer` side with vertical mode `vmode`.
/// `tuck_frac` pulls the window toward the buddy by that fraction of the buddy
/// width (0.0 = flush beside). `None` when the buddy or target geometry isn't
/// available yet — callers then leave the window where it was (best-effort).
fn place_beside_buddy(
    app: &tauri::AppHandle,
    target: &tauri::WebviewWindow,
    prefer: vault_buddy_core::companion_placement::Side,
    vmode: vault_buddy_core::companion_placement::VMode,
    tuck_frac: f64,
) -> Option<(
    tauri::PhysicalPosition<i32>,
    vault_buddy_core::companion_placement::Anchor,
)> {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{place_beside, Rect, Side};
    let buddy = app.get_webview_window("main")?;
    let bpos = buddy.outer_position().ok()?;
    let bsize = buddy.outer_size().ok()?;
    let tsize = target.outer_size().ok()?;
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
    let (point, anchor) = place_beside(
        buddy_rect,
        work,
        tsize.width as i32,
        tsize.height as i32,
        prefer,
        vmode,
    );
    // Tuck toward the buddy along the side the window actually landed on, so the
    // tail nearly touches the character. Scaled by the buddy width so it tracks
    // display DPI.
    let overlap = (bsize.width as f64 * tuck_frac) as i32;
    let x = match anchor.side {
        Side::Right => point.x - overlap,
        Side::Left => point.x + overlap,
    };
    // Diagnostic for the "bubble misplaced on startup but correct after a move"
    // report: logs every bubble placement (initial show, startup re-pins, and
    // drag) so the buddy vs bubble scale factors and sizes can be compared. A
    // buddy scale != bubble scale (or a bubble size that doesn't match the
    // buddy's monitor) points at a cross-monitor / not-yet-realized DPI read.
    if target.label() == "bubble" {
        log::info!(
            "bubble place: buddy pos=({},{}) size={}x{} scale={:.2}; bubble size={}x{} scale={:.2} vis={:?}; -> pos=({},{}) {:?}",
            bpos.x,
            bpos.y,
            bsize.width,
            bsize.height,
            buddy.scale_factor().unwrap_or(0.0),
            tsize.width,
            tsize.height,
            target.scale_factor().unwrap_or(0.0),
            target.is_visible().ok(),
            x,
            point.y,
            anchor
        );
    }
    Some((tauri::PhysicalPosition::new(x, point.y), anchor))
}

/// Tell the bubble window which side/valign it landed on so it can draw its
/// tail pointing back at the buddy. Emitted on every (re)placement, so the tail
/// flips live when a drag carries the buddy across the screen midline or to an
/// edge. Emitted app-wide (only the bubble window listens) — the payload keys
/// match the SpeechBubble `side`/`valign` props.
fn emit_bubble_anchor(
    app: &tauri::AppHandle,
    anchor: vault_buddy_core::companion_placement::Anchor,
) {
    use tauri::Emitter;
    use vault_buddy_core::companion_placement::{Side, VAlign};
    let payload = serde_json::json!({
        "side": match anchor.side {
            Side::Left => "left",
            Side::Right => "right",
        },
        "valign": match anchor.valign {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        },
    });
    let _ = app.emit("bubble-anchor", payload);
}

/// Move the (hidden) panel window beside the buddy, respecting screen edges.
/// Best-effort: any missing window/monitor info leaves the panel where it was.
/// The panel prefers the right side, edge-aligns vertically, and ignores the
/// anchor.
pub(crate) fn position_panel(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{Side, VMode};
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    if let Some((pos, _anchor)) = place_beside_buddy(app, &panel, Side::Right, VMode::Edge, 0.0) {
        if let Err(e) = panel.set_position(pos) {
            log::warn!("position_panel: set_position failed: {e}");
        }
    }
}

/// Position + show the greeting bubble beside the buddy on launch. Opens on the
/// side the buddy faces and emits the resolved anchor so the tail points back
/// at the buddy. Best-effort; shown only once positioned — a moved-only window
/// has no stale-frame flash.
pub(crate) fn show_bubble(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(bubble) = app.get_webview_window("bubble") else {
        return;
    };
    let Some((pos, anchor)) = place_beside_buddy(
        app,
        &bubble,
        buddy_facing(),
        vault_buddy_core::companion_placement::VMode::Center,
        BUBBLE_TUCK_FRAC,
    ) else {
        return;
    };
    let _ = bubble.set_position(pos);
    emit_bubble_anchor(app, anchor);
    let _ = bubble.show();
}

/// Keep the greeting bubble beside the buddy as the buddy moves — called from
/// the buddy window's `Moved` handler so the bubble follows a drag instead of
/// stranding at its launch spot. A no-op unless the bubble is currently
/// visible, so a normal drag (no greeting up) does essentially no work.
///
/// Runs on the MAIN thread (window events dispatch there) and only reads
/// window geometry + calls `set_position` — it never takes the window-state
/// plugin's cache lock. That is the crucial difference from the off-main
/// `save_window_state` that caused the original drag deadlock: this reposition
/// cannot wedge against the plugin's main-thread `Moved` listener, because it
/// touches no shared lock at all. The bubble's own resulting `Moved` does not
/// recurse — the caller gates on the buddy's window label.
pub(crate) fn reposition_bubble_if_visible(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(bubble) = app.get_webview_window("bubble") else {
        return;
    };
    if !bubble.is_visible().unwrap_or(false) {
        return;
    }
    if let Some((pos, anchor)) = place_beside_buddy(
        app,
        &bubble,
        buddy_facing(),
        vault_buddy_core::companion_placement::VMode::Center,
        BUBBLE_TUCK_FRAC,
    ) {
        let _ = bubble.set_position(pos);
        // Re-emit the anchor: a drag can carry the buddy across the midline or
        // to an edge, flipping which side the bubble sits on — the tail must
        // follow.
        emit_bubble_anchor(app, anchor);
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
