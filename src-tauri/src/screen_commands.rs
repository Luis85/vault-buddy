//! Screen capture IPC (spec 11). Six of the increment's fifteen commands;
//! the rest belong to later phases.
//!
//! Shaped on `capture_commands.rs` deliberately — the same reservation
//! mutex, the same named worker owning the !Send cpal streams, the same
//! ready-handshake so a failed start reports an error instead of a phantom
//! capture, the same single emit-failed chokepoint, and (unlike the literal
//! sketch in the task brief — see the `ScreenCaptureState` doc comment
//! below for why) the same monitor thread draining the worker's outcome so
//! self-finalization clears the reservation exactly like an explicit stop.
//!
//! THREE deliberate differences from the audio domain:
//!
//! 1. No startup-wedged janitor. Audio needs one because a wedged driver can
//!    hang device setup past the handshake with a .part already on disk. The
//!    same is true here — `ScreenSession::start` blocks until the mux has
//!    OPENED the .part, so a timeout can fire with a file on disk — but a
//!    screen capture stages OUTSIDE every vault, so the orphan is a
//!    near-empty staged .mp4 in the app's own staging directory rather than
//!    something in the user's notes, and `screen_recovery` (Phase 5) sweeps
//!    it. What the timeout does NOT do any more is release the guard
//!    (GAP-110, closed): the device thread is by definition still alive
//!    there, so the release belongs to the outcome monitor, which is the
//!    only thing that learns it has ended. The reservation therefore
//!    outlives the failed start, which is why `bypasses_shutdown_wait`
//!    below exists — a wedged start with nothing on disk must not make the
//!    app unquittable (GAP-08).
//! 2. Mutual exclusion lives in `CaptureGuard`, claimed FIRST. The
//!    reservation below is defence in depth behind it, not the mechanism.
//! 3. A source closing mid-capture is a WARNING that finalizes cleanly
//!    (spec 14): a closed window must not lose the recording that preceded
//!    it. `ScreenSession`'s own frame callback already turns that into a
//!    stop signal (`frames.rs::on_closed`); this file's job is only to
//!    drain the resulting outcome and report it, never to treat it as a
//!    failure.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_capture::devices::DeviceSelection;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::session::Control;
use vault_buddy_screen::source;

use crate::capture_commands::{now_ms, toast};
use crate::capture_guard::{CaptureGuard, CaptureKind};
use crate::screen_dto::{ScreenStatusPayload, ScreenStopOutcomeDto, StagedCaptureDto};

/// A start that has not reported readiness within this long is treated as
/// wedged (difference 1 above) — release the guard and fail rather than
/// wait indefinitely.
pub(crate) const READY_TIMEOUT: Duration = Duration::from_secs(15);
/// Bound on `stop_screen_capture`'s own wait. Video finalize (mux STOP_GRACE,
/// the fMP4 fragment close, and the publish rename) can run longer than
/// audio's LAME flush, so this is wider than `stop_capture`'s 15 s.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ActiveScreenCapture {
    pub control_tx: Sender<Control>,
    pub vault_id: String,
    pub source_title: String,
    pub started_at_ms: u64,
    /// Pause bookkeeping mirrors the session (which owns the truth for the
    /// encoded timeline) so screen_capture_status can resync a reloaded
    /// webview's frozen-elapsed display exactly.
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    /// The `.part` the live session owns, once the worker has reserved it —
    /// `None` while the source is still being resolved and devices opened.
    pub part: Option<PathBuf>,
    /// Set when the 15 s ready handshake expired and this reservation was
    /// deliberately KEPT (GAP-110), so the device thread that is still
    /// opening devices cannot have `CaptureGuard` pulled out from under it.
    /// The capture never reported ready, so `capture_status` and friends go
    /// on describing it conservatively as running — but shutdown must not
    /// (see `bypasses_shutdown_wait`).
    pub startup_wedged: bool,
}

/// The mutex holds the active-capture reservation. Unlike the tuple struct
/// sketched in the task brief, this ALSO carries a `Condvar` (the
/// `CaptureState` shape from `capture_commands.rs`): the worker thread that
/// owns `done_rx` — the only place a finished `ScreenOutcome` is ever
/// produced — is a persistent monitor spawned once per capture, not the
/// `stop_screen_capture` command itself. That monitor is what lets a source
/// closing mid-capture (spec 14) finalize and clear the reservation even if
/// the user never calls stop; `stop_screen_capture` only needs to wait for
/// that clear, bounded, which is exactly what `CaptureState`'s condvar
/// already does for audio. Polling the mutex on a timer would work too, but
/// would either busy-poll or add latency to a stop the user is watching for.
#[derive(Default)]
pub struct ScreenCaptureState(pub Mutex<Option<ActiveScreenCapture>>, pub Condvar);

/// Build the device selection from the IPC arguments VERBATIM. These names
/// were ticked in front of the live device list; any normalization here
/// (trim, case-fold, dedupe) would stop them matching what cpal reports and
/// silently record nothing.
pub(crate) fn selection_from(inputs: Vec<String>, outputs: Vec<String>) -> DeviceSelection {
    DeviceSelection { inputs, outputs }
}

/// Every screen-capture failure surfaces through here — event AND toast —
/// so no path can log-and-vanish and leave the UI looking healthy.
/// `retained` carries the `.part` file's location for `ScreenError::Retained`
/// — a stop that failed AFTER real footage had already been written — so the
/// frontend can offer that file to the user directly instead of only reading
/// its path out of the message text (the module's fMP4 design exists
/// precisely so that file is still playable).
pub(crate) fn emit_screen_failed(app: &AppHandle, message: &str, retained: Option<&Path>) {
    log::error!("screen capture: failed: {message}");
    let _ = app.emit(
        "screen:failed",
        serde_json::json!({
            "message": message,
            "retainedPath": retained.map(|p| p.to_string_lossy().into_owned()),
        }),
    );
    toast(app, "Screen capture failed", message);
}

/// The stop notification's copy, split out so it can be asserted directly.
///
/// It deliberately does NOT say "saved", and that rule survives phase 5
/// unchanged: this toast fires when the capture is STAGED, which is before
/// any vault write exists for it. The wording started as a verbatim copy of
/// the audio domain's stop toast, where "saved" is true because the MP3 and
/// its companion note really are in the vault — the same words one domain
/// over send the user hunting through Obsidian and concluding the app lost
/// their recording.
///
/// **What DID change is the trailing sentence, and it had to.** Through
/// phases 2–4 it read "Editing and saving into a vault arrive in a later
/// update." Phase 4 shipped the editor and phase 5 shipped the save, so that
/// sentence became a false statement shown to every user at the end of every
/// recording — and it pointed them away from the button that now does the
/// thing. It names the capture bar instead, which is where **Edit** lives
/// (`ScreenCaptureBar` on the panel's list view). Keep any future edit
/// honest about what actually happened AND about what is now possible.
pub(crate) fn stopped_toast_copy(base: &str, warning: Option<&str>) -> (&'static str, String) {
    // The trailing sentence, shared by both arms, is what tells the user the
    // footage is not missing — only not in the vault YET — and where to go.
    let body = match warning {
        Some(w) => format!(
            "Recorded {base} with a warning: {w}. Edit it from the capture bar to \
             save it into a vault."
        ),
        None => {
            format!("Recorded {base}. Edit it from the capture bar to save it into a vault.")
        }
    };
    ("Screen capture ready", body)
}

pub(crate) fn emit_screen_stopped(app: &AppHandle, dto: &StagedCaptureDto, warning: Option<&str>) {
    let _ = app.emit("screen:stopped", dto);
    let (title, body) = stopped_toast_copy(&dto.base, warning);
    toast(app, title, &body);
}

/// THE chokepoint: drop the reservation and release the cross-domain claim
/// together, so no path can do one without the other. Every early-return
/// failure in `start_screen_capture_blocking` — including ones before the
/// reservation is even installed, where clearing it is a harmless no-op —
/// funnels through here too, so the guard is freed from exactly one place in
/// this file (see the structural test below).
pub(crate) fn clear_active_screen(app: &AppHandle) {
    let state = app.state::<ScreenCaptureState>();
    *lock_ignoring_poison(&state.0) = None;
    app.state::<CaptureGuard>().release(CaptureKind::Screen);
    clear_capture_window_effects(app);
    state.1.notify_all();
}

/// The window-visible half of a teardown: the buddy's capture exclusion and
/// the region border.
///
/// Split out of `clear_active_screen` for ONE caller (GAP-110's ready-timeout
/// arm) and no others. That arm deliberately KEEPS the reservation, because
/// the device thread may still be opening endpoints and freeing
/// `CaptureGuard` under it is the hazard spec 7.3 exists to prevent — but
/// keeping all FOUR of the chokepoint's effects would leave a click-through
/// border drawn over the user's screen with nothing recording, which
/// `region_indicator`'s own doc calls worse than no border at all, and which
/// they could not dismiss without quitting.
///
/// The seam is real rather than convenient: **the reservation tracks what the
/// DEVICES are doing, these track what the USER sees**, and a start that has
/// just been reported as failed is exactly the case where those diverge.
///
/// Both calls are idempotent — clearing an exclusion that was never applied
/// sets `WDA_NONE` on windows that already had it, and hiding a border that
/// was never raised is a no-op — so the monitor's later `clear_active_screen`
/// re-running them costs nothing. That is also why the structural tests in
/// `capture_exclusion.rs` still find exactly one raw call site each: it is
/// here, and `clear_active_screen` reaches it unconditionally.
pub(crate) fn clear_capture_window_effects(app: &AppHandle) {
    // Spec 5.3's exclusion is lifted HERE and nowhere else, for the same
    // reason the guard is: every teardown path funnels through
    // `clear_active_screen`, which calls this. Clearing an exclusion that
    // was never applied (a start that failed before the commit point) sets
    // WDA_NONE on windows that already had it, which is a no-op -- strictly
    // safer than a conditional that could be wrong in the other direction
    // and leave the user's windows hidden from every other app's
    // recordings.
    crate::capture_exclusion::clear(app);
    // Unconditional, like the exclusion clear above: hiding a border that
    // was never raised is a no-op, and the other direction strands one on
    // the user's desktop with nothing recording (GAP-165).
    crate::region_indicator::hide(app);
}

pub fn is_capturing(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<ScreenCaptureState>().0).is_some()
}

/// Whether shutdown/hide may skip waiting on this reservation: only a
/// start whose ready handshake timed out and that has nothing on disk.
///
/// Byte-for-byte the audio domain's rule
/// (`capture_commands::bypasses_shutdown_wait`), and for the same reason.
/// GAP-110's fix keeps the reservation past a ready timeout so the guard
/// stays claimed while the device thread may still be opening endpoints —
/// but a thread wedged in `open_selected_sources` never ends, and without
/// this exception that reservation would block quit, hide and the updater
/// for the rest of the process. That is GAP-08, which the audio domain
/// already paid for once. Nothing is on disk in that state, so nothing is
/// stranded by leaving.
///
/// A capture that DID reach ready keeps the wait-forever posture, because
/// its `.part` is real and an exit through it strands footage.
fn bypasses_shutdown_wait(active: &ActiveScreenCapture) -> bool {
    active.startup_wedged && active.part.is_none()
}

/// The buddy is the capture indicator for screen capture too, so a live
/// capture blocks hide and shutdown exactly as an audio recording does —
/// with the one scoped exception above.
pub fn capture_blocks_shutdown(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<ScreenCaptureState>().0)
        .as_ref()
        .is_some_and(|active| !bypasses_shutdown_wait(active))
}

fn our_window_titles(app: &AppHandle) -> Vec<String> {
    // Spec 7.2: Vault Buddy must never offer itself as a capture source.
    // `WebviewWindow::title()` is safe to call from the blocking-pool task
    // below, but NOT for the reason the obvious one: tauri marshals it, not
    // Win32. `title()` expands to `window_getter!` -> `send_user_message`,
    // which off the main thread posts a `Message::Window(_, Title(tx))` to
    // the tao event loop and then blocks on `rx.recv()`
    // (tauri-runtime-wry-2.11.4/src/lib.rs). That round-trip is UNBOUNDED:
    // while the main thread sits in an OS modal loop (a buddy drag, a native
    // file dialog) this call waits. Harmless only because the command that
    // reaches it is async; a future `sync` refactor of
    // `list_capture_sources` would hang the event loop on itself, which is
    // why this comment has to be accurate rather than merely reassuring.
    app.webview_windows()
        .values()
        .filter_map(|w| w.title().ok())
        .collect()
}

/// ASYNC: WGC/WinRT enumeration takes hundreds of ms (GAP-22's own reasoning
/// for `list_audio_devices`); on the main thread it would stall every window
/// operation. Enumeration itself is infallible — only the blocking-pool task
/// can fail, and that degrades to an empty list rather than an error,
/// matching `source::list_sources`'s own degrade-to-empty posture.
#[tauri::command]
pub async fn list_capture_sources(app: AppHandle) -> Vec<source::CaptureSourceInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        let titles = our_window_titles(&app);
        source::list_sources(&titles)
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("list_capture_sources: task failed: {e}");
        Vec::new()
    })
}

/// Whether `start_screen_capture`'s async tail may put the tray into its
/// recording state.
///
/// The tail runs after the blocking start returned, by which time the
/// `screen-capture-monitor` thread is already live — so spec 14's
/// self-finalize (the recorded window closes) can have run the whole
/// teardown first: `clear_active_screen` -> `screen:stopped` ->
/// `set_capture_state(Idle)`. Announcing `Recording` unconditionally after
/// that latches the tray into a phantom recording: it renders Pause/Stop and
/// DISABLES "Show / Hide", while Stop routes to the audio domain
/// (`menu_target(None)`) and only logs "No recording is running.", so nothing
/// clears it until a real capture starts and ends. The audio domain shares
/// the tail ordering but has no self-finalize path, which is why this is
/// newly reachable here. Reading the live reservation closes the window the
/// self-finalize actually opens; a teardown landing between this read and
/// the `set_capture_state` call is still possible in principle, but that is
/// a few instructions rather than a whole blocking start.
pub(crate) fn tray_state_after_start(
    still_capturing: bool,
) -> Option<crate::tray::TrayCaptureState> {
    still_capturing.then_some(crate::tray::TrayCaptureState::Recording)
}

/// ASYNC: device setup (source re-resolve, cpal endpoints), sink creation
/// and staging-directory I/O are all blocking work; on the main thread this
/// would stall window show/hide and drags for the whole handshake.
#[tauri::command]
pub async fn start_screen_capture(
    app: AppHandle,
    id: String,
    source_id: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
) -> Result<ScreenStatusPayload, String> {
    let worker = app.clone();
    let payload = tauri::async_runtime::spawn_blocking(move || {
        crate::screen_capture_worker::start_screen_capture_blocking(
            &worker, id, source_id, inputs, outputs,
        )
    })
    .await
    .map_err(|e| {
        log::warn!("start_screen_capture: task failed: {e}");
        "Screen capture start failed — see the logs for details.".to_string()
    })??;
    // Indicator hardening, mirroring start_capture: the recording buddy must
    // be visible. Best-effort — a failed show just loses the indicator, not
    // the capture.
    let shower = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = shower.get_webview_window("main") {
            let _ = window.show();
        }
    });
    if let Some(state) = tray_state_after_start(is_capturing(&app)) {
        crate::tray::set_capture_state(&app, state);
    }
    // The monitor thread is already live when the blocking start returns, so
    // a source that closes in the few ms this tail takes emits
    // `screen:stopped` BEFORE this `screen:started`. Matches the audio
    // precedent (capture_commands.rs emits `capture:started` the same way),
    // so it is left alone — but the frontend store must treat
    // `screen_capture_status` as authoritative and tolerate out-of-order
    // lifecycle events rather than deriving its state from arrival order.
    let _ = app.emit("screen:started", payload.clone());
    Ok(payload)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScreenStopWait {
    Cleared,
    TimedOut,
}

/// Send Stop and wait for the monitor thread to clear the reservation.
/// `wait: None` blocks forever — shutdown uses it, since the app must never
/// exit while a capture is still finalizing.
fn request_stop_and_wait_screen(app: &AppHandle, wait: Option<Duration>) -> ScreenStopWait {
    let capture_state = app.state::<ScreenCaptureState>();
    let mut guard = lock_ignoring_poison(&capture_state.0);
    let Some(active) = guard.as_ref() else {
        return ScreenStopWait::Cleared;
    };
    let _ = active.control_tx.send(Control::Stop);
    let deadline = wait.map(|limit| Instant::now() + limit);
    while guard.is_some() {
        match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    log::warn!("screen capture: stop wait timed out");
                    return ScreenStopWait::TimedOut;
                }
                guard = capture_state
                    .1
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            None => {
                let (g, timeout) = capture_state
                    .1
                    .wait_timeout(guard, Duration::from_secs(15))
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard = g;
                if timeout.timed_out() && guard.is_some() {
                    log::warn!("screen capture: still finalizing…");
                }
            }
        }
    }
    ScreenStopWait::Cleared
}

/// ASYNC: the wait is bounded at 30 s of mux teardown + fMP4 finalize + the
/// publish rename on a slow disk — freezing the main thread for that would
/// stall window show/hide and drags.
#[tauri::command]
pub async fn stop_screen_capture(app: AppHandle) -> Result<ScreenStopOutcomeDto, String> {
    if !is_capturing(&app) {
        return Err("No screen capture is running.".to_string());
    }
    let waiter = app.clone();
    let wait = tauri::async_runtime::spawn_blocking(move || {
        request_stop_and_wait_screen(&waiter, Some(STOP_TIMEOUT))
    })
    .await
    .map_err(|e| {
        log::warn!("stop_screen_capture: wait task failed: {e}");
        "Stop failed — see the logs for details.".to_string()
    })?;
    Ok(ScreenStopOutcomeDto {
        still_saving: wait == ScreenStopWait::TimedOut,
    })
}

/// Shared by the pause/resume commands. Errors are typed for the UI, but
/// every precondition re-checks here since a status read can always race a
/// concurrent stop.
fn set_screen_paused(app: &AppHandle, pause: bool) -> Result<(), String> {
    let state = app.state::<ScreenCaptureState>();
    let mut guard = lock_ignoring_poison(&state.0);
    let Some(active) = guard.as_mut() else {
        return Err("No screen capture is running.".to_string());
    };
    if active.part.is_none() {
        return Err("Screen capture is still starting.".to_string());
    }
    if pause == active.paused {
        return Err(if pause {
            "Screen capture is already paused."
        } else {
            "Screen capture is not paused."
        }
        .to_string());
    }
    let now = now_ms();
    if pause {
        active.paused = true;
        active.paused_since_ms = Some(now);
        let _ = active.control_tx.send(Control::Pause);
    } else {
        active.paused = false;
        active.paused_total_ms += now.saturating_sub(active.paused_since_ms.take().unwrap_or(now));
        let _ = active.control_tx.send(Control::Resume);
    }
    let paused_total_ms = active.paused_total_ms;
    drop(guard);
    if pause {
        let _ = app.emit("screen:paused", serde_json::json!({ "atMs": now }));
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Paused);
    } else {
        let _ = app.emit(
            "screen:resumed",
            serde_json::json!({ "pausedTotalMs": paused_total_ms }),
        );
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Recording);
    }
    Ok(())
}

#[tauri::command]
pub fn pause_screen_capture(app: AppHandle) -> Result<(), String> {
    set_screen_paused(&app, true)
}

#[tauri::command]
pub fn resume_screen_capture(app: AppHandle) -> Result<(), String> {
    set_screen_paused(&app, false)
}

#[tauri::command]
pub fn screen_capture_status(state: tauri::State<ScreenCaptureState>) -> ScreenStatusPayload {
    let guard = lock_ignoring_poison(&state.0);
    match guard.as_ref() {
        Some(active) => ScreenStatusPayload {
            capturing: true,
            vault_id: Some(active.vault_id.clone()),
            started_at_ms: Some(active.started_at_ms),
            paused: active.paused,
            paused_total_ms: active.paused_total_ms,
            paused_since_ms: active.paused_since_ms,
            // Empty until the ready handshake lands (source not yet
            // resolved) — reported as absent rather than a blank chip.
            source_title: Some(active.source_title.clone()).filter(|s| !s.is_empty()),
        },
        None => ScreenStatusPayload::idle(),
    }
}

/// The tray's capture controls, routed here by `tray::menu_target` when a
/// SCREEN capture holds the cross-domain guard. Bounded like the command
/// (the tray is not a shutdown path); callers must NOT be on the main
/// thread — `tray.rs` spawns `tray-stop` for exactly that reason.
pub fn stop_from_menu(app: &AppHandle) {
    let _ = request_stop_and_wait_screen(app, Some(STOP_TIMEOUT));
}

/// Pause/resume from the tray. Safe on the main thread: `set_screen_paused`
/// holds the state mutex for O(1) work and sends on an unbounded channel.
/// The error is logged, not surfaced — the tray has no panel to show it in,
/// mirroring `capture_commands::pause_from_menu`.
pub fn pause_from_menu(app: &AppHandle) {
    if let Err(e) = set_screen_paused(app, true) {
        log::warn!("pause screen capture from tray: {e}");
    }
}

pub fn resume_from_menu(app: &AppHandle) {
    if let Err(e) = set_screen_paused(app, false) {
        log::warn!("resume screen capture from tray: {e}");
    }
}

/// Every shutdown path funnels through here so quitting mid-capture saves
/// through the normal stop flow instead of stranding a `.part`. Callers must
/// NOT be on the main/event-loop thread (the wait is unbounded); tray::quit
/// and the CloseRequested handler spawn a worker thread for it — mirrors
/// `capture_commands::finalize_if_recording` exactly.
pub fn finalize_if_capturing(app: &AppHandle) {
    if is_capturing(app) {
        log::info!("screen capture: finalizing an active capture before shutdown");
        let _ = request_stop_and_wait_screen(app, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reservation(startup_wedged: bool, part: Option<PathBuf>) -> ActiveScreenCapture {
        let (control_tx, _rx) = std::sync::mpsc::channel::<Control>();
        ActiveScreenCapture {
            control_tx,
            vault_id: "vault-1".to_string(),
            source_title: "Demo".to_string(),
            started_at_ms: 0,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            part,
            startup_wedged,
        }
    }

    // GAP-110's fix keeps the reservation past a ready timeout so the guard
    // stays claimed. GAP-08 is the bill that comes with it: a device thread
    // wedged in `open_selected_sources` never ends, so that reservation is
    // never cleared, and a shutdown predicate reading it alone would refuse
    // quit, hide AND the updater for the rest of the process. Only the
    // wedged-with-nothing-on-disk case is exempt, because only that case
    // strands nothing by leaving.
    #[test]
    fn only_a_wedged_start_with_nothing_on_disk_skips_the_shutdown_wait() {
        assert!(
            bypasses_shutdown_wait(&reservation(true, None)),
            "a wedged start with no .part strands nothing, so it must not \
             make the app unquittable (GAP-08)"
        );
        assert!(
            !bypasses_shutdown_wait(&reservation(false, None)),
            "an ordinary start that has not reserved its .part YET is still \
             on its way to one; it keeps the wait-forever posture"
        );
        assert!(
            !bypasses_shutdown_wait(&reservation(true, Some(PathBuf::from("/s/.a.mp4.part")))),
            "a wedged start that DID open a .part strands real footage on \
             the way out -- being wedged is not enough on its own"
        );
        assert!(
            !bypasses_shutdown_wait(&reservation(false, Some(PathBuf::from("/s/.a.mp4.part")))),
            "a live capture always blocks shutdown"
        );
    }

    #[test]
    fn the_stop_notification_does_not_claim_a_save_that_did_not_happen() {
        // A STOP is not a save, in any phase. The capture stages in
        // `%LOCALAPPDATA%\\com.vaultbuddy.desktop\\screen-captures`; the ninth
        // sanctioned vault write happens later, when the user presses Save
        // in the editor. This copy was lifted verbatim from the audio
        // domain, where "saved" is true because the MP3 and its note really
        // are in the vault. Here it sends the user hunting through their
        // vault for a file that was never put there, and concluding the app
        // lost a ten-minute recording.
        //
        // The trailing sentence is pinned for the opposite reason. It used
        // to promise that "editing and saving into a vault arrive in a later
        // update"; phases 4 and 5 shipped both, so that sentence became a
        // falsehood shown at the end of every recording AND pointed the user
        // away from the button that does the thing.
        let (title, body) = stopped_toast_copy("2026-09-19 1432 Figma", None);
        assert_eq!(title, "Screen capture ready");
        assert_eq!(
            body,
            "Recorded 2026-09-19 1432 Figma. Edit it from the capture bar to \
             save it into a vault."
        );

        // The warning arm keeps its meaning — a capture that finalized with
        // a vanished source or device still has to say so — but must not
        // smuggle the same false claim back in.
        let (title, body) = stopped_toast_copy("2026-09-19 1432 Figma", Some("a device vanished"));
        assert_eq!(title, "Screen capture ready");
        assert_eq!(
            body,
            "Recorded 2026-09-19 1432 Figma with a warning: a device vanished. \
             Edit it from the capture bar to save it into a vault."
        );

        // Belt for a future copy edit: whatever the wording becomes, it may
        // not assert a save, because at STOP time nothing is in the vault
        // yet whatever phase we are in.
        for w in [None, Some("a device vanished")] {
            let (title, body) = stopped_toast_copy("base", w);
            assert!(!title.to_lowercase().contains("saved"), "title: {title}");
            assert!(!body.to_lowercase().contains("saved"), "body: {body}");
        }
    }

    #[test]
    fn the_device_selection_is_built_from_the_ipc_arguments_verbatim() {
        // These names were ticked in front of the live device list. Any
        // normalization here (trimming, casing, dedupe) would stop them
        // matching what cpal reports and silently record nothing.
        let sel = selection_from(
            vec!["Microphone (Yeti)".to_string()],
            vec!["Speakers (Realtek)".to_string()],
        );
        assert_eq!(sel.inputs, vec!["Microphone (Yeti)".to_string()]);
        assert_eq!(sel.outputs, vec!["Speakers (Realtek)".to_string()]);
    }

    #[test]
    fn a_self_finalized_capture_does_not_latch_the_tray_into_a_phantom_recording() {
        // `start_screen_capture`'s async tail runs AFTER the blocking start
        // returned, and the `screen-capture-monitor` thread is already live
        // by then. Spec 14's self-finalize (the recorded window closes) is
        // unique to this domain — the audio path has no such route — so the
        // monitor can run `clear_active_screen` -> `screen:stopped` ->
        // `set_capture_state(Idle)` and only THEN does the tail land. An
        // unconditional `Recording` there leaves the tray rendering
        // "Pause recording" / "Stop recording" and DISABLING "Show / Hide"
        // with nothing running; Stop routes to the audio domain and only
        // logs "No recording is running.", so nothing clears it until a real
        // capture starts and ends.
        assert!(
            tray_state_after_start(false).is_none(),
            "the tail must not announce a recording the capture no longer has"
        );
        assert!(
            matches!(
                tray_state_after_start(true),
                Some(crate::tray::TrayCaptureState::Recording)
            ),
            "a capture that IS still claimed must still light the tray"
        );
    }

    #[test]
    fn the_tray_tail_asks_whether_the_capture_is_still_claimed() {
        // Structural, because the property is the WIRING: the helper above
        // is only worth anything if the tail feeds it the live reservation
        // rather than a constant. `is_capturing` is the one reader of that
        // reservation, and `clear_active_screen` (the sole release site)
        // clears it before the monitor touches the tray, so it is exactly
        // the question the tail has to ask.
        let src = include_str!("screen_commands.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert_eq!(
            production
                .matches("tray_state_after_start(is_capturing(&app))")
                .count(),
            1,
            "the start tail must gate the tray state on the live reservation"
        );
        // The exact shape the bug had. `pause_from_menu` / `resume_from_menu`
        // also write the tray state, but they take `app: &AppHandle` and run
        // only while the reservation is held, so they are spelled `(app, ..)`
        // and are not what this scan is about — restoring the start tail's
        // unconditional write is.
        assert_eq!(
            production
                .matches("set_capture_state(&app, crate::tray::TrayCaptureState::Recording)")
                .count(),
            0,
            "an ungated tray write in the start tail reopens the phantom-recording window"
        );
    }

    #[test]
    fn an_unparseable_source_id_is_refused_before_anything_is_claimed() {
        // Structural, because the property IS an ordering, not a value: the
        // id crosses the IPC boundary untrusted, and `SourceId::parse` must
        // run BEFORE `CaptureGuard::try_claim`. Asserting `parse("nonsense")
        // .is_none()` instead would only re-test Task 5's parser and would
        // stay green with the claim moved in front of it.
        let src = include_str!("screen_capture_worker.rs");
        let parse = src
            .find("SourceId::parse")
            .expect("the start path must parse the source id");
        let claim = src
            .find("try_claim(CaptureKind::Screen)")
            .expect("the start path must claim the cross-domain guard");
        assert!(
            parse < claim,
            "the source id must be validated BEFORE the guard is claimed: a malformed id \
             that claimed first and returned on `?` would leak the claim and wedge BOTH \
             capture domains until the app restarts, with no error and no log line"
        );
    }

    // Structural regression: the guard must be released from exactly one
    // place in the WHOLE shell crate. A second release site can free a
    // claim on a path that never made one, letting a capture start on top
    // of a live one; a missing one wedges both domains until restart. Both
    // failures are silent -- no error, no log line.
    //
    // This scanned TWO named files (`screen_commands.rs` and
    // `screen_capture_worker.rs`) until a rogue
    // `app.state::<CaptureGuard>().release(CaptureKind::Screen)` injected
    // into `discard_staged_capture` left the entire shell suite green: 185
    // passed, 0 failed. Eight modules that can reach the guard were
    // unguarded -- `staged_commands`, `region_commands`, `editor_commands`,
    // `screen_recovery`, `tray`, `window_close`, `lib`, `commands`. A scan
    // is worth exactly what its file set covers, so the file set now comes
    // from `structural_scan::shell_sources`, which walks the tree and
    // self-checks for vacuity. `capture_exclusion.rs` had already learned
    // this lesson once, for the same reason (the lifecycle split into
    // `screen_capture_worker.rs`); the two pins now share one walk.
    //
    // Test halves, comments and string literals are stripped, so neither
    // this comment nor the assertions below can satisfy themselves, and the
    // four module docs that QUOTE the invariant are not counted as calls.
    #[test]
    fn the_screen_guard_is_released_only_from_the_clear_chokepoint() {
        let releases = crate::structural_scan::call_args(".release(");
        assert!(
            releases
                .iter()
                .all(|(_, arg)| arg.contains("Screen") || arg.contains("Audio")),
            "a CaptureGuard release whose kind this scan cannot read: {releases:?}. \
             Pass the variant literally, or these pins stop classifying it."
        );
        let sites: Vec<&String> = releases
            .iter()
            .filter(|(_, arg)| arg.contains("Screen"))
            .map(|(file, _)| file)
            .collect();
        assert_eq!(
            sites.len(),
            1,
            "expected exactly one Screen release in the whole shell crate \
             (clear_active_screen); found {sites:?}. Funnel it through \
             clear_active_screen instead."
        );
        assert!(
            sites[0].ends_with("screen_commands.rs"),
            "the release must live in clear_active_screen -- the single chokepoint every \
             screen-capture teardown already funnels through -- but it is in {}",
            sites[0]
        );

        // And in the right FUNCTION of that file: moved into
        // `stop_screen_capture` it is skipped entirely by spec 14's
        // self-finalize, which never goes through that command, and the
        // count above would not move.
        let production =
            crate::structural_scan::production_code(include_str!("screen_commands.rs"));
        let chokepoint = crate::structural_scan::offset_of(&production, "fn clear_active_screen(");
        let release =
            crate::structural_scan::offset_of(&production, ".release(CaptureKind::Screen)");
        let after = crate::structural_scan::offset_of(&production, "\npub fn is_capturing(");
        assert!(
            chokepoint < release && release < after,
            "the release must sit INSIDE clear_active_screen"
        );
    }
}
