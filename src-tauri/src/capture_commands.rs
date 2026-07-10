use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vault_buddy_capture::session::{CaptureSession, Control, Outcome, SessionParams};
use vault_buddy_core::services::{self, RecordingDto, ServicePaths};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery, transcript, uri};

use crate::transcription::{enqueue_transcription, TranscriptionJob};

pub struct ActiveCapture {
    pub control_tx: Sender<Control>,
    pub vault_id: String,
    pub started_at_ms: u64,
    /// Pause bookkeeping mirrors the session (which owns the truth for the
    /// encoded timeline) so capture_status can resync a reloaded webview's
    /// frozen-elapsed display exactly.
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    /// The .part file the live session owns, once the worker has reserved
    /// it — None while devices are still being set up (and for a timed-out
    /// start whose worker never reported back).
    pub part: Option<PathBuf>,
    /// True only for a reservation whose start timed out (the worker never
    /// reported back). Together with `part.is_none()` it marks the one
    /// state shutdown may bypass: nothing reached disk, so the
    /// never-lose-audio invariant is not in play (GAP-08).
    pub startup_wedged: bool,
}

/// The mutex holds the active-capture reservation; the condvar is notified
/// whenever the reservation is cleared so stop-waiters block on it instead
/// of polling (see `request_stop_and_wait` / `clear_active`).
#[derive(Default)]
pub struct CaptureState(pub Mutex<Option<ActiveCapture>>, pub Condvar);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub recording: bool,
    pub vault_id: Option<String>,
    pub started_at_ms: Option<u64>,
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamedPayload {
    pub mp3: String,
    pub note: Option<String>,
    pub warning: Option<String>,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn toast(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Every capture failure surfaces through here — panel event AND toast —
/// so no path can silently log-and-vanish (the UI must never look healthy
/// after a failed start or finalize).
fn emit_failed(app: &AppHandle, message: &str) {
    log::error!("capture: failed: {message}");
    let _ = app.emit("capture:failed", serde_json::json!({ "message": message }));
    toast(app, "Recording failed", message);
}

/// Clear the active-capture reservation and wake everyone blocked in
/// `request_stop_and_wait`. Every site that resets the state to None must
/// go through here, or stop-waiters sleep until their next timeout.
fn clear_active(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    *lock_ignoring_poison(&state.0) = None;
    state.1.notify_all();
}

fn emit_saved(app: &AppHandle, outcome: &Outcome) {
    let file_name = outcome
        .mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = app.emit(
        "capture:saved",
        serde_json::json!({
            "mp3": outcome.mp3.to_string_lossy(),
            "note": outcome.note.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "endedEarly": outcome.ended_early,
            "warning": outcome.warning,
        }),
    );
    // Source-loss warnings were already emitted live via warn_tx; here the
    // outcome also feeds the note metadata, the ended-early toast copy, and
    // (serialized string-or-null) the `warning` field — e.g. a failed
    // companion-note write — which the panel shows as its own notification.
    let body = if outcome.ended_early {
        format!("Recording ended early — saved {file_name}")
    } else {
        format!("Saved {file_name}")
    };
    toast(app, "Recording saved", &body);
}

#[tauri::command]
pub fn capture_status(state: tauri::State<CaptureState>) -> StatusPayload {
    let guard = lock_ignoring_poison(&state.0);
    match guard.as_ref() {
        Some(active) => StatusPayload {
            recording: true,
            vault_id: Some(active.vault_id.clone()),
            started_at_ms: Some(active.started_at_ms),
            paused: active.paused,
            paused_total_ms: active.paused_total_ms,
            paused_since_ms: active.paused_since_ms,
        },
        None => StatusPayload {
            recording: false,
            vault_id: None,
            started_at_ms: None,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
        },
    }
}

/// Serializes set_capture_config's read-modify-write of config.json —
/// concurrent saves for different vaults must not lose each other's
/// fields (the write path itself is lock-free by design).
#[derive(Default)]
pub struct ConfigWriteLock(pub Mutex<()>);

pub const BITRATES_KBPS: [u32; 3] = [128, 160, 192];
pub const TRANSCRIPTION_MODELS: [&str; 3] = ["base", "small", "medium"];

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfigDto {
    pub mode: String,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub transcribe: bool,
    pub transcription_model: String,
    pub transcription_language: Option<String>,
    pub transcript_timestamps: bool,
    pub follow_up_template: bool,
}

impl CaptureConfigDto {
    fn from_config(v: &capture_config::VaultCaptureConfig) -> Self {
        Self {
            mode: v.mode.as_key().to_string(),
            recording_folder: v.recording_folder.clone(),
            bitrate_kbps: v.bitrate_kbps,
            create_note: v.create_note,
            input_device: v.input_device.clone(),
            output_device: v.output_device.clone(),
            transcribe: v.transcribe,
            transcription_model: v.transcription_model.clone(),
            transcription_language: v.transcription_language.clone(),
            transcript_timestamps: v.transcript_timestamps,
            follow_up_template: v.follow_up_template,
        }
    }
}

#[tauri::command]
pub fn get_capture_config(id: String) -> CaptureConfigDto {
    // Unknown vaults return the defaults — exactly what a fresh form shows.
    CaptureConfigDto::from_config(&capture_config::vault_config(
        &capture_config::load_config(),
        &id,
    ))
}

#[tauri::command]
pub fn set_capture_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    cfg: CaptureConfigDto,
) -> Result<(), String> {
    let mode = capture_config::RecordingMode::from_key(&cfg.mode)
        .ok_or_else(|| format!("Unknown recording mode: {}", cfg.mode))?;
    if !BITRATES_KBPS.contains(&cfg.bitrate_kbps) {
        return Err(format!("Bitrate must be one of {BITRATES_KBPS:?} kbps"));
    }
    if !TRANSCRIPTION_MODELS.contains(&cfg.transcription_model.as_str()) {
        return Err(format!(
            "Unknown transcription model: {}",
            cfg.transcription_model
        ));
    }
    // Validate the folder against the real vault path BEFORE writing —
    // an invalid folder is an inline field error, nothing gets written.
    let vault = crate::commands::find_vault(&id)?;
    let folder = cfg
        .recording_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    if let Some(folder) = &folder {
        capture_paths::safe_recording_root(Path::new(&vault.path), folder)?;
    }
    let _guard = lock_ignoring_poison(&lock.0);
    // Preserve fields CaptureConfigDto doesn't carry (tasks are configured on
    // their own surface) so saving capture settings can't reset them. The read
    // must sit INSIDE the lock: a concurrent set_tasks_config also
    // read-modify-writes this vault, so reading tasks_folder before the guard
    // would let us write back a stale value and clobber its update.
    let existing = capture_config::vault_config(&capture_config::load_config(), &id);
    let value = capture_config::VaultCaptureConfig {
        mode,
        recording_folder: folder,
        bitrate_kbps: cfg.bitrate_kbps,
        create_note: cfg.create_note,
        input_device: cfg.input_device.clone().filter(|d| !d.is_empty()),
        output_device: cfg.output_device.clone().filter(|d| !d.is_empty()),
        transcribe: cfg.transcribe,
        transcription_model: cfg.transcription_model.clone(),
        transcription_language: cfg.transcription_language.clone().filter(|l| !l.is_empty()),
        transcript_timestamps: cfg.transcript_timestamps,
        follow_up_template: cfg.follow_up_template,
        tasks_folder: existing.tasks_folder,
    };
    let result = capture_config::update_vault_config(&id, value.clone());
    if result.is_ok() {
        log::info!(
            "capture config saved for vault {id}: mode={}, folder={:?}, bitrate={}kbps, note={}, input={:?}, output={:?}, transcribe={}",
            value.mode.as_key(),
            value.recording_folder,
            value.bitrate_kbps,
            value.create_note,
            value.input_device,
            value.output_device,
            value.transcribe
        );
    }
    result
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoDto {
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListDto {
    pub inputs: Vec<DeviceInfoDto>,
    pub outputs: Vec<DeviceInfoDto>,
}

/// ASYNC (GAP-22): COM/WASAPI enumeration commonly takes hundreds of ms;
/// on the main thread that stalled the settings view. cpal initializes COM
/// per calling thread, so the blocking pool is fine (the capture-device
/// worker already enumerates off-main).
#[tauri::command]
pub async fn list_audio_devices() -> DeviceListDto {
    tauri::async_runtime::spawn_blocking(|| {
        let list = vault_buddy_capture::devices::list_devices();
        let map = |d: vault_buddy_capture::devices::DeviceInfo| DeviceInfoDto {
            name: d.name,
            is_default: d.is_default,
        };
        DeviceListDto {
            inputs: list.inputs.into_iter().map(map).collect(),
            outputs: list.outputs.into_iter().map(map).collect(),
        }
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("list_audio_devices: task failed: {e}");
        DeviceListDto {
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    })
}

/// Read-only list of a vault's past recordings for the Recordings view.
/// Scans the vault's recording roots (custom folder, or both mode defaults)
/// and reads each recording's companion note for type/duration. An unknown
/// vault or unreadable roots yield an empty list — never an error (mirrors
/// discovery's degrade-to-empty rule). Never writes into the vault.
///
/// ASYNC (GAP-22): scans dated folders and reads every companion note's
/// frontmatter — a large archive stalled the UI on every panel open.
#[tauri::command]
pub async fn list_recordings(id: String) -> Vec<RecordingDto> {
    tauri::async_runtime::spawn_blocking(move || {
        services::list_recordings(&ServicePaths::real(), &id)
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("list_recordings: task failed: {e}");
        Vec::new()
    })
}

fn start_capture_blocking(
    app: &AppHandle,
    id: String,
    mode: Option<String>,
) -> Result<StatusPayload, String> {
    // Everything fallible-but-cheap (discovery, config, path validation)
    // runs BEFORE the state lock is touched — the mutex must never be held
    // across file I/O or device setup.
    let vault = crate::commands::find_vault(&id)?;
    let vault_path = PathBuf::from(&vault.path);
    if !vault_path.is_dir() {
        log::warn!("start_capture: vault folder missing: {}", vault.path);
        return Err("Vault folder not found — was it moved or deleted?".to_string());
    }

    let mut cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    // Per-recording override from the mode chooser: the config only
    // supplies the DEFAULT. Overriding cfg.mode up front keeps every
    // downstream decision (loopback, label, folder default, note type)
    // consistent with the user's pick.
    if let Some(key) = &mode {
        cfg.mode = capture_config::RecordingMode::from_key(key)
            .ok_or_else(|| format!("Unknown recording mode: {key}"))?;
        log::info!(
            "capture: mode override for this recording: {}",
            cfg.mode.label()
        );
    }
    let uses_loopback = cfg.mode.uses_loopback();
    let label = cfg.mode.label();
    // Hand-editable config must never escape the vault (PRD guarantee).
    let root = capture_paths::safe_recording_root(&vault_path, cfg.effective_recording_folder())?;

    // Device validation happens on the worker thread BEFORE any file is
    // created (spec: start failures stay file-free).
    let (control_tx, control_rx) = mpsc::channel::<Control>();
    let (done_tx, done_rx) = mpsc::channel::<Result<Outcome, String>>();
    // Ok carries the reserved .part path so the reservation below learns
    // which file the live session owns.
    let (ready_tx, ready_rx) = mpsc::channel::<Result<PathBuf, String>>();

    // Reserve the state up front: the lock is held only for the is-running
    // check plus the insert, which closes the double-start window without
    // serializing device setup (or any I/O) under the mutex.
    let state = app.state::<CaptureState>();
    {
        let mut guard = lock_ignoring_poison(&state.0);
        if guard.is_some() {
            return Err("A recording is already running.".to_string());
        }
        *guard = Some(ActiveCapture {
            control_tx: control_tx.clone(),
            vault_id: id.clone(),
            started_at_ms: now_ms(),
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            part: None,
            startup_wedged: false,
        });
    }

    let vault_name = vault.name.clone();
    let vault_path2 = vault_path.clone();

    // Live source-loss warnings: forwarded to the panel while recording.
    let (warn_tx, warn_rx) = mpsc::channel::<String>();
    let app_warn = app.clone();
    let spawned = std::thread::Builder::new()
        .name("capture-warn".into())
        .spawn(move || {
            while let Ok(message) = warn_rx.recv() {
                let _ = app_warn.emit("capture:warning", serde_json::json!({ "message": message }));
            }
        });
    if let Err(e) = spawned {
        // Recording proceeds without live warning forwarding; sends into
        // the dropped receiver are already fire-and-forget.
        log::warn!("could not spawn capture-warn thread: {e}");
    }

    // Advisory level meter: forward the worker's ~5 Hz peaks to the panel.
    let (level_tx, level_rx) = mpsc::channel::<f32>();
    let app_level = app.clone();
    let spawned = std::thread::Builder::new()
        .name("capture-level".into())
        .spawn(move || {
            while let Ok(peak) = level_rx.recv() {
                let _ = app_level.emit("capture:level", serde_json::json!({ "peak": peak }));
            }
        });
    if let Err(e) = spawned {
        // Recording proceeds without live level forwarding; sends into the
        // dropped receiver are already fire-and-forget.
        log::warn!("could not spawn capture-level thread: {e}");
    }

    let device_thread = std::thread::Builder::new()
        .name("capture-device".into())
        .spawn(move || {
            let open = match vault_buddy_capture::devices::open_sources(
                uses_loopback,
                cfg.input_device.as_deref(),
                cfg.output_device.as_deref(),
            ) {
                Ok(o) => o,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            // Stale-device fallbacks: surface live (capture:warning via the
            // forwarder) AND seed the session so the note metadata records it.
            for w in &open.warnings {
                let _ = warn_tx.send(w.clone());
            }
            let start_warning = (!open.warnings.is_empty()).then(|| open.warnings.join("; "));
            let now = chrono::Local::now();
            use chrono::Timelike;
            let date = now.date_naive();
            let dir = capture_paths::dated_folder(&root, date);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                let _ = ready_tx.send(Err(format!("Cannot create recording folder: {e}")));
                return;
            }
            // A pre-existing symlink/junction at the recording folder must
            // not carry writes outside the vault (lexical check can't see it).
            if let Err(e) = capture_paths::assert_root_inside_vault(&vault_path2, &dir) {
                let _ = ready_tx.send(Err(e));
                return;
            }
            let base = capture_paths::base_name(date, now.hour(), now.minute(), label);
            let names = capture_paths::reserve_names(&dir, &base);
            let params = SessionParams {
                dir: dir.clone(),
                base: names.base.clone(),
                part: names.part.clone(),
                bitrate_kbps: cfg.bitrate_kbps,
                vault_name: vault_name.clone(),
                recording_type: label.to_string(),
                create_note: cfg.create_note,
                transcribe: cfg.transcribe,
                follow_up: cfg.follow_up_template,
                recorded_at: now.to_rfc3339(),
                flush_every: Duration::from_secs(1),
                fsync_every: Duration::from_secs(30),
                warn_tx: Some(warn_tx),
                level_tx: Some(level_tx),
                start_warning,
            };
            let session = match CaptureSession::start(params, open.inputs) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("Could not start recording: {e}")));
                    return;
                }
            };
            log::info!(
                "capture: started in vault '{vault_name}' → {}",
                names.part.display()
            );
            let _ = ready_tx.send(Ok(names.part.clone()));

            // Own the streams here; poll for control or self-finalization.
            let streams = open.streams;
            loop {
                match control_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Control::Pause) => session.pause(),
                    Ok(Control::Resume) => session.resume(),
                    Err(RecvTimeoutError::Timeout) => {
                        if !session.is_running() {
                            break; // sources died; worker self-finalized
                        }
                    }
                }
            }
            // Stop the session while the streams are still alive: dropping
            // them first disconnects every source channel, and the worker
            // could mistake an ordinary stop for all-sources-lost (bogus
            // ended_early + source-loss warnings in the toast and note).
            let outcome = session.stop();
            drop(streams);
            let _ = done_tx.send(outcome);
        });
    if let Err(e) = device_thread {
        // Without the worker there IS no recording: nothing has touched
        // disk yet, so fail the start cleanly and drop the reservation.
        clear_active(app);
        let msg = format!("Could not start the recording worker: {e}");
        emit_failed(app, &msg);
        return Err(msg);
    }

    // Wait for device readiness WITHOUT the lock — concurrent starts are
    // already rejected by the reservation above.
    let started_at_ms = match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(part)) => {
            // Recording is live: pin the .part the session owns (recovery
            // must never treat it as an orphan) and restamp the start time
            // now that device setup is done — that is what the UI timer
            // should count from.
            let started_at_ms = now_ms();
            if let Some(active) = lock_ignoring_poison(&state.0).as_mut() {
                active.part = Some(part);
                active.started_at_ms = started_at_ms;
            }
            started_at_ms
        }
        Ok(Err(e)) => {
            clear_active(app);
            emit_failed(app, &e);
            return Err(e);
        }
        Err(_) => {
            // Startup hung (e.g. a wedged audio driver). The worker may
            // still create the .part and start the session AFTER this
            // return — never leave that recording detached, and never let
            // a second start_capture race it. So the reservation installed
            // above is deliberately KEPT: capture_status conservatively
            // reports "recording" and concurrent starts stay rejected,
            // exactly as if the late worker had already succeeded. Send an
            // immediate stop so a late worker halts as soon as it reaches
            // its poll loop, and leave cleanup to the janitor thread
            // below, which drains the worker's real outcome and only then
            // clears the reservation (and resets the tray) — so if the
            // worker is truly wedged, the state stays reserved until its
            // recv() finally returns or the app restarts.
            let msg = "Recording did not start in time.".to_string();
            let _ = control_tx.send(Control::Stop);
            if let Some(active) = lock_ignoring_poison(&state.0).as_mut() {
                active.startup_wedged = true;
            }
            let app4 = app.clone();
            let janitor = std::thread::Builder::new()
                .name("capture-janitor".into())
                .spawn(move || {
                    if let Ok(Ok(part)) = ready_rx.recv() {
                        // The late worker DID reach disk: record its .part so
                        // the shutdown bypass (GAP-08) closes for this drain.
                        if let Some(active) =
                            lock_ignoring_poison(&app4.state::<CaptureState>().0).as_mut()
                        {
                            active.part = Some(part.clone());
                        }
                        log::warn!(
                            "capture: late start after timeout — stopping and draining {}",
                            part.display()
                        );
                        match done_rx.recv() {
                            Ok(Ok(outcome)) => emit_saved(&app4, &outcome),
                            Ok(Err(e)) => {
                                // A late-start finalize failure must reach the
                                // UI, not just the log file.
                                log::warn!("capture: late-start cleanup failed: {e}");
                                emit_failed(&app4, &e);
                            }
                            Err(_) => {
                                log::warn!("capture: late-start cleanup: worker vanished");
                                emit_failed(&app4, "capture thread vanished");
                            }
                        }
                    }
                    // worker replied Err (or vanished): nothing was created,
                    // but the reservation installed above still needs clearing
                    // either way, or a real recording could never start again.
                    clear_active(&app4);
                    crate::tray::set_capture_state(&app4, crate::tray::TrayCaptureState::Idle);
                });
            if let Err(e) = janitor {
                // The janitor closure (and the ready_rx/done_rx it would
                // have consumed) is dropped along with the failed spawn, so
                // nothing is left listening for the worker's reply — a late
                // reply clears nothing. The reservation stays wedged until
                // the app restarts; quit stays possible via the
                // startup-wedged shutdown bypass stamped above (GAP-08),
                // and a late `.part` that does land is recovered as
                // `(recovered)` on the next launch.
                log::error!("could not spawn capture-janitor thread: {e}");
            }
            emit_failed(app, &msg);
            return Err(msg);
        }
    };

    let monitor_vault_id = id.clone();
    let payload = StatusPayload {
        recording: true,
        vault_id: Some(id),
        started_at_ms: Some(started_at_ms),
        paused: false,
        paused_total_ms: 0,
        paused_since_ms: None,
    };

    // Monitor thread: the ONLY consumer of the session outcome. Covers
    // user/menu/shutdown stops AND self-finalization (all sources lost) —
    // the state clears and the outcome surfaces no matter who ended it.
    let app3 = app.clone();
    let monitor = std::thread::Builder::new()
        .name("capture-monitor".into())
        .spawn(move || {
            let result = done_rx
                .recv()
                .unwrap_or_else(|_| Err("capture thread vanished".to_string()));
            clear_active(&app3);
            match result {
                Ok(outcome) => {
                    emit_saved(&app3, &outcome);
                    maybe_enqueue_transcription(&app3, &monitor_vault_id, &outcome.mp3);
                }
                Err(e) => {
                    log::error!("capture: finalize failed: {e}");
                    emit_failed(&app3, &e);
                }
            }
            crate::tray::set_capture_state(&app3, crate::tray::TrayCaptureState::Idle);
        });
    if let Err(e) = monitor {
        // Without a monitor nothing would ever drain the outcome or clear
        // the state. Stop the session — the device thread still finalizes
        // and the audio reaches disk (its done_tx send is a no-op into the
        // dropped receiver) — and report the start as failed.
        let _ = control_tx.send(Control::Stop);
        clear_active(app);
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Idle);
        let msg = format!("Recording could not be monitored; stopping: {e}");
        emit_failed(app, &msg);
        return Err(msg);
    }

    Ok(payload)
}

/// ASYNC (GAP-21): the 10 s device-ready wait (`ready_rx.recv_timeout`)
/// froze the whole UI when this ran as a sync command on the main thread —
/// a wedged audio driver is the timeout's own premise. The body runs on
/// the blocking pool; reservation semantics are unchanged (names reserved
/// under the CaptureState mutex before the worker spawns, double-starts
/// rejected). The one main-thread-only side effect — showing the buddy,
/// the recording indicator — is marshalled back via run_on_main_thread
/// (window show/hide is main-thread-only; tray updates off-main are the
/// capture-monitor precedent).
#[tauri::command]
pub async fn start_capture(
    app: AppHandle,
    id: String,
    mode: Option<String>,
) -> Result<StatusPayload, String> {
    let worker = app.clone();
    let payload =
        tauri::async_runtime::spawn_blocking(move || start_capture_blocking(&worker, id, mode))
            .await
            .map_err(|e| {
                log::warn!("start_capture: task failed: {e}");
                "Recording start failed — see the logs for details.".to_string()
            })??;
    // Indicator hardening: recording buddy must be visible. Best-effort,
    // same as before — a failed post just loses the show, never the start.
    let shower = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = shower.get_webview_window("main") {
            let _ = window.show();
        }
    });
    crate::tray::set_capture_state(&app, crate::tray::TrayCaptureState::Recording);
    let _ = app.emit("capture:started", payload.clone());
    Ok(payload)
}

/// After a save, if the vault opted into transcription, drop the
/// "transcribing…" placeholder (so the note's embed resolves instantly) and
/// queue the recording. Config is re-read here so a toggle mid-session is
/// respected.
fn maybe_enqueue_transcription(app: &AppHandle, vault_id: &str, mp3: &Path) {
    let cfg = capture_config::vault_config(&capture_config::load_config(), vault_id);
    if !cfg.transcribe {
        return;
    }
    if let Err(e) = vault_buddy_core::transcript::write_placeholder(mp3) {
        log::warn!(
            "transcribe: writing placeholder for {} failed: {e}",
            mp3.display()
        );
    }
    enqueue_transcription(
        app,
        TranscriptionJob {
            mp3: mp3.to_path_buf(),
            vault_id: vault_id.to_string(),
            force: false,
        },
    );
}

/// Ask the device thread to stop and wait until the monitor thread has
/// cleared the state (i.e. the outcome landed and events were emitted).
/// The wait blocks on the state condvar — no polling. `wait: None` means
/// wait forever — shutdown paths use it so the app can never exit while a
/// recording is still finalizing (a slow vault or a stuck fsync must not
/// strand the capture as .part).
/// Outcome of a stop wait: `Cleared` = the reservation was released (the
/// save landed, there was nothing to wait for, or a startup-wedged
/// reservation was bypassed); `TimedOut` = the bounded deadline expired
/// while finalize was still running — the caller must not report success.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopWait {
    Cleared,
    TimedOut,
}

fn request_stop_and_wait(app: &AppHandle, wait: Option<Duration>) -> StopWait {
    // Bound to a local so the guard below can borrow it across statements —
    // `app.state::<CaptureState>()` is otherwise a temporary that would be
    // dropped at the end of the `let guard = …;` statement.
    let capture_state = app.state::<CaptureState>();
    let mut guard = lock_ignoring_poison(&capture_state.0);
    let Some(active) = guard.as_ref() else {
        return StopWait::Cleared;
    };
    let _ = active.control_tx.send(Control::Stop);
    if wait.is_none() && bypasses_shutdown_wait(active) {
        // Shutdown against a wedged startup: nothing on disk to strand, and
        // recv() may never return — don't hold quit hostage. The Stop above
        // still halts a late worker the moment it reaches its poll loop.
        log::warn!(
            "capture: bypassing shutdown wait for a startup-wedged reservation (nothing on disk)"
        );
        return StopWait::Cleared;
    }
    let deadline = wait.map(|limit| std::time::Instant::now() + limit);
    while guard.is_some() {
        match deadline {
            Some(deadline) => {
                let now = std::time::Instant::now();
                if now >= deadline {
                    log::warn!("capture: stop wait timed out");
                    return StopWait::TimedOut;
                }
                // A poisoned condvar wait must recover the same way the
                // mutex does: recovering the pair keeps shutdown waiting
                // instead of panicking mid-finalize.
                guard = capture_state
                    .1
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            None => {
                // Unbounded waits still wake every 15s, purely to keep the
                // "still finalizing…" heartbeat in the shutdown logs.
                let (g, timeout) = capture_state
                    .1
                    .wait_timeout(guard, Duration::from_secs(15))
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard = g;
                if timeout.timed_out() && guard.is_some() {
                    log::warn!("capture: still finalizing…");
                }
            }
        }
    }
    StopWait::Cleared
}

/// Wire result for stop_capture. `stillSaving` = the bounded wait expired
/// while finalize was still running; the frontend keeps its saving UI and
/// lets capture:saved/failed finish the story (GAP-20).
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopOutcomeDto {
    pub still_saving: bool,
}

impl StopOutcomeDto {
    fn from_wait(wait: StopWait) -> Self {
        Self {
            still_saving: wait == StopWait::TimedOut,
        }
    }
}

/// ASYNC (GAP-20): the wait is on CaptureState's condvar — up to 15 s of
/// LAME flush + fsync + rename on a slow vault — which froze the whole UI
/// when this ran as a sync command on the main thread. It touches no
/// window APIs and no window-state locks, so the window-thread invariant
/// doesn't pin it; the condvar wait runs on the blocking pool.
#[tauri::command]
pub async fn stop_capture(app: AppHandle) -> Result<StopOutcomeDto, String> {
    if !is_recording(&app) {
        return Err("No recording is running.".to_string());
    }
    let waiter = app.clone();
    let wait = tauri::async_runtime::spawn_blocking(move || {
        request_stop_and_wait(&waiter, Some(Duration::from_secs(15)))
    })
    .await
    .map_err(|e| {
        log::warn!("stop_capture: wait task failed: {e}");
        "Stop failed — see the logs for details.".to_string()
    })?;
    Ok(StopOutcomeDto::from_wait(wait))
}

/// Stop triggered from a native menu (tray or buddy) rather than the panel.
pub fn stop_from_menu(app: &AppHandle) {
    let _ = request_stop_and_wait(app, Some(std::time::Duration::from_secs(15)));
}

pub fn is_recording(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<CaptureState>().0).is_some()
}

/// Whether shutdown/hide may skip waiting on this reservation: only a
/// startup-wedged one with nothing on disk. Everything else — live
/// recording, ordinary start, late worker whose .part we've learned —
/// keeps the wait-forever posture.
fn bypasses_shutdown_wait(active: &ActiveCapture) -> bool {
    active.startup_wedged && active.part.is_none()
}

/// The shutdown/hide variant of `is_recording`: a startup-wedged
/// reservation with no .part must not make the app unquittable or
/// unhidable (GAP-08), while capture_status et al. keep conservatively
/// reporting it as recording.
pub fn recording_blocks_shutdown(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<CaptureState>().0)
        .as_ref()
        .is_some_and(|active| !bypasses_shutdown_wait(active))
}

/// Shared by the IPC commands and the tray menu items. Errors are typed
/// for the UI (which disables the buttons in starting/saving states) —
/// but the tray can always race, so every precondition re-checks here.
fn set_paused(app: &AppHandle, pause: bool) -> Result<(), String> {
    let state = app.state::<CaptureState>();
    let mut guard = lock_ignoring_poison(&state.0);
    let Some(active) = guard.as_mut() else {
        return Err("No recording is running.".to_string());
    };
    if active.part.is_none() {
        return Err("Recording is still starting.".to_string());
    }
    if pause == active.paused {
        return Err(if pause {
            "Recording is already paused."
        } else {
            "Recording is not paused."
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
    // Captured under the lock so the audit log line below (after the guard
    // drops and the event emits) doesn't need to reacquire the mutex.
    let vault_id = active.vault_id.clone();
    drop(guard);
    if pause {
        let _ = app.emit("capture:paused", serde_json::json!({ "atMs": now }));
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Paused);
        log::info!("capture: paused (vault {vault_id})");
    } else {
        let _ = app.emit(
            "capture:resumed",
            serde_json::json!({ "pausedTotalMs": paused_total_ms }),
        );
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Recording);
        log::info!("capture: resumed after {paused_total_ms}ms total paused (vault {vault_id})");
    }
    Ok(())
}

#[tauri::command]
pub fn pause_capture(app: AppHandle) -> Result<(), String> {
    set_paused(&app, true)
}

#[tauri::command]
pub fn resume_capture(app: AppHandle) -> Result<(), String> {
    set_paused(&app, false)
}

#[tauri::command]
pub fn rename_capture(
    app: AppHandle,
    state: tauri::State<CaptureState>,
    mp3: String,
    title: String,
) -> Result<RenamedPayload, String> {
    // The prompt dismisses on a new recording (UI rule); this is the
    // backend guard for the same thing — never shuffle files next to a
    // directory a live session is writing into.
    if lock_ignoring_poison(&state.0).is_some() {
        return Err("Cannot rename while a recording is running.".to_string());
    }
    // Codex PR #46: the worker holds this exact path open mid-decode and its
    // terminal write (the sidecar) targets it. Renaming underneath an ACTIVE
    // job would leave the worker completing into the now-orphaned old path
    // while the renamed note embeds a placeholder that never resolves —
    // refuse outright rather than try to retarget work already in flight.
    // Check-then-act: the queue mutex is released between this check and the
    // execute below, so a job the worker claims in that window is renamed
    // anyway and the pending-retarget misses — bounded blast radius (the
    // pre-fix behavior for exactly that one job), accepted over holding the
    // transcription lock across rename I/O.
    if crate::transcription::is_active_transcription(&app, Path::new(&mp3)) {
        return Err("Cannot rename while this recording is being transcribed.".to_string());
    }
    if !Path::new(&mp3).is_file() {
        return Err("Recording file not found — was it moved?".to_string());
    }
    // Containment (GAP-07): every other write path gates on
    // assert_*_inside_vault; rename_plan validates only the capture-pattern
    // stem, so IPC could rename any `YYYY-MM-DD HHmm *.mp3` (and retarget
    // its note) anywhere on disk. Canonical matching per GAP-01's helper.
    let vaults = discovery::discover_vaults();
    if capture_paths::vault_owning_path(&vaults, Path::new(&mp3)).is_none() {
        return Err("Recording is not inside a known vault.".to_string());
    }
    let plan = capture_paths::rename_plan(Path::new(&mp3), &title)?;
    let stem = plan
        .mp3_from
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if plan.new_base == stem {
        // Confirming the unedited prefill: nothing to do, and running the
        // reservation anyway would mint a pointless " (2)" suffix (the
        // source itself occupies the target name).
        return Ok(RenamedPayload {
            note: plan
                .note_from
                .is_file()
                .then(|| plan.note_from.to_string_lossy().into_owned()),
            mp3,
            warning: None,
        });
    }
    let outcome = vault_buddy_capture::rename::execute(&plan)?;
    // Codex PR #46: a job still QUEUED for the pre-rename path must follow
    // the file, or the worker later writes its terminal sidecar under a name
    // the renamed note no longer embeds, leaving the note's (moved) pending
    // placeholder stuck until the next launch's backfill re-runs inference.
    // `outcome.mp3` (not `mp3`/`plan`) is the actual landed name — it may
    // carry a collision suffix (` (2)`) the plan didn't predict.
    if crate::transcription::retarget_pending_transcription(
        &app,
        Path::new(&mp3),
        outcome.mp3.clone(),
    ) {
        log::info!(
            "transcribe: retargeted queued transcription from {} to {}",
            mp3,
            outcome.mp3.display()
        );
    }
    Ok(RenamedPayload {
        mp3: outcome.mp3.to_string_lossy().into_owned(),
        note: outcome.note.map(|p| p.to_string_lossy().into_owned()),
        warning: outcome.warning,
    })
}

/// Tray menu variants: failures only log — there is no panel to show them.
pub fn pause_from_menu(app: &AppHandle) {
    if let Err(e) = set_paused(app, true) {
        log::warn!("pause from tray: {e}");
    }
}

pub fn resume_from_menu(app: &AppHandle) {
    if let Err(e) = set_paused(app, false) {
        log::warn!("resume from tray: {e}");
    }
}

/// Every shutdown path funnels through here so quitting mid-meeting saves
/// the capture through the normal stop flow instead of stranding a .part.
/// Callers must NOT be on the main/event-loop thread (the wait is
/// unbounded); tray::quit and the CloseRequested handler spawn a worker
/// thread for it.
pub fn finalize_if_recording(app: &AppHandle) {
    if is_recording(app) {
        log::info!("capture: finalizing active recording before shutdown");
        // Unbounded: quitting must block until the save lands — exiting
        // on a timeout would kill the worker and strand the .part.
        let _ = request_stop_and_wait(app, None);
    }
}

/// Startup recovery over every discovered vault's effective recording
/// root; pending work (fresh orphans, or a pass postponed by an active
/// recording) retries every 90s, bounded at ~24h of attempts.
pub fn run_recovery(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("capture-recovery".into())
        .spawn(move || {
            let pass = |stale: Duration| -> bool {
                // A live recording's .part should never be caught by a recovery
                // pass in practice: a clock jump could give the live .part a
                // future mtime that makes it look stale, and it would be
                // "recovered" out from under the encoder. recover_root has no
                // notion of the active session, so postpone the whole pass
                // while a recording is active — returning true keeps the pass
                // retrying every 90s while work is pending, rather than only
                // running again at next launch. Coarse, but safe in practice
                // (this is-recording check runs once per pass, not once per
                // file recover_root scans).
                {
                    let state = app.state::<CaptureState>();
                    let guard = lock_ignoring_poison(&state.0);
                    if let Some(active) = guard.as_ref() {
                        // Build the message while still holding the guard,
                        // then drop it before the synchronous file-log write
                        // — the state mutex must never be held across I/O.
                        let live_part = active
                            .part
                            .as_deref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "not yet reserved".to_string());
                        drop(guard);
                        log::info!(
                            "recovery: postponed while a recording is active (live part: {live_part})"
                        );
                        return true;
                    }
                }
                let cfg = capture_config::load_config();
                let mut fresh_found = false;
                for vault in discovery::discover_vaults() {
                    let v = capture_config::vault_config(&cfg, &vault.id);
                    // Configured folder, or BOTH mode defaults when no config
                    // entry exists — a first-ever crash may have used either.
                    for folder in v.recording_roots() {
                        let Ok(root) =
                            capture_paths::safe_recording_root(Path::new(&vault.path), folder)
                        else {
                            log::warn!("recovery: skipping unsafe configured folder {folder:?}");
                            continue;
                        };
                        if !root.is_dir() {
                            continue;
                        }
                        if let Err(e) =
                            capture_paths::assert_root_inside_vault(Path::new(&vault.path), &root)
                        {
                            log::warn!("recovery: skipping root: {e}");
                            continue;
                        }
                        for action in vault_buddy_capture::recovery::recover_root(
                            &root,
                            &vault.name,
                            stale,
                            v.create_note,
                            v.transcribe,
                        ) {
                            use vault_buddy_capture::recovery::RecoveryAction;
                            match action {
                                RecoveryAction::Recovered { mp3 } => {
                                    let name = mp3
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    toast(&app, "Recording recovered", &name);
                                    if v.transcribe {
                                        if let Err(e) =
                                            vault_buddy_core::transcript::write_placeholder(&mp3)
                                        {
                                            log::warn!(
                                                "transcribe: writing placeholder for {} failed: {e}",
                                                mp3.display()
                                            );
                                        }
                                        enqueue_transcription(
                                            &app,
                                            TranscriptionJob {
                                                mp3,
                                                vault_id: vault.id.clone(),
                                                force: false,
                                            },
                                        );
                                    }
                                }
                                RecoveryAction::Fresh(_) => fresh_found = true,
                                RecoveryAction::DeletedEmpty(_) => {}
                            }
                        }
                    }
                }
                fresh_found
            };
            // Retry while work is pending (fresh orphans aging, or passes
            // postponed by an active recording). Bounded so a pathological
            // state cannot spin forever: 960 × 90s ≈ 24h of retries, far
            // beyond any realistic recording session; recovery also reruns
            // on every app launch.
            let mut retries = 0u32;
            while pass(Duration::from_secs(60)) {
                retries += 1;
                if retries >= 960 {
                    log::warn!("recovery: giving up rescans after {retries} attempts");
                    break;
                }
                std::thread::sleep(Duration::from_secs(90));
            }
        })
        .expect("failed to spawn capture-recovery thread");
}

/// Shared by `open_transcript` and `open_recording`: launch an
/// `obsidian://open` for a recording's companion note `<base>.md` when it
/// exists (the richest view — it embeds the transcript and the audio player),
/// otherwise the `<base>.transcript.md` sidecar. Read-only: never writes into
/// the vault; the launch is logged by `uri::launch`, the same audit trail as
/// every other vault open.
fn open_recording_note(path: &str) -> Result<(), String> {
    let mp3 = PathBuf::from(path);
    let vaults = discovery::discover_vaults();
    // Canonical containment (GAP-01's read-only sibling): the lexical
    // starts_with accepted `..`/symlink paths pointing outside every vault.
    let owned = capture_paths::vault_owning_path(&vaults, &mp3)
        .ok_or_else(|| "Recording is not inside a known vault.".to_string())?;
    let note = owned.path_canonical.with_extension("md");
    let target = if note.exists() {
        note
    } else {
        transcript::transcript_path(&owned.path_canonical)
    };
    // Both sides canonical, so strip_prefix agrees on Windows' \\?\ form
    // (the open_task precedent).
    let rel = uri::vault_relative_no_ext(&target, &owned.vault_canonical).ok_or_else(|| {
        log::warn!(
            "open_recording_note: {} resolved outside its vault",
            target.display()
        );
        "Recording is outside its vault.".to_string()
    })?;
    uri::launch(&uri::open_file_uri(&owned.vault.id, &rel))
}

/// Open a finished recording's note (or transcript sidecar) — the
/// Transcriptions.vue "Open in Obsidian" row for a finished job.
#[tauri::command]
pub fn open_transcript(path: String) -> Result<(), String> {
    open_recording_note(&path)
}

/// Open a recording's note from the Recordings list row.
#[tauri::command]
pub fn open_recording(path: String) -> Result<(), String> {
    open_recording_note(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(startup_wedged: bool, part: Option<PathBuf>) -> ActiveCapture {
        let (control_tx, _rx) = mpsc::channel::<Control>();
        // _rx dropped: sends become no-ops, which these pure-predicate
        // tests never exercise anyway.
        ActiveCapture {
            control_tx,
            vault_id: "v".to_string(),
            started_at_ms: 0,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            part,
            startup_wedged,
        }
    }

    #[test]
    fn shutdown_bypasses_only_a_wedged_startup_with_nothing_on_disk() {
        // GAP-08: a wedged device open kept is_recording true forever —
        // quit blocked forever, hide_buddy no-op'd forever, every Alt+F4
        // spawned another permanently blocked close-finalize thread. The
        // bypass must fire for exactly that state and nothing else.
        assert!(bypasses_shutdown_wait(&active(true, None)));
    }

    #[test]
    fn shutdown_still_waits_for_any_recording_that_reached_disk() {
        // Never-lose-audio: once a .part exists, wait-forever stands — even
        // if the wedged flag was set (belt and suspenders: the janitor
        // records a late worker's part, closing the bypass mid-drain).
        assert!(!bypasses_shutdown_wait(&active(
            true,
            Some(PathBuf::from(".x.mp3.part"))
        )));
        assert!(!bypasses_shutdown_wait(&active(
            false,
            Some(PathBuf::from(".x.mp3.part"))
        )));
    }

    #[test]
    fn shutdown_waits_for_a_normal_still_starting_recording() {
        // part=None WITHOUT the wedged flag is an ordinary start in its
        // first ten seconds — not bypassable.
        assert!(!bypasses_shutdown_wait(&active(false, None)));
    }

    // GAP-20: the moved commands must be async — this only compiles when
    // stop_capture returns a Future (fn-pointer bound, no runtime needed).
    #[allow(dead_code)]
    fn stop_capture_is_async() {
        fn takes_async<F: std::future::Future>(_: fn(AppHandle) -> F) {}
        takes_async(stop_capture);
    }

    // GAP-21: start_capture must be async — compiles only when the command
    // returns a Future.
    #[allow(dead_code)]
    fn start_capture_is_async() {
        fn takes_async<F: std::future::Future>(_: fn(AppHandle, String, Option<String>) -> F) {}
        takes_async(start_capture);
    }

    // GAP-22: the read-only list commands must be async (blocking fs/COM
    // work belongs on the blocking pool, not the main thread).
    #[allow(dead_code)]
    fn list_commands_are_async() {
        fn takes_async1<F: std::future::Future>(_: fn(String) -> F) {}
        fn takes_async0<F: std::future::Future>(_: fn() -> F) {}
        takes_async1(list_recordings);
        takes_async0(list_audio_devices);
    }

    #[test]
    fn stop_outcome_maps_timeout_to_still_saving() {
        // GAP-20 (related-low): the sync command returned a bare Ok(()) on
        // the 15 s timeout, so the frontend saw success while the recording
        // was still finalizing. The typed mapping is the fix's contract.
        assert!(StopOutcomeDto::from_wait(StopWait::TimedOut).still_saving);
        assert!(!StopOutcomeDto::from_wait(StopWait::Cleared).still_saving);
    }
}
