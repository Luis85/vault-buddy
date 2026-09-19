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
//!    hang device setup past the handshake with a .part already on disk.
//!    Here the sink is created only once `ScreenSession::start` succeeds, so
//!    a hang before the handshake has produced no file: the start fails
//!    cleanly and releases the guard instead of keeping a reservation
//!    nothing would ever clear on its own.
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenStatusPayload {
    pub capturing: bool,
    pub vault_id: Option<String>,
    pub started_at_ms: Option<u64>,
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    pub source_title: Option<String>,
}

impl ScreenStatusPayload {
    /// Every field cleared. A reloaded webview re-reads the status; leaking
    /// the last capture's vault id or start time would render a phantom
    /// capture bar counting up from a recording that already ended.
    pub fn idle() -> ScreenStatusPayload {
        ScreenStatusPayload {
            capturing: false,
            vault_id: None,
            started_at_ms: None,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            source_title: None,
        }
    }
}

/// What the editor/capture-bar needs about a just-finished capture. `path`
/// is the staged `.mp4` inside the (outside-every-vault) staging directory —
/// nothing here is a vault write yet.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCaptureDto {
    pub base: String,
    pub path: String,
    pub duration_ms: u64,
    pub source_title: String,
    pub width: u32,
    pub height: u32,
}

/// Wire result for `stop_screen_capture`, mirroring `StopOutcomeDto`:
/// `still_saving` = the bounded wait expired while finalize was still
/// running, so the frontend keeps its saving UI and lets `screen:stopped` /
/// `screen:failed` finish the story instead of reporting a false success.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenStopOutcomeDto {
    pub still_saving: bool,
}

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

pub(crate) fn emit_screen_stopped(app: &AppHandle, dto: &StagedCaptureDto, warning: Option<&str>) {
    let _ = app.emit("screen:stopped", dto);
    let body = match warning {
        Some(w) => format!("Saved with a warning: {w}"),
        None => format!("Saved {}", dto.base),
    };
    toast(app, "Screen capture saved", &body);
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
    state.1.notify_all();
}

pub fn is_capturing(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<ScreenCaptureState>().0).is_some()
}

/// The buddy is the capture indicator for screen capture too, so a live
/// capture blocks hide and shutdown exactly as an audio recording does.
pub fn capture_blocks_shutdown(app: &AppHandle) -> bool {
    is_capturing(app)
}

fn our_window_titles(app: &AppHandle) -> Vec<String> {
    // Spec 7.2: Vault Buddy must never offer itself as a capture source.
    // `WebviewWindow::title()` reads the OS window text (Win32 marshals a
    // cross-thread GetWindowText itself), unlike `show`/`hide`/`set_position`
    // which the window-system invariant reserves for the main thread — this
    // is safe to call from the blocking-pool task below.
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
    crate::tray::set_capture_state(&app, crate::tray::TrayCaptureState::Recording);
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

    #[test]
    fn the_status_payload_serializes_camel_case_for_the_webview() {
        let payload = ScreenStatusPayload {
            capturing: true,
            vault_id: Some("v1".into()),
            started_at_ms: Some(1_700_000_000_000),
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            source_title: Some("Screen 1".into()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"startedAtMs\""), "got {json}");
        assert!(json.contains("\"sourceTitle\""), "got {json}");
        assert!(!json.contains("started_at_ms"), "got {json}");
    }

    #[test]
    fn an_idle_status_payload_reports_nothing_rather_than_stale_values() {
        // A reloaded webview re-reads this. Leaking the last capture's vault
        // id or start time into an idle payload would render a phantom
        // capture bar counting up from a recording that ended.
        let p = ScreenStatusPayload::idle();
        assert!(!p.capturing);
        assert_eq!(p.vault_id, None);
        assert_eq!(p.started_at_ms, None);
        assert_eq!(p.source_title, None);
        assert_eq!(p.paused_total_ms, 0);
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
    fn an_unparseable_source_id_is_refused_before_anything_is_claimed() {
        // The id crosses the IPC boundary and is untrusted. Refusing it here,
        // before the guard is claimed, means a malformed request cannot
        // wedge both capture domains.
        assert!(vault_buddy_screen::source::SourceId::parse("nonsense").is_none());
    }

    // Structural regression, mirroring config_lock_guard.rs: the guard must
    // be released from exactly one place in this file. A second release site
    // can free a claim on a path that never made one, letting a capture
    // start on top of a live one; a missing one wedges both domains until
    // restart, with no error and no log line.
    #[test]
    fn the_screen_guard_is_released_only_from_the_clear_chokepoint() {
        // include_str! pulls in this ENTIRE file, including this assertion's
        // own string literal — scanning the whole thing would always find
        // one more "release(CaptureKind::Screen)" than production actually
        // contains (this very line). config_lock_guard.rs's cross-file scan
        // doesn't hit this because it reads a DIFFERENT file than the one
        // holding the test; here the test and the code it checks are the
        // same file, so only the portion BEFORE the trailing `#[cfg(test)]`
        // module — this file's one and only test module, always last — is
        // scanned. That is production code alone, which is what the
        // invariant is actually about.
        let src = include_str!("screen_commands.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        let releases = production.matches("release(CaptureKind::Screen)").count();
        assert_eq!(
            releases, 1,
            "expected exactly one Screen release (clear_active_screen); found {releases}"
        );
    }
}
