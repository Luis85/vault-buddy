//! Region selection (spec 5.2, 11): show the overlay over ONE monitor,
//! wait for the user's drag, and hand back a `region:` source id.
//!
//! Split out of `screen_commands.rs` because that file is near its 800-line
//! cap, and because this is a WINDOW-lifecycle concern rather than a
//! capture-lifecycle one — nothing here touches `CaptureGuard`,
//! `ScreenCaptureState` or the session.
//!
//! **Why the overlay covers exactly one monitor.** `to_physical` applies
//! ONE scale factor to the origin and the size alike, which is only
//! correct for monitor-local coordinates at that monitor's own DPI (see
//! `core::screen_geometry`'s module doc). Sizing the overlay to one
//! monitor makes its viewport origin that monitor's origin, so the
//! webview's coordinates are monitor-local by construction rather than by
//! a subtraction somebody has to remember.
//!
//! **Why the webview's `devicePixelRatio` is carried but never used.** The
//! webview knows its own ratio; it does not know which monitor Rust picked.
//! Two sources of truth for the scale factor is the most likely way this
//! feature goes wrong on a mixed-DPI desktop, so the monitor's
//! `scale_factor` is the only one that scales anything and a disagreement
//! is LOGGED — visible in a bug report, inert in the arithmetic.

use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};
use vault_buddy_core::screen_geometry::{clamp_to_frame, to_physical, LogicalRect};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::region::{self, RegionSource};
use vault_buddy_screen::source::SourceId;

const OVERLAY_LABEL: &str = "overlay";

/// Told to the overlay immediately before it is shown, so `RegionRoot` can
/// re-arm itself for a NEW selection.
///
/// The overlay window is hidden and REUSED, never reloaded, so the webview's
/// component state survives from one selection to the next — including the
/// one-shot latch that stops a second answer to a single selection. Without
/// this signal the second selection of an app run paints a full-screen,
/// always-on-top scrim that ignores every pointerdown AND Escape until the
/// 120-second wait below expires, then reports a cancel the user never made.
const REGION_BEGIN_EVENT: &str = "region:begin";

/// A selection nobody ever answers must not strand a full-screen,
/// invisible, always-on-top window over the user's desktop. Generous
/// because drawing a rectangle is a human action, bounded because a
/// crashed webview is a real outcome.
const REGION_TIMEOUT: Duration = Duration::from_secs(120);

/// How long the overlay has to come up. The closure below runs on the main
/// thread and does three window calls; anything near this bound means the
/// event loop is wedged, and reporting that beats waiting on it forever.
const OVERLAY_SHOW_TIMEOUT: Duration = Duration::from_secs(5);

/// One drag, in LOGICAL (CSS) pixels relative to the overlay's viewport,
/// i.e. to the target monitor's origin.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionPick {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The webview's own `devicePixelRatio`. Diagnostic only — see the
    /// module doc.
    pub dpr: f64,
}

/// What the picker renders and hands to `start_screen_capture`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionSelectionDto {
    pub source_id: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The one-shot answer slot. `Some` exactly while a selection is in
/// flight, so a second `select_capture_region` is refused rather than
/// stealing the first one's answer.
#[derive(Default)]
pub struct RegionSelectionState(pub Mutex<Option<Sender<Option<RegionPick>>>>);

/// Does this Tauri monitor name denote GDI display `display`?
///
/// The ONE place the two monitor numbering schemes are joined: Tauri names
/// a Windows monitor by `MONITORINFOEXW.szDevice` (`\\.\DISPLAYn`) and
/// `windows_capture::monitor::Monitor::index()` is that same `n`. Never
/// match on a friendly name, and never use a positional index — phase 2
/// shipped a wrong-screen bug by crossing exactly these.
pub(crate) fn matches_display(name: Option<&str>, display: usize) -> bool {
    name.and_then(region::display_number_from_device_name) == Some(display)
}

/// The GDI display number a region selection targets, or `None` if the id
/// is not a whole screen.
fn target_display(source_id: &str) -> Option<usize> {
    match SourceId::parse(source_id)? {
        SourceId::Screen(n) => Some(n),
        // A window moves and resizes; a region of one is not a rectangle
        // on a monitor and the overlay has nothing to cover. A region of a
        // region is meaningless. Both are refused rather than silently
        // falling back to the primary display.
        SourceId::Window(_) | SourceId::Region(_) => None,
    }
}

/// Turn one logical drag into a region source id, scaled by the MONITOR's
/// factor and clamped to the monitor's current size.
fn region_from_pick(
    picked: RegionPick,
    scale: f64,
    monitor_w: u32,
    monitor_h: u32,
    display: usize,
) -> Result<RegionSelectionDto, String> {
    if (picked.dpr - scale).abs() > 0.01 {
        // Not an error: the monitor is the authority and the arithmetic
        // below already uses it. This line is how a mixed-DPI mis-scale
        // becomes visible in a bug report instead of being invisible.
        log::warn!(
            "region select: the overlay reported devicePixelRatio {} but monitor {display} \
             scales at {scale}; using the monitor's factor",
            picked.dpr
        );
    }
    let physical = to_physical(
        LogicalRect {
            x: picked.x,
            y: picked.y,
            width: picked.width,
            height: picked.height,
        },
        scale,
    );
    // Clamped at SELECTION time so an impossible region is refused while
    // the user is still looking at the picker. It is clamped AGAIN at
    // capture start (`source::resolve`), because the resolution can change
    // in between — this one is the friendly refusal, that one is the
    // correctness gate.
    let rect = clamp_to_frame(physical, monitor_w, monitor_h)
        .ok_or_else(|| "That region is not on the screen any more.".to_string())?;
    Ok(RegionSelectionDto {
        source_id: SourceId::Region(RegionSource {
            monitor: display,
            rect,
        })
        .to_string(),
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    })
}

/// Claim the one-shot answer slot for this selection, or report that one is
/// already in flight.
///
/// Returns `false` **without disturbing the sender already there**: refusing
/// a second selection, never stealing the first one's answer. An
/// unconditional `*slot = Some(tx)` would compile, still return `false`, and
/// leave the first caller parked on a channel nobody can ever answer.
///
/// Its own function, and free of `AppHandle`, so the refusal is a real unit
/// test rather than a byte-offset proxy over the source.
fn claim_slot(state: &RegionSelectionState, tx: Sender<Option<RegionPick>>) -> bool {
    let mut slot = lock_ignoring_poison(&state.0);
    if slot.is_some() {
        return false;
    }
    *slot = Some(tx);
    true
}

/// Hide the overlay, drop any pending answer slot, let the panel auto-hide
/// again, and give the panel back its focus.
///
/// THE ONE CLEANUP SITE, the `clear_active_screen` discipline applied to
/// this feature's paired state: a path that hid the overlay without
/// clearing `DIALOG_ACTIVE` would leave the panel unable to auto-hide for
/// the rest of the session, with no error and no log line. Pinned by a
/// structural test below.
fn finish_region_selection(app: &AppHandle) {
    *lock_ignoring_poison(&app.state::<RegionSelectionState>().0) = None;
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(overlay) = handle.get_webview_window(OVERLAY_LABEL) {
            let _ = overlay.hide();
        }
        // The overlay took focus when it was shown; hand it back rather
        // than leaving the user on whatever Windows picks next.
        if let Some(panel) = handle.get_webview_window("panel") {
            if panel.is_visible().unwrap_or(false) {
                let _ = panel.set_focus();
            }
        }
    });
    crate::set_dialog_active(false);
}

/// ASYNC: this WAITS for a human to draw a rectangle. A sync command runs
/// on the main thread, where waiting freezes the event loop — which is
/// also the thread that has to dispatch the overlay's own pointer events,
/// so a sync version could never complete.
#[tauri::command]
pub async fn select_capture_region(
    app: AppHandle,
    source_id: String,
) -> Result<Option<RegionSelectionDto>, String> {
    let display = target_display(&source_id)
        .ok_or_else(|| "Pick a screen to select a region on.".to_string())?;

    // The answer slot is claimed HERE, not inside, and both refusals above
    // and below return BEFORE the one cleanup call. Cleanup drops whatever
    // sender the slot holds, so a second call that fell through to it
    // would cancel the selection it is refusing to duplicate — the exact
    // opposite of refusing rather than stealing the first one's answer.
    let (tx, rx) = mpsc::channel::<Option<RegionPick>>();
    // The `State` guard is a statement temporary so it cannot be held across
    // the `.await` below.
    if !claim_slot(&app.state::<RegionSelectionState>(), tx) {
        return Err("A region selection is already in progress.".to_string());
    }

    // Set beside the claim, not deeper in: the one cleanup below clears this
    // flag unconditionally, so anything that sets it later leaves early
    // failure paths CLEARING a suppression they never set — and
    // `DIALOG_ACTIVE` is one process-wide bool the frontend's
    // `withDialogSuppressed` drives too, so that clear would un-suppress an
    // open native file dialog and let the panel auto-hide out from under it.
    // Set and cleared at the same scope as the slot; every path that clears
    // it has now also set it.
    //
    // Why it is needed at all: the overlay steals OS focus the moment it is
    // shown, and without the suppression the panel's focus-out check hides
    // the panel — and the picker state the user is halfway through — out
    // from under them.
    crate::set_dialog_active(true);

    let outcome = select_region_inner(&app, display, rx).await;
    finish_region_selection(&app);
    outcome
}

async fn select_region_inner(
    app: &AppHandle,
    display: usize,
    rx: mpsc::Receiver<Option<RegionPick>>,
) -> Result<Option<RegionSelectionDto>, String> {
    // `available_monitors` marshals to the event loop and blocks on the
    // reply with no timeout (the same tauri-runtime-wry round trip
    // `screen_commands::our_window_titles` documents). Safe only because
    // this command is async; do not make it sync.
    let monitors = app
        .available_monitors()
        .map_err(|e| format!("Could not read the display layout: {e}"))?;
    let monitor = monitors
        .into_iter()
        .find(|m| matches_display(m.name().map(String::as_str), display))
        .ok_or_else(|| "That screen is no longer connected.".to_string())?;
    let position = *monitor.position();
    let size = *monitor.size();
    let scale = monitor.scale_factor();
    drop(monitor);

    // POSITION AND SIZE WHILE HIDDEN, then show: a moved-or-resized window
    // that is already visible repaints its stale last frame at the new
    // bounds for a frame (AGENTS.md, "The window system"). Physical units
    // throughout, so no logical/physical conversion happens on the way to
    // a monitor whose scale we are about to depend on.
    let (setup_tx, setup_rx) = mpsc::channel::<Result<(), String>>();
    let shower = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let overlay = shower
                .get_webview_window(OVERLAY_LABEL)
                .ok_or_else(|| "The region overlay window is missing.".to_string())?;
            overlay
                .set_position(PhysicalPosition::new(position.x, position.y))
                .map_err(|e| format!("Could not place the region overlay: {e}"))?;
            overlay
                .set_size(PhysicalSize::new(size.width, size.height))
                .map_err(|e| format!("Could not size the region overlay: {e}"))?;
            // Re-arm the overlay's one-shot latch BEFORE it is shown, and
            // from inside this same main-thread closure so the ordering is
            // not a hope. The latch cannot lose a race with the user's
            // first press of the NEW selection: a hidden window receives no
            // pointer input at all, so no pointer event for this selection
            // can exist yet, and this event is already queued onto the
            // webview's own task queue — which is FIFO — by the time
            // `show()` makes input possible. Emitting after the show, or
            // from a worker thread, would have neither guarantee.
            //
            // A failed emit is a hard error rather than a `let _ =`: an
            // overlay that was not re-armed is the inert full-screen scrim
            // this event exists to prevent, so refusing to show it beats
            // showing one the user cannot dismiss for two minutes.
            shower
                .emit_to(OVERLAY_LABEL, REGION_BEGIN_EVENT, ())
                .map_err(|e| format!("Could not arm the region overlay: {e}"))?;
            overlay
                .show()
                .map_err(|e| format!("Could not show the region overlay: {e}"))?;
            let _ = overlay.set_focus();
            Ok(())
        })();
        let _ = setup_tx.send(result);
    })
    .map_err(|e| format!("Could not reach the main thread: {e}"))?;
    setup_rx
        .recv_timeout(OVERLAY_SHOW_TIMEOUT)
        .map_err(|_| "The region overlay did not open.".to_string())??;

    let picked = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(REGION_TIMEOUT))
        .await
        .map_err(|e| format!("Region selection failed: {e}"))?;
    let picked = match picked {
        Ok(p) => p,
        Err(_) => {
            // The webview never answered. Treat it as a cancel rather than
            // an error the user has to dismiss: nothing is broken, and the
            // cleanup below is about to take the overlay down.
            log::warn!("region select: no answer within {REGION_TIMEOUT:?}; cancelling");
            None
        }
    };
    let Some(picked) = picked else {
        return Ok(None);
    };
    region_from_pick(picked, scale, size.width, size.height, display).map(Some)
}

/// SYNC: takes the one-shot sender and answers. O(1) under a mutex and one
/// channel send on an unbounded channel, so it cannot block the main
/// thread — the same posture as the sibling screen commands.
///
/// Deliberately does NOT hide the overlay. `select_capture_region`'s single
/// cleanup does that, on this path and on the timeout path alike; a second
/// hide site here is the thing the structural test below forbids.
#[tauri::command]
pub fn resolve_region_selection(app: AppHandle, rect: Option<RegionPick>) {
    let state = app.state::<RegionSelectionState>();
    let sender = lock_ignoring_poison(&state.0).take();
    match sender {
        Some(tx) => {
            if tx.send(rect).is_err() {
                // The bounded wait expired and dropped the receiver between
                // the `take()` above and this send, so the rectangle the
                // user drew has nowhere to go and the selection reports a
                // cancel. A narrow race, but "the app threw away a region I
                // drew" must not be invisible in the log.
                log::warn!(
                    "region select: an answer arrived just after the wait expired; discarding it"
                );
            }
        }
        // Not an error: a duplicate pointerup, or an answer arriving after
        // the timeout already cancelled. Logged, never silent.
        None => log::debug!("region select: an answer arrived with no selection in flight"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(x: f64, y: f64, width: f64, height: f64) -> RegionPick {
        RegionPick {
            x,
            y,
            width,
            height,
            dpr: 1.0,
        }
    }

    // THE PHASE GATE, in the one place a CI runner can reach it. Each
    // expected value is logical x scale, worked out by hand:
    //   100%  : 320x180 at (100, 50) stays 320x180 at (100, 50)
    //   125%  : 800x600 at (0, 0)    -> 1000x750
    //   150%  : 1280x720 at (0, 0)   -> 1920x1080
    //   200%  : 100x100 at (10, 10)  -> 200x200 at (20, 20)
    #[test]
    fn a_pick_scales_by_the_monitors_own_factor() {
        assert_eq!(
            region_from_pick(pick(100.0, 50.0, 320.0, 180.0), 1.0, 1920, 1080, 1),
            Ok(RegionSelectionDto {
                source_id: "region:1,100,50,320,180".into(),
                x: 100,
                y: 50,
                width: 320,
                height: 180,
            })
        );
        assert_eq!(
            region_from_pick(pick(0.0, 0.0, 800.0, 600.0), 1.25, 2560, 1440, 1)
                .map(|d| (d.width, d.height)),
            Ok((1000, 750))
        );
        assert_eq!(
            region_from_pick(pick(0.0, 0.0, 1280.0, 720.0), 1.5, 2560, 1440, 2).map(|d| (
                d.source_id,
                d.width,
                d.height
            )),
            Ok(("region:2,0,0,1920,1080".into(), 1920, 1080))
        );
        assert_eq!(
            region_from_pick(pick(10.0, 10.0, 100.0, 100.0), 2.0, 3840, 2160, 1)
                .map(|d| (d.x, d.y, d.width, d.height)),
            Ok((20, 20, 200, 200))
        );
    }

    // The overlay reports what the WEBVIEW thinks; the monitor is the
    // authority. Sending a wrong dpr must change nothing but the log --
    // if this ever starts failing, someone has wired the webview's ratio
    // into the arithmetic and mixed-DPI setups will silently mis-scale.
    #[test]
    fn the_webviews_device_pixel_ratio_never_scales_anything() {
        let lying = RegionPick {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            dpr: 3.0,
        };
        assert_eq!(
            region_from_pick(lying, 2.0, 3840, 2160, 1).map(|d| (d.width, d.height)),
            Ok((200, 200)),
            "scaled by the monitor's 2.0, not the webview's 3.0"
        );
    }

    // H.264 with NV12 4:2:0 needs even dimensions on both axes (GAP-101),
    // and clamp_to_frame rounds DOWN -- rounding up would grow the region
    // past what the user drew. 401 logical at 1.5 is 601.5 -> 601 -> 600.
    #[test]
    fn an_odd_physical_dimension_is_rounded_down_to_even() {
        assert_eq!(
            region_from_pick(pick(0.0, 0.0, 401.0, 300.0), 1.5, 2560, 1440, 1)
                .map(|d| (d.width, d.height)),
            Ok((600, 450))
        );
    }

    // The monitor is smaller than the scaled rectangle claims -- which is
    // what a resolution change between drawing and picking looks like.
    #[test]
    fn a_pick_overflowing_the_monitor_is_truncated() {
        assert_eq!(
            region_from_pick(pick(1800.0, 1000.0, 400.0, 400.0), 1.0, 1920, 1080, 1)
                .map(|d| (d.x, d.y, d.width, d.height)),
            Ok((1800, 1000, 120, 80))
        );
    }

    #[test]
    fn a_pick_entirely_off_the_monitor_is_an_error() {
        assert!(region_from_pick(pick(5000.0, 5000.0, 100.0, 100.0), 1.0, 1920, 1080, 1).is_err());
        // And a sub-2-physical-pixel region, which rounds to zero.
        assert!(region_from_pick(pick(0.0, 0.0, 1.0, 100.0), 1.0, 1920, 1080, 1).is_err());
    }

    // The join between Tauri's monitor naming and windows-capture's index
    // (see the plan's Global Constraints). A monitor with no readable name
    // must not match SOMETHING -- it must match nothing.
    #[test]
    fn a_monitor_is_matched_by_its_gdi_display_number() {
        assert!(matches_display(Some(r"\\.\DISPLAY2"), 2));
        assert!(!matches_display(Some(r"\\.\DISPLAY2"), 1));
        assert!(!matches_display(Some("Dell U2720Q"), 2));
        assert!(!matches_display(None, 2));
        assert!(!matches_display(None, 0));
    }

    // Only a whole SCREEN can be the target of a region selection: the
    // overlay covers one monitor, which is what makes to_physical's single
    // scale factor correct. A window or an existing region id must be
    // refused rather than quietly selecting on the primary display.
    #[test]
    fn only_a_screen_id_can_be_the_region_target() {
        assert_eq!(target_display("screen:2"), Some(2));
        assert_eq!(target_display("window:1234"), None);
        assert_eq!(target_display("region:1,0,0,100,100"), None);
        assert_eq!(target_display("nonsense"), None);
    }

    // The semantic half of "a second selection is refused rather than
    // stealing the first one's answer". The `false` return is the cheap
    // half; the half that matters is that the FIRST caller's sender is
    // still the one in the slot afterwards. An unconditional
    // `*slot = Some(tx)` that returned `slot.is_none()` would satisfy the
    // return-value assertion, compile, and silently park the first caller
    // on a channel nobody can answer for the full 120-second wait.
    #[test]
    fn a_second_claim_is_refused_without_replacing_the_first_sender() {
        let state = RegionSelectionState::default();
        let (tx1, rx1) = mpsc::channel::<Option<RegionPick>>();
        // `_rx2` is a real binding, not a bare `_`: the second receiver has
        // to stay alive, or sending on a leaked tx2 would fail for the
        // wrong reason and the probe below would pass by accident.
        let (tx2, _rx2) = mpsc::channel::<Option<RegionPick>>();

        assert!(claim_slot(&state, tx1), "the first claim takes a free slot");
        assert!(
            !claim_slot(&state, tx2),
            "a second claim while one is in flight is refused"
        );

        let held = lock_ignoring_poison(&state.0)
            .take()
            .expect("the slot still holds a sender after the refused claim");
        assert!(
            held.send(None).is_ok(),
            "the sender in the slot must still have a live receiver"
        );
        assert!(
            matches!(rx1.try_recv(), Ok(None)),
            "the slot must still hold the FIRST caller's sender, not the refused one"
        );
    }

    /// Production source only. `include_str!` pulls in THIS file, tests
    /// included, so a scan over the whole string counts the test's own
    /// literals and an `== 1` assertion becomes unreachable — the
    /// `production_src()` precedent from `screen/src/sink.rs` and
    /// `screen_commands.rs`, which two earlier tasks on this branch got
    /// wrong before review caught it.
    fn production_src() -> &'static str {
        let src = include_str!("region_commands.rs");
        match src.find("\n#[cfg(test)]") {
            Some(i) => &src[..i],
            None => src,
        }
    }

    // A second cleanup path that hid the overlay but forgot to clear the
    // dialog flag would leave the panel unable to auto-hide for the rest
    // of the session — no error, no log line, and a user who has to
    // restart the app. Every exit from a region selection funnels through
    // one function, and this is what keeps it that way.
    #[test]
    fn the_region_selection_is_cleaned_up_from_exactly_one_place() {
        let src = production_src();
        assert_eq!(
            src.matches("finish_region_selection(&app)").count(),
            1,
            "add cleanup to finish_region_selection, do not add a second call site"
        );
        assert_eq!(
            src.matches("set_dialog_active(false)").count(),
            1,
            "DIALOG_ACTIVE must be cleared from the one cleanup function"
        );
        assert_eq!(
            src.matches("overlay.hide()").count(),
            1,
            "the overlay is hidden from the one cleanup function"
        );
        // The `false` side was pinned from the start and the `true` side
        // was not, which made deleting the `true` a silent, green mutation:
        // the panel then auto-hides the moment the overlay takes focus,
        // losing the picker state mid-selection — exactly what the flag
        // exists to prevent.
        assert_eq!(
            src.matches("set_dialog_active(true)").count(),
            1,
            "DIALOG_ACTIVE must be set once, beside the slot claim, or the panel auto-hides under the overlay"
        );
        // Deleting this leaves the slot `Some` forever after any selection
        // that ended without an answer (the timeout path), so every later
        // one is refused for the life of the process — and the resolve path
        // hides it, because that already `take()`s the sender.
        assert_eq!(
            src.matches("RegionSelectionState>().0) = None").count(),
            1,
            "the answer slot must be dropped in the one cleanup function"
        );
    }

    // The refusal must RETURN before that one cleanup call ever runs.
    // Cleanup drops whatever sender the answer slot holds, so a second
    // call that fell through to it would cancel the selection it is
    // refusing to duplicate — and the first caller would see its own
    // channel disconnect and report a cancel it never made. Structural
    // because the property is an ORDERING inside a command that needs a
    // real AppHandle, which no CI runner here can build.
    #[test]
    fn a_second_selection_is_refused_before_the_cleanup_runs() {
        let src = production_src();
        let refusal = src
            .find("A region selection is already in progress.")
            .expect("a concurrent selection must be refused");
        let cleanup = src
            .find("finish_region_selection(&app)")
            .expect("the command must clean up on the way out");
        assert!(
            refusal < cleanup,
            "the concurrent-selection refusal must return before the cleanup call"
        );
    }
}
