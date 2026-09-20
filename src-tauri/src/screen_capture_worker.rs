//! The screen-capture start path's lifecycle worker, split out of
//! `screen_commands.rs` to keep that file under the LOC cap (task brief
//! step 5): source re-resolution, device setup, the ready handshake, the
//! device thread that owns the `!Send` cpal streams, and the persistent
//! monitor thread that drains the worker's outcome — an explicit stop OR
//! self-finalization (the source closing mid-capture, spec 14) — and clears
//! the reservation no matter which one happened.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_capture::devices::open_selected_sources;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths};
use vault_buddy_screen::session::{
    Control, FrameStats, ScreenOutcome, ScreenSession, ScreenSessionParams,
};
use vault_buddy_screen::{source, staging, ScreenError};

use crate::capture_commands::now_ms;
use crate::capture_guard::{CaptureGuard, CaptureKind};

use crate::screen_commands::{
    clear_active_screen, emit_screen_failed, emit_screen_stopped, selection_from,
    ActiveScreenCapture, ScreenCaptureState, ScreenStatusPayload, StagedCaptureDto, READY_TIMEOUT,
};

fn source_kind_str(id: &source::SourceId) -> &'static str {
    match id.kind() {
        source::SourceKind::Screen => "screen",
        source::SourceKind::Window => "window",
        source::SourceKind::Region => "region",
    }
}

/// Everything a finished worker hands to the monitor thread: the outcome
/// itself, plus what only the worker knew (the re-resolved source's title —
/// which may differ from the picker's stale copy — and the audio devices
/// that actually opened) and can no longer be read off the reservation once
/// it clears.
struct ScreenStopBundle {
    outcome: ScreenOutcome,
    source_title: String,
    source_kind: &'static str,
    recorded_at: String,
    inputs: Vec<String>,
}

/// What the device thread hands back over `ready_tx` on success.
struct ReadyInfo {
    part: PathBuf,
    title: String,
}

pub(crate) fn start_screen_capture_blocking(
    app: &AppHandle,
    id: String,
    source_id: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
) -> Result<ScreenStatusPayload, String> {
    // The id crosses the IPC boundary untrusted. Refusing an unparseable one
    // here, before ANYTHING is claimed, means a malformed request cannot
    // wedge either capture domain.
    let parsed =
        source::SourceId::parse(&source_id).ok_or_else(|| "Unknown capture source.".to_string())?;

    // Mutual exclusion across BOTH capture domains (spec 7.3), claimed
    // BEFORE any of the fallible-but-cheap setup below — difference (2) from
    // the audio domain's ordering, so every subsequent early return in this
    // function must release what was just claimed, which is why they all go
    // through `clear_active_screen` rather than a raw release call.
    if let Err(held) = app.state::<CaptureGuard>().try_claim(CaptureKind::Screen) {
        return Err(held.busy_message());
    }

    let vault = match crate::commands::find_vault(&id) {
        Ok(v) => v,
        Err(e) => {
            clear_active_screen(app);
            return Err(e);
        }
    };
    if !PathBuf::from(&vault.path).is_dir() {
        clear_active_screen(app);
        return Err("Vault folder not found — was it moved or deleted?".to_string());
    }
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);

    let local_dir = match app.path().app_local_data_dir() {
        Ok(d) => d,
        Err(e) => {
            clear_active_screen(app);
            return Err(format!("Could not resolve the app data folder: {e}"));
        }
    };
    let dir = staging::staging_dir(&local_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        clear_active_screen(app);
        return Err(format!("Cannot create the staging folder: {e}"));
    }

    let sel = selection_from(inputs, outputs);
    let (control_tx, control_rx) = mpsc::channel::<Control>();
    let (done_tx, done_rx) = mpsc::channel::<Result<ScreenStopBundle, ScreenError>>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<ReadyInfo, String>>();

    // Reserve the state up front: the lock is held only for the is-running
    // check plus the insert, which closes the double-start window without
    // serializing device setup (or any I/O) under the mutex.
    let state = app.state::<ScreenCaptureState>();
    {
        let mut guard = lock_ignoring_poison(&state.0);
        if guard.is_some() {
            drop(guard);
            // Structurally unreachable: CaptureGuard::try_claim above
            // already serializes every screen-capture start, so a second
            // caller can never reach this branch while a live reservation
            // exists — but CaptureState's own audio-domain double-start
            // check has the same shape, kept as defence in depth. Freed
            // through the shared chokepoint (rather than a raw guard
            // release, the audio precedent) so screen_commands.rs keeps
            // exactly one release site.
            clear_active_screen(app);
            return Err("A screen capture is already running.".to_string());
        }
        *guard = Some(ActiveScreenCapture {
            control_tx: control_tx.clone(),
            vault_id: id.clone(),
            source_title: String::new(),
            started_at_ms: now_ms(),
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            part: None,
        });
    }

    // Spec 5.3: from here on the capture is committed, and every exit --
    // including every failure below -- goes through `clear_active_screen`,
    // which is the one place the exclusion is lifted. Applying it before
    // the session opens means the first frames are already clean; it is
    // fire-and-forget on the main thread, so a busy event loop can still
    // let a frame or two of buddy through (docs/Gaps.md, recorded in
    // task 8) rather than delaying the start.
    crate::capture_exclusion::apply(app);

    // Live source-loss / device warnings: forwarded to the panel while
    // capturing (spec 14's screen:warning).
    let (warn_tx, warn_rx) = mpsc::channel::<String>();
    let app_warn = app.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("screen-warn".into())
        .spawn(move || {
            while let Ok(message) = warn_rx.recv() {
                let _ = app_warn.emit("screen:warning", serde_json::json!({ "message": message }));
            }
        })
    {
        // Capture proceeds without live warning forwarding; sends into the
        // dropped receiver are already fire-and-forget.
        log::warn!("could not spawn screen-warn thread: {e}");
    }

    // Advisory frame stats (~2 Hz, lossy by design): forwarded as
    // screen:frames for the capture bar.
    let (stats_tx, stats_rx) = mpsc::channel::<FrameStats>();
    let app_stats = app.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("screen-stats".into())
        .spawn(move || {
            while let Ok(stats) = stats_rx.recv() {
                let _ = app_stats.emit(
                    "screen:frames",
                    serde_json::json!({ "fps": stats.fps, "dropped": stats.dropped }),
                );
            }
        })
    {
        log::warn!("could not spawn screen-stats thread: {e}");
    }

    let fps = cfg.screen_fps;
    let quality = cfg.screen_quality;
    let device_thread = std::thread::Builder::new()
        .name("screen-capture-device".into())
        .spawn(move || {
            // Re-check the picked source AT START TIME (spec 14): the list
            // the user clicked may be seconds stale.
            let resolved = match source::resolve(&parsed) {
                Ok(r) => r,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            let open = match open_selected_sources(&sel) {
                Ok(o) => o,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            for w in &open.warnings {
                let _ = warn_tx.send(w.clone());
            }

            let title = resolved.title.clone();
            let now = chrono::Local::now();
            use chrono::Timelike;
            let candidate = capture_paths::base_name(
                now.date_naive(),
                now.hour(),
                now.minute(),
                &staging::sanitize_title(&title),
            );
            let base = staging::reserve_base(&dir, &candidate);
            let part = dir.join(staging::part_file_name(&base));
            let staged = dir.join(staging::mp4_file_name(&base));
            let input_names: Vec<String> = open.inputs.iter().map(|s| s.name.clone()).collect();
            let source_kind = source_kind_str(&parsed);
            let recorded_at = now.to_rfc3339();

            let params = ScreenSessionParams {
                source: resolved,
                part: part.clone(),
                staged,
                fps,
                quality,
                audio: open.inputs,
                warn_tx: Some(warn_tx),
                stats_tx: Some(stats_tx),
            };
            let session = match ScreenSession::start(params) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            log::info!("screen capture: started ({title}) -> {}", part.display());
            let _ = ready_tx.send(Ok(ReadyInfo {
                part: part.clone(),
                title: title.clone(),
            }));

            // Own the streams here (they are !Send); poll for control or
            // self-finalization (the session's own on_closed sets its
            // stopping flag when the source disappears — see the module doc
            // comment's difference 3).
            let streams = open.streams;
            loop {
                match control_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Control::Pause) => session.pause(),
                    Ok(Control::Resume) => session.resume(),
                    Err(RecvTimeoutError::Timeout) => {
                        if !session.is_running() {
                            break;
                        }
                    }
                }
            }
            // Stop the session while the streams are still alive, mirroring
            // the audio domain's own ordering: dropping them first would
            // disconnect every source channel before the session asks for
            // its own pause flush.
            let outcome = session.stop();
            drop(streams);
            let _ = done_tx.send(outcome.map(|o| ScreenStopBundle {
                outcome: o,
                source_title: title,
                source_kind,
                recorded_at,
                inputs: input_names,
            }));
        });
    if let Err(e) = device_thread {
        // Without the worker there IS no capture: nothing has touched disk
        // yet, so fail the start cleanly and drop the reservation.
        clear_active_screen(app);
        let msg = format!("Could not start the screen capture worker: {e}");
        emit_screen_failed(app, &msg, None);
        return Err(msg);
    }

    let (started_at_ms, source_title) = match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(info)) => {
            let started_at_ms = now_ms();
            if let Some(active) = lock_ignoring_poison(&state.0).as_mut() {
                active.part = Some(info.part);
                active.source_title = info.title.clone();
                active.started_at_ms = started_at_ms;
            }
            (started_at_ms, info.title)
        }
        Ok(Err(e)) => {
            clear_active_screen(app);
            emit_screen_failed(app, &e, None);
            return Err(e);
        }
        Err(_) => {
            // Brief-mandated: release the guard and fail cleanly rather than
            // keep a reservation nothing will ever clear on its own. Be
            // clear about what that COSTS, because the obvious justification
            // is false: `run_mux` opens the .part BEFORE it signals ready,
            // and ScreenSession::start blocks on that handshake
            // (screen/src/session/windows_session.rs), so the sink exists
            // strictly before `start` returns — a timeout here can fire with
            // an open .part on disk. The likelier shape is a device thread
            // still inside open_selected_sources (a wedged audio driver);
            // when it later succeeds it sees the pre-emptive Stop below,
            // finalizes, and renames to a staged .mp4 whose done_tx.send
            // lands in a dropped receiver — so it publishes with no sidecar
            // and no event, and no janitor sweeps the staging dir (Phase 5
            // owns staging recovery). The guard is also free for that whole
            // stretch, so a retry can open the same audio endpoint beside
            // the first capture. Both residuals: docs/Gaps.md GAP-110.
            clear_active_screen(app);
            let _ = control_tx.send(Control::Stop);
            let msg = "Screen capture did not start in time.".to_string();
            emit_screen_failed(app, &msg, None);
            return Err(msg);
        }
    };

    // The ONLY consumer of the worker's outcome — covers an explicit stop
    // AND self-finalization (the source closing mid-capture), so the
    // reservation clears and screen:stopped/failed fires no matter who
    // ended the capture (mirrors capture_commands.rs's capture-monitor).
    let monitor_app = app.clone();
    let monitor_vault_id = id.clone();
    let monitor = std::thread::Builder::new()
        .name("screen-capture-monitor".into())
        .spawn(move || {
            let result = done_rx.recv().unwrap_or_else(|_| {
                Err(ScreenError::Sink("screen capture thread vanished".into()))
            });
            // Clear FIRST, then announce — the ordering capture_commands.rs's
            // capture-monitor uses. The device thread has already finalized
            // and published the .mp4 by the time `done_rx` yields, so the
            // reservation is protecting nothing from here on; holding it
            // across the sidecar write and the OS toast would let a listener
            // reacting to `screen:stopped` read `capturing: true`, and would
            // put that disk I/O inside stop_screen_capture's 30 s budget so
            // a slow disk reports `stillSaving` on a fully-saved capture.
            clear_active_screen(&monitor_app);
            match result {
                Ok(bundle) => {
                    let (dto, warning) = finalize_stopped(&monitor_vault_id, bundle);
                    emit_screen_stopped(&monitor_app, &dto, warning.as_deref());
                }
                Err(e) => {
                    let (message, retained) = describe_screen_error(&e);
                    emit_screen_failed(&monitor_app, &message, retained.as_deref());
                }
            }
            crate::tray::set_capture_state(&monitor_app, crate::tray::TrayCaptureState::Idle);
        });
    if let Err(e) = monitor {
        // Without a monitor nothing would ever drain the outcome or clear
        // the reservation. Stop the session — the device thread still
        // finalizes and the file reaches disk (its done_tx send is a no-op
        // into the dropped receiver) — and report the start as failed.
        let _ = control_tx.send(Control::Stop);
        clear_active_screen(app);
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Idle);
        let msg = format!("Screen capture could not be monitored; stopping: {e}");
        emit_screen_failed(app, &msg, None);
        return Err(msg);
    }

    Ok(ScreenStatusPayload {
        capturing: true,
        vault_id: Some(id),
        started_at_ms: Some(started_at_ms),
        paused: false,
        paused_total_ms: 0,
        paused_since_ms: None,
        source_title: Some(source_title),
    })
}

/// `Retained` carries the `.part` file's path as typed data; everything else
/// renders its own (fixed or interpolated) message with nothing left over.
fn describe_screen_error(err: &ScreenError) -> (String, Option<PathBuf>) {
    match err {
        ScreenError::Retained { path, .. } => (err.to_string(), Some(path.clone())),
        other => (other.to_string(), None),
    }
}

/// Write the staging sidecar and build the DTO the frontend renders. A
/// sidecar write failure is logged but does not fail the finalize — the
/// capture itself already landed on disk under `outcome.mp4`; losing the
/// resume metadata is a lesser, separately-visible problem than reporting a
/// safely-saved capture as failed. INFALLIBLE by construction, and typed
/// that way: the only fallible call is the sidecar write above, which is
/// deliberately swallowed, so a `Result` here would give the monitor an
/// error arm nothing can ever reach.
fn finalize_stopped(
    vault_id: &str,
    bundle: ScreenStopBundle,
) -> (StagedCaptureDto, Option<String>) {
    let ScreenStopBundle {
        outcome,
        source_title,
        source_kind,
        recorded_at,
        inputs,
    } = bundle;
    let base = outcome
        .mp4
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = outcome.mp4.parent().unwrap_or_else(|| Path::new("."));
    let sidecar = staging::StagedSidecar {
        base: base.clone(),
        vault_id: vault_id.to_string(),
        source_title: source_title.clone(),
        source_kind: source_kind.to_string(),
        inputs,
        duration_ms: outcome.duration_ms,
        paused_ms: outcome.paused_ms,
        width: outcome.width,
        height: outcome.height,
        recorded_at,
        timeline: None,
    };
    if let Err(e) = staging::write_sidecar(dir, &sidecar) {
        log::warn!("screen capture: writing the sidecar failed: {e}");
    }
    (
        StagedCaptureDto {
            base,
            path: outcome.mp4.to_string_lossy().into_owned(),
            duration_ms: outcome.duration_ms,
            source_title,
            width: outcome.width,
            height: outcome.height,
        },
        outcome.warning,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retained_error_hands_back_the_part_path_as_typed_data() {
        // Task 7 introduced `Retained` precisely so a stop that failed AFTER
        // real footage was written hands the surviving `.part` back as DATA,
        // not as prose the frontend would have to parse out of a message.
        // Folding this arm into the catch-all below is a one-character edit
        // that silently drops `retainedPath` from the emitted payload.
        let err = ScreenError::Retained {
            path: PathBuf::from("/staging/.2026-01-02 0915 Demo.mp4.part"),
            holds_footage: true,
            cause: Box::new(ScreenError::Sink("disk full".into())),
        };
        let (message, retained) = describe_screen_error(&err);
        assert_eq!(
            retained.as_deref(),
            Some(Path::new("/staging/.2026-01-02 0915 Demo.mp4.part"))
        );
        // The message must still name the cause, so a listener that only
        // renders `message` does not regress to "something went wrong".
        assert!(message.contains("disk full"), "got {message}");
    }

    #[test]
    fn a_retained_file_with_no_footage_is_still_surfaced_but_not_called_a_recording() {
        // A capture whose frames were all the wrong size leaves a `.part`
        // with no video in it. The path is still handed back — suppressing
        // it would leave a permanent invisible orphan, since nothing sweeps
        // staging (docs/Gaps.md GAP-115) — but the MESSAGE must not tell the
        // user their footage was kept.
        let err = ScreenError::Retained {
            path: PathBuf::from("/staging/.2026-01-02 0915 Demo.mp4.part"),
            holds_footage: false,
            cause: Box::new(ScreenError::Sink("the capture recorded no video".into())),
        };
        let (message, retained) = describe_screen_error(&err);
        assert_eq!(
            retained.as_deref(),
            Some(Path::new("/staging/.2026-01-02 0915 Demo.mp4.part"))
        );
        assert!(
            !message.contains("still holds the recording"),
            "got {message}"
        );
        assert!(message.contains("no video"), "got {message}");
    }

    #[test]
    fn every_other_screen_error_carries_a_message_and_no_retained_path() {
        // A non-Retained variant has no surviving file, so reporting one
        // would offer the user a path that does not exist.
        for err in [
            ScreenError::Unsupported,
            ScreenError::SourceGone,
            ScreenError::EncoderUnavailable,
            ScreenError::AlreadyCapturing,
            ScreenError::Io("no space".into()),
            ScreenError::Sink("mux refused".into()),
        ] {
            let (message, retained) = describe_screen_error(&err);
            assert_eq!(retained, None, "{err} must carry no retained path");
            assert_eq!(message, err.to_string(), "{err} must render its Display");
        }
    }

    #[test]
    fn the_source_kind_crosses_the_wire_as_the_sidecar_spelling() {
        // The sidecar stores this as a plain string (staging::StagedSidecar),
        // so a renamed spelling here would silently orphan every capture a
        // previous build staged.
        let screen = source::SourceId::parse("screen:0").expect("screen id");
        let window = source::SourceId::parse("window:12345").expect("window id");
        assert_eq!(source_kind_str(&screen), "screen");
        assert_eq!(source_kind_str(&window), "window");
    }

    #[test]
    fn a_finalize_reports_the_capture_even_when_the_sidecar_cannot_be_written() {
        // The capture already landed on disk as `outcome.mp4`. Losing the
        // resume metadata is a lesser, separately-logged problem than
        // reporting a safely-saved capture as failed — so the DTO must come
        // back intact. The parent directory deliberately does not exist, so
        // `staging::write_sidecar` really fails here (nothing is written).
        let bundle = ScreenStopBundle {
            outcome: ScreenOutcome {
                mp4: PathBuf::from("/vb-no-such-dir-9e3a/2026-01-02 0915 Demo.mp4"),
                duration_ms: 61_000,
                paused_ms: 4_000,
                width: 1920,
                height: 1080,
                dropped: 3,
                warning: Some("the capture source closed".to_string()),
            },
            source_title: "Demo Window".to_string(),
            source_kind: "window",
            recorded_at: "2026-01-02T09:15:00+01:00".to_string(),
            inputs: vec!["Microphone (Yeti)".to_string()],
        };
        let (dto, warning) = finalize_stopped("vault-1", bundle);
        assert_eq!(dto.base, "2026-01-02 0915 Demo");
        assert_eq!(dto.path, "/vb-no-such-dir-9e3a/2026-01-02 0915 Demo.mp4");
        assert_eq!(dto.duration_ms, 61_000);
        assert_eq!(dto.source_title, "Demo Window");
        assert_eq!(dto.width, 1920);
        assert_eq!(dto.height, 1080);
        // A warning must survive to the stopped event: spec 14's "a closed
        // window still saves" story is told by this string.
        assert_eq!(warning.as_deref(), Some("the capture source closed"));
    }

    // Structural regression, the style this file's sibling already uses. The
    // monitor must drop the reservation BEFORE it announces the outcome —
    // audio's capture-monitor does (capture_commands.rs), and a listener
    // that reacts to `screen:stopped` by reading `screen_capture_status`
    // would otherwise still be told a capture is running.
    #[test]
    fn the_monitor_clears_the_reservation_before_it_emits() {
        let src = include_str!("screen_capture_worker.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        let clear = production
            .find("clear_active_screen(&monitor_app)")
            .expect("the monitor must clear the reservation");
        let emit = production
            .find("emit_screen_stopped(&monitor_app")
            .expect("the monitor must emit the stopped event");
        assert!(
            clear < emit,
            "clear the reservation before emitting, as capture_commands.rs's monitor does: \
             emitting first lets a listener read `capturing: true` for a capture it was \
             just told had finished, and puts the sidecar write and the OS toast inside \
             stop_screen_capture's 30 s budget (a slow disk then reports stillSaving on a \
             fully-saved capture)"
        );
    }
}
