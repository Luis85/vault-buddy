use chrono::Local;
use std::sync::atomic::{AtomicBool, Ordering};
use vault_buddy_core::{services, uri};

/// Last facing emitted to the buddy window, so `emit_buddy_facing` fires the
/// `buddy-facing` event only when the buddy actually crosses the screen midline
/// — a drag would otherwise flood the webview with one event per Moved. Seeded
/// by `get_buddy_facing` (the buddy's mount-time read) so the first real flip
/// still emits. Only touched on the main thread; a relaxed atomic is enough.
static LAST_FACES_RIGHT: AtomicBool = AtomicBool::new(true);

/// The buddy's facing, DERIVED from its position: it looks toward the center of
/// the work area (and the bubble opens the same way), instead of a manual
/// setting. Best-effort — `Right` when the geometry isn't available yet.
fn current_facing(app: &tauri::AppHandle) -> vault_buddy_core::companion_placement::Side {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{toward_center_side, Rect, Side};
    let Some(buddy) = app.get_webview_window("main") else {
        return Side::Right;
    };
    let (Ok(bpos), Ok(bsize)) = (buddy.outer_position(), buddy.outer_size()) else {
        return Side::Right;
    };
    let buddy_rect = Rect {
        x: bpos.x,
        y: bpos.y,
        w: bsize.width as i32,
        h: bsize.height as i32,
    };
    let work = buddy.current_monitor().ok().flatten().map(|m| {
        let wa = m.work_area();
        Rect {
            x: wa.position.x,
            y: wa.position.y,
            w: wa.size.width as i32,
            h: wa.size.height as i32,
        }
    });
    toward_center_side(buddy_rect, work)
}

fn facing_str(side: vault_buddy_core::companion_placement::Side) -> &'static str {
    use vault_buddy_core::companion_placement::Side;
    match side {
        Side::Right => "right",
        Side::Left => "left",
    }
}

/// The buddy's current facing, derived from its position. Called by the buddy
/// window on mount to set the initial sprite direction; later flips arrive via
/// the `buddy-facing` event. Seeds `LAST_FACES_RIGHT` so the dedup in
/// `emit_buddy_facing` is aligned with what the sprite already shows.
#[tauri::command]
pub fn get_buddy_facing(app: tauri::AppHandle) -> String {
    let side = current_facing(&app);
    LAST_FACES_RIGHT.store(
        matches!(side, vault_buddy_core::companion_placement::Side::Right),
        Ordering::Relaxed,
    );
    facing_str(side).to_string()
}

/// Recompute the buddy's facing from its position and, if it changed, emit
/// `buddy-facing` so the sprite flips to look toward the screen center. Deduped
/// via `LAST_FACES_RIGHT`, so a drag emits only when the buddy crosses the
/// midline, not on every Moved. Runs on the main thread; best-effort.
pub(crate) fn emit_buddy_facing(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let faces_right = matches!(
        current_facing(app),
        vault_buddy_core::companion_placement::Side::Right
    );
    if LAST_FACES_RIGHT.swap(faces_right, Ordering::Relaxed) != faces_right {
        let _ = app.emit("buddy-facing", if faces_right { "right" } else { "left" });
    }
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

/// Position (while hidden), show, focus the panel window, tell the panel
/// webview it was just revealed, and hide the greeting bubble. Idempotent —
/// safe to call on an already-visible panel (a harmless re-show/re-focus/
/// re-emit, no flicker, since the panel only ever moves, never resizes).
///
/// Factored out of `toggle_panel`'s show branch so `begin_document_import`
/// can reuse it: a buddy drop must always SHOW the panel, never toggle it —
/// toggling would HIDE an already-open panel instead of routing it to the
/// import picker.
pub(crate) fn show_panel(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let Some(panel) = app.get_webview_window("panel") else {
        log::warn!("show_panel: no panel window");
        return;
    };
    position_panel(app);
    if let Err(e) = panel.show() {
        log::warn!("show_panel: show failed: {e}");
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

/// Show/hide the panel window. A sync command, so it runs on the main thread
/// (where window show/hide and the placement getters are valid). Positioned
/// while still hidden, then shown — the panel window never resizes and is
/// only moved, so there is no WebView2 stale-frame flash. Opening hides the
/// greeting bubble (via `show_panel`).
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
    show_panel(&app);
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

/// Show the panel window (idempotent). The clickable bubble calls this on a
/// click that carries an action: unlike `toggle_panel` it never HIDES an
/// already-open panel, so a bubble click always REVEALS the panel — which
/// then runs its `panel-shown` refresh and consumes the armed pending view
/// (for the update announcement, the dedicated update view). Sync, so it runs
/// on the main thread where window show/focus are valid.
#[tauri::command]
pub fn open_panel(app: tauri::AppHandle) {
    show_panel(&app);
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
/// window's width so it sits snug against the character instead of floating in
/// the window's transparent padding. Windows clamps the tiny 88px buddy window
/// up to its minimum window size (much wider than the ~64px character), so there
/// is a lot of padding to cross; the character is centered in that window
/// (`BuddyRoot`), so the padding — and this tuck — are symmetric on both sides.
/// A fraction rather than a fixed px so it scales with display DPI. Cosmetic,
/// tuned against a manual Windows check — a single symmetric knob now that the
/// character is centered. The panel uses 0.0.
const BUBBLE_TUCK_FRAC: f64 = 0.15;

/// Which side a companion window prefers: a fixed side (the panel always opens
/// right) or derived from the buddy's position (the bubble opens toward the
/// work-area center).
enum SidePref {
    Fixed(vault_buddy_core::companion_placement::Side),
    TowardCenter,
}

/// Top-left AND the resolved anchor for a companion window (panel or bubble)
/// placed beside the buddy per `side_pref` with vertical mode `vmode`.
/// `tuck_frac` pulls the window toward the buddy by that fraction of the buddy
/// width (0.0 = flush beside). `None` when the buddy or target geometry isn't
/// available yet — callers then leave the window where it was (best-effort).
fn place_beside_buddy(
    app: &tauri::AppHandle,
    target: &tauri::WebviewWindow,
    side_pref: SidePref,
    vmode: vault_buddy_core::companion_placement::VMode,
    tuck_frac: f64,
) -> Option<(
    tauri::PhysicalPosition<i32>,
    vault_buddy_core::companion_placement::Anchor,
)> {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{place_beside, toward_center_side, Rect, Side};
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
    let prefer = match side_pref {
        SidePref::Fixed(s) => s,
        SidePref::TowardCenter => toward_center_side(buddy_rect, work),
    };
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
    Some((tauri::PhysicalPosition::new(x, point.y), anchor))
}

/// The `{side, valign}` payload (matching the SpeechBubble props) for an anchor.
/// Shared by the `bubble-anchor` event and the `get_bubble_anchor` pull.
fn anchor_payload(anchor: vault_buddy_core::companion_placement::Anchor) -> serde_json::Value {
    use vault_buddy_core::companion_placement::{Side, VAlign};
    serde_json::json!({
        "side": match anchor.side {
            Side::Left => "left",
            Side::Right => "right",
        },
        "valign": match anchor.valign {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        },
    })
}

/// Tell the bubble window which side/valign it landed on so it can draw its
/// tail pointing back at the buddy. Emitted on every (re)placement, so the tail
/// flips live when a drag carries the buddy across the screen midline or to an
/// edge. Emitted app-wide (only the bubble window listens).
fn emit_bubble_anchor(
    app: &tauri::AppHandle,
    anchor: vault_buddy_core::companion_placement::Anchor,
) {
    use tauri::Emitter;
    let _ = app.emit("bubble-anchor", anchor_payload(anchor));
}

/// The bubble's current placement anchor, so `BubbleRoot` can render its tail
/// correctly on mount instead of racing the `bubble-anchor` event: the bubble
/// webview is hidden until the greeting shows, so it can register its listener
/// after the startup emits have already fired (the "bubble too high until I
/// drag" bug). Mirrors the facing pull (`get_buddy_facing`). Defaults to
/// right/middle when the geometry isn't available.
#[tauri::command]
pub fn get_bubble_anchor(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    let anchor = app.get_webview_window("bubble").and_then(|bubble| {
        place_beside_buddy(
            &app,
            &bubble,
            SidePref::TowardCenter,
            vault_buddy_core::companion_placement::VMode::Center,
            BUBBLE_TUCK_FRAC,
        )
        .map(|(_, a)| a)
    });
    match anchor {
        Some(a) => anchor_payload(a),
        None => serde_json::json!({ "side": "right", "valign": "middle" }),
    }
}

/// Move the (hidden) panel window beside the buddy, respecting screen edges.
/// Best-effort: any missing window/monitor info leaves the panel where it was.
/// The panel prefers the right side, edge-aligns vertically, and ignores the
/// anchor.
///
/// Also sizes the panel from the configured preset — but ONLY while the
/// panel is hidden. `show_panel` calls this unconditionally, and `show_panel`
/// is itself called on an already-VISIBLE panel from a few idempotent-reveal
/// paths (`open_panel`'s bubble click, `begin_document_import`,
/// `begin_add_document`) — so the visibility check has to live here, not just
/// at those call sites, or one of them would resize a visible surface and
/// reintroduce the WebView2 stale-frame flash this window system exists to
/// avoid. When hidden, sizing first is safe (nothing is on screen to flash)
/// and correct: `place_beside_buddy` below reads the panel's just-updated
/// `outer_size()`, so the chosen preset is positioned correctly. A size
/// change made while the panel is open simply doesn't apply until the panel
/// is next hidden and reshown — the documented way to re-apply a size (see
/// `set_panel_size`), never a `set_size` on a shown window.
pub(crate) fn position_panel(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{Side, VMode};
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    if !panel.is_visible().unwrap_or(false) {
        let (w, h) = vault_buddy_core::capture_config::load_config()
            .panel
            .size
            .dims();
        if let Err(e) = panel.set_size(tauri::LogicalSize::new(w, h)) {
            log::warn!("position_panel: set_size failed: {e}");
        }
    }
    if let Some((pos, _anchor)) =
        place_beside_buddy(app, &panel, SidePref::Fixed(Side::Right), VMode::Edge, 0.0)
    {
        if let Err(e) = panel.set_position(pos) {
            log::warn!("position_panel: set_position failed: {e}");
        }
    }
}

/// The panel's configured preset size (`"compact"` | `"comfortable"` |
/// `"large"`), as read by `get_panel_config` and written by `set_panel_size`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelConfigDto {
    pub size: String,
}

/// The panel's configured preset size, for the settings control.
#[tauri::command]
pub fn get_panel_config() -> PanelConfigDto {
    PanelConfigDto {
        size: vault_buddy_core::capture_config::load_config()
            .panel
            .size
            .as_str()
            .to_string(),
    }
}

/// Persist the panel preset size. This never touches a visible window itself
/// — it only writes config.json. The new size takes effect the next time the
/// panel goes hidden → shown (`position_panel`'s guard above), which the
/// frontend triggers by re-showing the panel after a successful save.
/// Read-modify-write under `ConfigWriteLock`, mirroring the sibling
/// app-global-section writers (`set_transcription_config`,
/// `mcp_commands::set_mcp_config`'s `persist`): resolve `config_path`, then
/// call `capture_config::update_panel_config_at` — the same config-write
/// helper every other app-global section's setter calls, never a
/// hand-rolled writer.
///
/// `size` is not pre-validated against a fixed list the way `set_capture_config`
/// gates `mode`/`bitrate_kbps`/`transcription_model` (or `delete_transcription_model`
/// gates its model id): those all name a real file/tier a wrong match would
/// silently act on, so a typo must error, not default. A panel size has no
/// such target — `PanelSize::from_str` is infallible by design specifically so
/// an unrecognized value (impossible from the closed preset UI; only reachable
/// via a stale/future client) safely lands on `Comfortable` instead of
/// rejecting the save.
#[tauri::command]
pub fn set_panel_size(
    lock: tauri::State<'_, crate::capture_commands::ConfigWriteLock>,
    size: String,
) -> Result<(), String> {
    use vault_buddy_core::capture_config;
    use vault_buddy_core::sync_util::lock_ignoring_poison;
    let new_size = capture_config::PanelSize::from_str(&size);
    let _guard = lock_ignoring_poison(&lock.0);
    let path = capture_config::config_path().ok_or("Cannot resolve the config directory")?;
    capture_config::update_panel_config_at(&path, capture_config::PanelConfig { size: new_size })
        .map_err(|e| format!("Could not save panel settings: {e}"))
}

/// Position + show the greeting bubble beside the buddy on launch. Opens on the
/// side the buddy faces and emits the resolved anchor so the tail points back
/// at the buddy. Best-effort; shown only once positioned — a moved-only window
/// has no stale-frame flash. Returns whether the bubble was actually shown so
/// `announce` can skip delivering text to a window that never appeared.
pub(crate) fn show_bubble(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    // The bubble is the buddy's accessory: hidden-to-tray hides ALL windows
    // (tray::hide_buddy), and a background announcer — the startup update
    // check, a transcription finishing — must not pop a bubble beside an
    // invisible character. Guarded here, at the single positioning/reveal
    // chokepoint, so the greeting's post-settle show and every announce path
    // inherit it.
    let buddy_visible = app
        .get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false);
    if !buddy_visible {
        log::info!("bubble: suppressed while the buddy is hidden");
        return false;
    }
    let Some(bubble) = app.get_webview_window("bubble") else {
        return false;
    };
    let Some((pos, anchor)) = place_beside_buddy(
        app,
        &bubble,
        SidePref::TowardCenter,
        vault_buddy_core::companion_placement::VMode::Center,
        BUBBLE_TUCK_FRAC,
    ) else {
        return false;
    };
    // A window created `visible: false` can ignore `set_position` until it has
    // been shown and realized on its monitor — the cause of "the greeting is
    // placed right only after I move the buddy" (a drag's set_position lands
    // because the window is realized by then, but the startup pre-show one is
    // dropped). So position, show, then position again: the post-show call is
    // the authoritative one.
    let _ = bubble.set_position(pos);
    let _ = bubble.show();
    let _ = bubble.set_position(pos);
    emit_bubble_anchor(app, anchor);
    true
}

/// Speak an acknowledgement in the buddy's bubble. The frontend announcer (the
/// buddy window for capture progress, the panel window's vaults store for
/// vault/note opens) composes the text and gates it on the "Buddy messages"
/// setting before calling this. Positions + reveals the bubble beside the buddy
/// exactly like the greeting, then pushes the words to `BubbleRoot` via
/// `bubble-message`; the bubble's own auto-dismiss timer hides it again
/// (`close_bubble`). Best-effort throughout — a missing window just means no
/// bubble, never an error to the caller.
#[tauri::command]
pub fn announce(app: tauri::AppHandle, text: String, action: Option<String>) {
    use tauri::Emitter;
    // Same placement/reveal path as the launch greeting. A suppressed show
    // (buddy hidden to tray) also skips the text emit: delivering it would
    // start BubbleRoot's dismiss timer inside a window that never appeared.
    if !show_bubble(&app) {
        return;
    }
    // Deliver the text and the optional click action (which makes the bubble
    // clickable in BubbleRoot); BubbleRoot renders it and (re)starts its
    // dismiss timer.
    let _ = app.emit(
        "bubble-message",
        serde_json::json!({ "text": text, "action": action }),
    );
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
        SidePref::TowardCenter,
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

    // The menu's only vault ACTION leads; the toggles and window controls
    // follow. Ellipsis = more UI follows (the vault-first import picker).
    // "Import document" is the domain's verb for this capability (CONTEXT.md;
    // the panel view title and RecordMode's action use it too).
    let import_doc = MenuItem::with_id(
        &app,
        "buddy-import-document",
        "Import document…",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let sep_import = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
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
    let menu = Menu::with_items(
        &app,
        &[
            &import_doc,
            &sep_import,
            &animation,
            &dragging,
            &separator,
            &hide,
            &quit,
        ],
    )
    .map_err(|e| e.to_string())?;
    // Win32 popup menus require the owning window to be foreground —
    // without this the menu is delayed or silently ignored until the user
    // left-clicks the (unfocused) buddy first.
    let _ = window.set_focus();
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

/// Vault lookup for shell commands living outside the services layer (the
/// search commands). Delegates to `services::find_vault` so the user-facing
/// not-found copy stays single-sourced with the panel's.
pub(crate) fn find_vault(id: &str) -> Result<vault_buddy_core::discovery::Vault, String> {
    services::find_vault(&services::ServicePaths::real(), id)
}

#[tauri::command]
pub fn list_vaults() -> Vec<vault_buddy_core::discovery::Vault> {
    services::list_vaults(&services::ServicePaths::real())
}

#[tauri::command]
pub fn open_vault(id: String) -> Result<(), String> {
    services::open_vault(&services::ServicePaths::real(), &id, &|u| uri::launch(u))
}

#[tauri::command]
pub fn open_daily_note(id: String) -> Result<(), String> {
    let today = Local::now().date_naive();
    // allow_create: true — the human UI keeps its open-or-create behavior.
    // The created-vs-opened bool exists for the MCP tool's on_write hook;
    // the IPC contract stays Result<(), String>.
    services::open_daily_note(&services::ServicePaths::real(), &id, today, true, &|u| {
        uri::launch(u)
    })
    .map(|_created| ())
}

/// Reveal the app log folder (holding `vault-buddy.log` and `crash.log`) in
/// the OS file manager, so a user can attach logs after a crash.
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) {
    crate::diagnostics::open_log_dir(&app);
}

/// Suppress the panel's focus-out auto-hide while a native OS dialog (file
/// picker / Pandoc Browse) is in flight. The frontend calls this `true` before
/// `open()` and `false` in its `finally`, so the dialog stealing OS focus can't
/// hide the panel (and its in-progress import state) out from under the user.
/// Sync → main thread, where the focus-out check that reads it also runs.
#[tauri::command]
pub fn set_dialog_active(active: bool) {
    crate::set_dialog_active(active);
}

/// Open an external `https://` link in the OS default browser. The frontend
/// never navigates the panel webview to an external URL directly — in a Tauri
/// v2 webview a raw `target="_blank"` either no-ops or replaces the app UI —
/// so links (e.g. the Pandoc install page) route through here. Restricted to
/// `https://` so the frontend can't launch arbitrary schemes (`file:`, custom
/// handlers); `uri::launch` logs it as the same audit trail every launched
/// URI gets.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("refusing to open non-https URL".into());
    }
    uri::launch(&url)
}

/// The frontend calls this when an update install fails after
/// prepare_update_install stamped a clean shutdown — the app keeps
/// running, so crash detection must come back on.
#[tauri::command]
pub fn rearm_crash_detection() {
    log::warn!("update install failed after shutdown prep — re-arming crash detection");
    crate::diagnostics::rearm_running_marker();
}

/// Whether the app is registered to start at login. OS-owned state (the
/// registry on Windows) — read fresh by the settings view on mount, never
/// cached app-side, so the UI always reflects what the OS will actually do.
#[tauri::command]
pub fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Register/unregister launch-at-login. Logged like every other
/// user-initiated config change (audit trail).
#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    match result {
        Ok(()) => {
            log::info!("autostart {}", if enabled { "enabled" } else { "disabled" });
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}
