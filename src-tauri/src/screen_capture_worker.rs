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
use vault_buddy_screen::{source, staging, staging_title, ScreenError};

use crate::capture_commands::now_ms;
use crate::capture_guard::{CaptureGuard, CaptureKind};

use crate::screen_commands::{
    clear_active_screen, clear_capture_window_effects, emit_screen_failed, emit_screen_stopped,
    selection_from, ActiveScreenCapture, ScreenCaptureState, READY_TIMEOUT,
};
use crate::screen_dto::{ScreenStatusPayload, StagedCaptureDto};

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

/// Whether the outcome monitor tells the user how the capture ended.
///
/// `Events` is the ordinary capture: the user pressed Start, saw it running,
/// and is owed `screen:stopped` (or `screen:failed`) when it finishes.
///
/// `Silent` is the ready-handshake timeout. That start has ALREADY been
/// reported to the user as a failure, so a later "Screen capture ready"
/// toast would contradict the error they are still looking at and park a
/// capture they were told did not happen in the bar's **Edit** slot. The
/// monitor still runs, and still writes the sidecar, so the footage the
/// device thread did manage to publish is a complete staged capture — it
/// shows up in the Record Screen picker's staged list, where a capture the
/// user did not expect is discoverable rather than announced.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Announce {
    Events,
    Silent,
}

/// Spawn the one thread that drains the device worker's outcome.
///
/// It owns the release of the reservation — and therefore of `CaptureGuard`,
/// which `clear_active_screen` frees with it. That ownership is the whole
/// point: the monitor is the only thing in the process that learns when the
/// device thread has really ended, so any path that frees the reservation
/// without it is guessing (GAP-110).
fn spawn_outcome_monitor(
    app: &AppHandle,
    vault_id: String,
    done_rx: mpsc::Receiver<Result<ScreenStopBundle, ScreenError>>,
    announce: Announce,
) -> std::io::Result<()> {
    let monitor_app = app.clone();
    let monitor_vault_id = vault_id;
    std::thread::Builder::new()
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
                    // The sidecar write happens on BOTH announce modes: a
                    // staged .mp4 without one is invisible to the staged
                    // list and only guessable by the recovery sweep.
                    let (dto, warning) = finalize_stopped(&monitor_vault_id, bundle);
                    if announce == Announce::Events {
                        emit_screen_stopped(&monitor_app, &dto, warning.as_deref());
                    } else {
                        log::warn!(
                            "screen capture: a start that timed out published {:?} anyway; \
                             staged without an event",
                            dto.base
                        );
                    }
                }
                Err(e) => {
                    let (message, retained) = describe_screen_error(&e);
                    if announce == Announce::Events {
                        emit_screen_failed(&monitor_app, &message, retained.as_deref());
                    } else {
                        log::warn!("screen capture: a start that timed out ended: {message}");
                    }
                }
            }
            crate::tray::set_capture_state(&monitor_app, crate::tray::TrayCaptureState::Idle);
        })
        .map(|_| ())
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
    webcam_id: Option<String>,
    inputs: Vec<String>,
    outputs: Vec<String>,
) -> Result<ScreenStatusPayload, String> {
    // The id crosses the IPC boundary untrusted. Refusing an unparseable one
    // here, before ANYTHING is claimed, means a malformed request cannot
    // wedge either capture domain.
    let parsed =
        source::SourceId::parse(&source_id).ok_or_else(|| "Unknown capture source.".to_string())?;
    // The optional webcam (F-22), refused on the same terms and at the same
    // point: before anything is claimed. `None` is today's capture.
    let webcam = crate::screen_webcam_commands::parse_webcam_request(webcam_id.as_deref())?;

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
            startup_wedged: false,
        });
    }

    // Spec 5.3: from here on the capture is committed, and every exit --
    // including every failure below -- goes through `clear_active_screen`,
    // which is the one place the exclusion is lifted. Applying it before
    // the session opens means the first frames are already clean; it is
    // fire-and-forget on the main thread, so a busy event loop can still
    // let a frame or two of buddy through (docs/Gaps.md GAP-124)
    // rather than delaying the start.
    crate::capture_exclusion::apply(app);

    // The region border (GAP-165), beside the exclusion for the same
    // reasons: both are capture-scoped window side effects, and both belong
    // after the reservation so every failure below funnels through the same
    // teardown. Gated INSIDE `show` on the source being a REGION -- Windows'
    // own border is already truthful for a screen or window capture.
    crate::region_indicator::show(app, &parsed);

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
    // Task 53: read once with the rest of the vault's settings, so a toggle
    // flipped mid-capture applies to the NEXT recording, never this one.
    let keep_stems = cfg.screen_audio_stems;
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
                &staging_title::sanitize_title(&title),
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
                webcam: crate::screen_webcam_commands::webcam_params(webcam, &dir, &base, quality),
                stems: vault_buddy_screen::session::stems::stem_params(
                    keep_stems,
                    &dir,
                    &base,
                    &input_names,
                ),
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
            // GAP-110. The timeout fires while the device thread is BY
            // DEFINITION still alive — the commonest shape is one wedged
            // inside `open_selected_sources` on a bad audio driver, the same
            // premise the audio domain's own timeout was written for. So the
            // reservation is NOT freed here: freeing it frees `CaptureGuard`
            // with it, and the user's retry then opens the same audio
            // endpoint beside a capture that is still starting — the exact
            // reliability hazard spec 7.3 says the guard exists to prevent.
            //
            // Stop pre-emptively (the thread sees it the moment it reaches
            // its control loop), then hand `done_rx` to the monitor, which
            // is the only thing that learns the device thread has really
            // ended. It clears the reservation then, writes the sidecar for
            // whatever did get published — an .mp4 without one is invisible
            // to the staged list — and stays SILENT, because the user is
            // about to be told this start failed.
            let _ = control_tx.send(Control::Stop);
            let msg = "Screen capture did not start in time.".to_string();
            if let Err(e) = spawn_outcome_monitor(app, id.clone(), done_rx, Announce::Silent) {
                // No monitor means nothing would EVER free the reservation,
                // which is worse than the window this arm exists to close:
                // the app would refuse every later capture until restart.
                // Fall back to today's behaviour and say why.
                log::warn!("screen capture: no monitor for a timed-out start ({e}); releasing");
                clear_active_screen(app);
                emit_screen_failed(app, &msg, None);
                return Err(msg);
            }
            // The reservation now outlives this call, so mark it: with no
            // `.part` yet it must not make the app unquittable or unhidable
            // (GAP-08, the audio domain's own scoped exception).
            if let Some(active) = lock_ignoring_poison(&state.0).as_mut() {
                active.startup_wedged = true;
            }
            // The WINDOW effects go NOW, even though the reservation does
            // not. The reservation tracks what the DEVICES are doing; these
            // track what the USER sees, and the user is about to be told
            // this start failed. A region border left up is a click-through
            // window over their screen that nothing recording explains and
            // that they cannot dismiss without quitting; the buddy's
            // exclusion is invisible from inside the app and would silently
            // keep it out of other applications' recordings. Both are
            // idempotent, so the monitor's own `clear_active_screen` will
            // re-run them harmlessly.
            clear_capture_window_effects(app);
            emit_screen_failed(app, &msg, None);
            return Err(msg);
        }
    };

    // The ONLY consumer of the worker's outcome — covers an explicit stop
    // AND self-finalization (the source closing mid-capture), so the
    // reservation clears and screen:stopped/failed fires no matter who
    // ended the capture (mirrors capture_commands.rs's capture-monitor).
    let monitor = spawn_outcome_monitor(app, id.clone(), done_rx, Announce::Events);
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
    let mut sidecar = staging::StagedSidecar {
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
        // Explicit, never `..Default::default()`: this is the ONE production
        // construction site, and a new sidecar field must fail to compile
        // here rather than silently default (review fix round 1).
        webcam: None,
        stems: Vec::new(),
        extra: Default::default(),
    };
    // The measured webcam track (F-22, GAP-199), when one finished.
    if let Some(webcam) = &outcome.webcam {
        sidecar.set_webcam(crate::screen_webcam_commands::webcam_block(&base, webcam));
    }
    // The stems that finished complete (Task 53); an incomplete one was a
    // warning and is not listed, so discard and Clear own exactly these.
    sidecar.set_stems(vault_buddy_screen::session::stems::stem_sidecar_entries(
        &base,
        &outcome.stems,
    ));
    if let Err(e) = staging::write_sidecar(dir, &base, &sidecar) {
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
#[path = "screen_capture_worker_tests.rs"]
mod tests;
