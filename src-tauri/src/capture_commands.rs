use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vault_buddy_capture::session::{CaptureSession, Control, Outcome, SessionParams};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery, recordings, transcript, uri};
use vault_buddy_transcribe::engine::WhisperTranscriber;
use vault_buddy_transcribe::model::{download_model, model_path, ModelTier};
use vault_buddy_transcribe::{
    transcribe_recording, CancelToken, TranscribeError, TranscribeOptions,
};

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
}

/// The mutex holds the active-capture reservation; the condvar is notified
/// whenever the reservation is cleared so stop-waiters block on it instead
/// of polling (see `request_stop_and_wait` / `clear_active`).
#[derive(Default)]
pub struct CaptureState(pub Mutex<Option<ActiveCapture>>, pub Condvar);

#[derive(Clone)]
struct TranscriptionJob {
    mp3: PathBuf,
    vault_id: String,
    force: bool,
}

/// A coarse stage for the job currently being processed, surfaced to the UI
/// (Task 4) via `as_str()`. `Downloading` carries live byte counts — it's the
/// only stage with a percentage of its own before inference's 0-100 progress
/// starts.
#[derive(Clone)]
enum Phase {
    Downloading { received: u64, total: Option<u64> },
    Preparing,
    Transcribing,
}
impl Phase {
    fn as_str(&self) -> &'static str {
        match self {
            Phase::Downloading { .. } => "downloading",
            Phase::Preparing => "preparing",
            Phase::Transcribing => "transcribing",
        }
    }
}

/// The job currently being processed, published under the queue mutex so a
/// future cancel command (Task 4) always has a `CancelToken` to flip.
/// `progress` is written lock-free (`Ordering::Relaxed`) from the whisper
/// progress callback in `process_transcription` — that callback must never
/// take the queue mutex, only this atomic plus `app.emit`.
struct ActiveJob {
    mp3: PathBuf,
    vault_id: String,
    cancel: CancelToken,
    started_at_ms: u64,
    phase: Phase,
    progress: Arc<AtomicU8>, // 0..100 inference %, written lock-free from the callback
}

#[derive(Default)]
struct TranscriptionQueue {
    pending: VecDeque<TranscriptionJob>,
    /// Paths currently queued or in flight — dedupes the save-time enqueue
    /// against the startup/late-recovery scans.
    known: HashSet<PathBuf>,
    /// The job the worker is presently on; None between jobs and at idle.
    active: Option<ActiveJob>,
}

/// Background transcription queue. One worker (see `run_transcription`)
/// drains it, yielding to any active recording so inference never steals
/// CPU from live capture.
#[derive(Default)]
pub struct TranscriptionState {
    inner: Mutex<TranscriptionQueue>,
    cv: Condvar,
}

fn enqueue_transcription(app: &AppHandle, job: TranscriptionJob) {
    let state = app.state::<TranscriptionState>();
    let mut guard = state.inner.lock().unwrap();
    if guard.known.insert(job.mp3.clone()) {
        log::info!("transcribe: queued {}", job.mp3.display());
        guard.pending.push_back(job);
        state.cv.notify_all();
    }
}

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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn toast(app: &AppHandle, title: &str, body: &str) {
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
        }),
    );
    // Source-loss warnings were already emitted live via warn_tx; here the
    // outcome only feeds the note metadata and the ended-early toast copy.
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
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
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

#[tauri::command]
pub fn list_audio_devices() -> DeviceListDto {
    let list = vault_buddy_capture::devices::list_devices();
    let map = |d: vault_buddy_capture::devices::DeviceInfo| DeviceInfoDto {
        name: d.name,
        is_default: d.is_default,
    };
    DeviceListDto {
        inputs: list.inputs.into_iter().map(map).collect(),
        outputs: list.outputs.into_iter().map(map).collect(),
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    pub mp3: String,
    pub title: String,
    pub recorded_at: String,
    pub duration: Option<String>,
    // `type` is a Rust keyword — expose the camelCase `type` the frontend wants.
    #[serde(rename = "type")]
    pub recording_type: Option<String>,
    pub transcript_status: String,
}

/// Read-only list of a vault's past recordings for the Recordings view.
/// Scans the vault's recording roots (custom folder, or both mode defaults)
/// and reads each recording's companion note for type/duration. An unknown
/// vault or unreadable roots yield an empty list — never an error (mirrors
/// discovery's degrade-to-empty rule). Never writes into the vault.
#[tauri::command]
pub fn list_recordings(id: String) -> Vec<RecordingDto> {
    let Some(vault) = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
    else {
        return Vec::new();
    };
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    // No swallowed error: a rejected (unsafe) folder is skipped WITH a warning,
    // matching run_recovery/scan_and_enqueue — a silent filter_map would hide it.
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in cfg.recording_roots() {
        let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), folder) else {
            log::warn!("list_recordings: skipping unsafe recording folder {folder:?}");
            continue;
        };
        roots.push(root);
    }
    recordings::list_recordings(&roots)
        .into_iter()
        .map(|e| RecordingDto {
            mp3: e.mp3_path.to_string_lossy().into_owned(),
            title: e.title,
            recorded_at: e.recorded_at,
            duration: e.duration,
            recording_type: e.recording_type,
            transcript_status: e.transcript_status.as_dto_str().to_string(),
        })
        .collect()
}

#[tauri::command]
pub fn start_capture(
    app: AppHandle,
    state: tauri::State<CaptureState>,
    id: String,
    mode: Option<String>,
) -> Result<StatusPayload, String> {
    // Everything fallible-but-cheap (discovery, config, path validation)
    // runs BEFORE the state lock is touched — the mutex must never be held
    // across file I/O or device setup.
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let vault_path = PathBuf::from(&vault.path);
    if !vault_path.is_dir() {
        return Err(format!("Vault folder not found: {}", vault.path));
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
        });
    }

    let vault_name = vault.name.clone();
    let vault_path2 = vault_path.clone();

    // Live source-loss warnings: forwarded to the panel while recording.
    let (warn_tx, warn_rx) = mpsc::channel::<String>();
    let app_warn = app.clone();
    std::thread::Builder::new()
        .name("capture-warn".into())
        .spawn(move || {
            while let Ok(message) = warn_rx.recv() {
                let _ = app_warn.emit("capture:warning", serde_json::json!({ "message": message }));
            }
        })
        .expect("failed to spawn capture-warn thread");

    // Advisory level meter: forward the worker's ~5 Hz peaks to the panel.
    let (level_tx, level_rx) = mpsc::channel::<f32>();
    let app_level = app.clone();
    std::thread::Builder::new()
        .name("capture-level".into())
        .spawn(move || {
            while let Ok(peak) = level_rx.recv() {
                let _ = app_level.emit("capture:level", serde_json::json!({ "peak": peak }));
            }
        })
        .expect("failed to spawn capture-level thread");

    std::thread::Builder::new()
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
        })
        .expect("failed to spawn capture-device thread");

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
            clear_active(&app);
            emit_failed(&app, &e);
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
            let app4 = app.clone();
            std::thread::Builder::new()
                .name("capture-janitor".into())
                .spawn(move || {
                    if let Ok(Ok(part)) = ready_rx.recv() {
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
                })
                .expect("failed to spawn capture-janitor thread");
            emit_failed(&app, &msg);
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
    std::thread::Builder::new()
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
        })
        .expect("failed to spawn capture-monitor thread");

    // Indicator hardening: recording buddy must be visible.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
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
    let _ = vault_buddy_core::transcript::write_placeholder(mp3);
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
fn request_stop_and_wait(app: &AppHandle, wait: Option<Duration>) {
    // Bound to a local so the guard below can borrow it across statements —
    // `app.state::<CaptureState>()` is otherwise a temporary that would be
    // dropped at the end of the `let guard = …;` statement.
    let capture_state = app.state::<CaptureState>();
    let mut guard = lock_ignoring_poison(&capture_state.0);
    let Some(active) = guard.as_ref() else { return };
    let _ = active.control_tx.send(Control::Stop);
    let deadline = wait.map(|limit| std::time::Instant::now() + limit);
    while guard.is_some() {
        match deadline {
            Some(deadline) => {
                let now = std::time::Instant::now();
                if now >= deadline {
                    log::warn!("capture: stop wait timed out");
                    return;
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
}

#[tauri::command]
pub fn stop_capture(app: AppHandle, state: tauri::State<CaptureState>) -> Result<(), String> {
    if lock_ignoring_poison(&state.0).is_none() {
        return Err("No recording is running.".to_string());
    }
    request_stop_and_wait(&app, Some(Duration::from_secs(15)));
    Ok(())
}

/// Stop triggered from a native menu (tray or buddy) rather than the panel.
pub fn stop_from_menu(app: &AppHandle) {
    request_stop_and_wait(app, Some(std::time::Duration::from_secs(15)));
}

pub fn is_recording(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<CaptureState>().0).is_some()
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
    // rename_plan re-validates ownership (capture-pattern stems only), so
    // an arbitrary user mp3 can never be renamed through this command.
    let plan = capture_paths::rename_plan(Path::new(&mp3), &title)?;
    if !plan.mp3_from.is_file() {
        return Err("Recording file not found — was it moved?".to_string());
    }
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
        request_stop_and_wait(app, None);
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
                                        let _ =
                                            vault_buddy_core::transcript::write_placeholder(&mp3);
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

/// Startup + on-demand worker: drains the transcription queue, postponing
/// while a recording is active. The loaded whisper model is cached across
/// jobs of the same tier. Mirrors `run_recovery`'s shape (own thread, coarse
/// is-recording gate).
pub fn run_transcription(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("transcribe-worker".into())
        .spawn(move || {
            // Backfill: transcribe anything already on disk missing a transcript
            // (previous-session saves, crash-recovered captures, freshly enabled
            // vaults).
            scan_and_enqueue(&app);
            let mut loaded: Option<(ModelTier, WhisperTranscriber)> = None;
            loop {
                // Block until a job is available; peek without claiming it.
                let job = {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = state.inner.lock().unwrap();
                    while guard.pending.is_empty() {
                        guard = state.cv.wait(guard).unwrap();
                    }
                    guard.pending.front().cloned().unwrap()
                };
                // Never contend with a live recording for CPU — re-check soon.
                if is_recording(&app) {
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
                {
                    let state = app.state::<TranscriptionState>();
                    state.inner.lock().unwrap().pending.pop_front();
                }
                process_transcription(&app, &job, &mut loaded);
                // Drop from the dedupe set: success leaves a `complete` sidecar
                // (won't rescan); failure leaves a `failed` one (a later
                // launch's scan or a manual retry re-queues it). Clear the
                // active-job slot the same way, under the same lock — this
                // runs after every `process_transcription` return path
                // (success, failure, or an early-return on a bad model/load),
                // so it's the one place that needs to clear `active`.
                {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = state.inner.lock().unwrap();
                    guard.known.remove(&job.mp3);
                    guard.active = None;
                }
            }
        })
        .expect("failed to spawn transcribe-worker thread");
}

/// Enqueue every capture recording still needing a transcript, across all
/// vaults that opted in. Same root discipline as `run_recovery`.
fn scan_and_enqueue(app: &AppHandle) {
    let cfg = capture_config::load_config();
    for vault in discovery::discover_vaults() {
        let v = capture_config::vault_config(&cfg, &vault.id);
        if !v.transcribe {
            continue;
        }
        for folder in v.recording_roots() {
            let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), folder)
            else {
                continue;
            };
            if !root.is_dir() {
                continue;
            }
            if capture_paths::assert_root_inside_vault(Path::new(&vault.path), &root).is_err() {
                continue;
            }
            for mp3 in vault_buddy_core::transcript::pending_transcriptions(&root) {
                enqueue_transcription(
                    app,
                    TranscriptionJob {
                        mp3,
                        vault_id: vault.id.clone(),
                        force: false,
                    },
                );
            }
        }
    }
}

fn process_transcription(
    app: &AppHandle,
    job: &TranscriptionJob,
    loaded: &mut Option<(ModelTier, WhisperTranscriber)>,
) {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &job.vault_id);
    // A forced (explicit) re-transcribe ignores the vault's auto-transcribe
    // setting; the automatic path still bails when disabled.
    if !cfg.transcribe && !job.force {
        return;
    }
    let tier = ModelTier::from_str(&cfg.transcription_model);

    // Publish the active job BEFORE any observable work starts, so a future
    // cancel command (Task 4) always has a token to flip. The mutex hold is
    // brief — just the insert — never across the download/model-load/
    // inference that follows. `cancel`/`progress` are kept as locals too
    // (not re-read from `active` later) — they share state with the clones
    // stored below via CancelToken's/Arc's Clone, so either handle works.
    let started_at_ms = now_ms();
    let cancel = CancelToken::new();
    let progress = Arc::new(AtomicU8::new(0));
    {
        let state = app.state::<TranscriptionState>();
        let mut guard = state.inner.lock().unwrap();
        guard.active = Some(ActiveJob {
            mp3: job.mp3.clone(),
            vault_id: job.vault_id.clone(),
            cancel: cancel.clone(),
            started_at_ms,
            phase: Phase::Preparing,
            progress: progress.clone(),
        });
    }
    let _ = app.emit(
        "capture:transcribing",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy(), "vaultId": job.vault_id }),
    );
    if job.force {
        // Reflect the in-flight regeneration in the note embed by swapping our
        // own regenerable sidecar for the "transcribing…" placeholder — but
        // NEVER overwrite a Complete/hand-edited transcript up-front. If this
        // forced job then fails, the original must survive: fail_transcription
        // writes via replace_if_ours, which skips a non-regenerable sidecar, so
        // leaving it untouched means a failed re-transcribe can't destroy it.
        // On success, transcribe_recording's force_write_sidecar swaps the
        // finished transcript for the freshly generated one.
        if vault_buddy_core::transcript::transcript_status(&job.mp3)
            != vault_buddy_core::transcript::TranscriptStatus::Complete
        {
            let name = job
                .mp3
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let _ = vault_buddy_core::transcript::force_write_sidecar(
                &vault_buddy_core::transcript::transcript_path(&job.mp3),
                &vault_buddy_core::transcript::render_placeholder(&name),
            );
        }
    } else {
        let _ = vault_buddy_core::transcript::write_placeholder(&job.mp3);
    }

    let model = match ensure_model(app, tier, &job.mp3) {
        Ok(p) => p,
        Err(e) => return fail_transcription(app, &job.mp3, &format!("model unavailable: {e}")),
    };
    // Handover: the model is on disk now (just downloaded, or already
    // present) — replace the download row with "preparing" BEFORE the
    // model-load gap below, so a download UI can never stick at 100%.
    {
        let state = app.state::<TranscriptionState>();
        let mut guard = state.inner.lock().unwrap();
        if let Some(active) = guard.active.as_mut() {
            active.phase = Phase::Preparing;
        }
    }
    let _ = app.emit(
        "capture:modelReady",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy() }),
    );

    if loaded.as_ref().map(|(t, _)| *t) != Some(tier) {
        match WhisperTranscriber::load(&model) {
            Ok(w) => *loaded = Some((tier, w)),
            Err(e) => return fail_transcription(app, &job.mp3, &e),
        }
    }
    let transcriber = &loaded.as_ref().unwrap().1;
    let opts = TranscribeOptions {
        language: cfg.transcription_language.clone(),
        timestamps: cfg.transcript_timestamps,
        model_label: tier.label(),
    };
    let generated_at = chrono::Local::now().to_rfc3339();

    {
        let state = app.state::<TranscriptionState>();
        let mut guard = state.inner.lock().unwrap();
        if let Some(active) = guard.active.as_mut() {
            active.phase = Phase::Transcribing;
        }
    }
    let _ = app.emit(
        "capture:transcribeProgress",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy(), "progress": 0 }),
    );
    let app_cb = app.clone();
    let mp3_cb = job.mp3.clone();
    let mut last_sent: i32 = -1;
    let mut last_logged: i32 = -1;
    let on_progress: Box<dyn FnMut(i32) + Send> = Box::new(move |p| {
        progress.store(p.clamp(0, 100) as u8, Ordering::Relaxed); // lock-free, no queue mutex
        if p - last_sent >= 5 || p >= 100 {
            // throttled UI event
            last_sent = p;
            let _ = app_cb.emit(
                "capture:transcribeProgress",
                serde_json::json!({ "mp3": mp3_cb.to_string_lossy(), "progress": p }),
            );
        }
        if p - last_logged >= 25 || p >= 100 {
            // honest log: coarse periodic progress
            last_logged = p;
            log::info!("transcribe: {} inference {}%", mp3_cb.display(), p);
        }
    });
    // (inference start/elapsed with audio length is logged inside
    // transcribe_recording — Task 2 — which owns the samples.)
    let result = transcribe_recording(
        &job.mp3,
        transcriber,
        &opts,
        &generated_at,
        job.force,
        &cancel,
        on_progress,
    );
    match result {
        Ok(path) => {
            log::info!("transcribe: wrote {}", path.display());
            let _ = app.emit(
                "capture:transcribed",
                serde_json::json!({
                    "mp3": job.mp3.to_string_lossy(),
                    "transcript": path.to_string_lossy(),
                }),
            );
        }
        Err(TranscribeError::Failed(e)) => fail_transcription(app, &job.mp3, &e),
        Err(TranscribeError::Cancelled) => {
            // Our own placeholder → cancelled; never a complete/user file.
            let name = job
                .mp3
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let _ = vault_buddy_core::transcript::replace_if_ours(
                &vault_buddy_core::transcript::transcript_path(&job.mp3),
                &vault_buddy_core::transcript::render_cancelled(&name),
            );
            let _ = app.emit(
                "capture:transcribeCancelled",
                serde_json::json!({ "mp3": job.mp3.to_string_lossy() }),
            );
            log::info!("transcribe: cancelled {}", job.mp3.display());
        }
    }
}

/// Ensure the tier's model is on disk, downloading with progress if not.
/// `mp3` identifies the job in the download-progress event and in the
/// `active` phase this sets while a download is running (Task 4 reads
/// both) — it names nothing else here.
fn ensure_model(app: &AppHandle, tier: ModelTier, mp3: &Path) -> Result<PathBuf, String> {
    if let Some(p) = model_path(tier) {
        if p.exists() {
            return Ok(p);
        }
    }
    log::info!("transcribe: downloading model {}", tier.as_str());
    let app = app.clone();
    let mp3 = mp3.to_path_buf();
    let mut last_emit: u64 = 0;
    download_model(tier, &mut |received, total| {
        // Throttle: an event every ~4 MB (and the final byte).
        if received.saturating_sub(last_emit) >= 4_000_000 || Some(received) == total {
            last_emit = received;
            // Phase update under the mutex is brief (one field write); the
            // emit itself happens after the guard drops below.
            {
                let state = app.state::<TranscriptionState>();
                let mut guard = state.inner.lock().unwrap();
                if let Some(active) = guard.active.as_mut() {
                    active.phase = Phase::Downloading { received, total };
                }
            }
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({
                    "mp3": mp3.to_string_lossy(),
                    "model": tier.as_str(),
                    "received": received,
                    "total": total,
                }),
            );
        }
    })
}

/// Best-effort failure: leave the audio + note untouched, replace the
/// sidecar with a retryable `failed` note, and surface it.
fn fail_transcription(app: &AppHandle, mp3: &Path, message: &str) {
    log::warn!("transcribe: {} failed: {message}", mp3.display());
    let name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = vault_buddy_core::transcript::transcript_path(mp3);
    let content = vault_buddy_core::transcript::render_error(&name, message);
    let _ = vault_buddy_core::transcript::replace_if_ours(&path, &content);
    let _ = app.emit(
        "capture:transcribeFailed",
        serde_json::json!({ "mp3": mp3.to_string_lossy(), "message": message }),
    );
    toast(app, "Transcription failed", message);
}

/// The vault whose folder contains `mp3` (for the retry command).
fn owning_vault_id(mp3: &Path) -> Option<String> {
    discovery::discover_vaults()
        .into_iter()
        .find(|v| mp3.starts_with(&v.path))
        .map(|v| v.id)
}

/// Retry / on-demand transcription of a specific recording.
#[tauri::command]
pub fn transcribe_recording_now(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    if !mp3.is_file() {
        return Err("Recording not found.".to_string());
    }
    let vault_id = owning_vault_id(&mp3).ok_or("Recording is not inside a known vault.")?;
    enqueue_transcription(
        &app,
        TranscriptionJob {
            mp3,
            vault_id,
            force: false,
        },
    );
    Ok(())
}

/// Explicit, forced re-transcription of a specific recording: regenerates even
/// a finished transcript and ignores the vault's auto-transcribe setting.
#[tauri::command]
pub fn retranscribe(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    if !mp3.is_file() {
        return Err("Recording not found.".to_string());
    }
    let vault_id = owning_vault_id(&mp3).ok_or("Recording is not inside a known vault.")?;
    enqueue_transcription(
        &app,
        TranscriptionJob {
            mp3,
            vault_id,
            force: true,
        },
    );
    Ok(())
}

/// Shared by `open_transcript` and `open_recording`: launch an
/// `obsidian://open` for a recording's companion note `<base>.md` when it
/// exists (the richest view — it embeds the transcript and the audio player),
/// otherwise the `<base>.transcript.md` sidecar. Read-only: never writes into
/// the vault; the launch is logged by `uri::launch`, the same audit trail as
/// every other vault open.
fn open_recording_note(path: &str) -> Result<(), String> {
    let mp3 = PathBuf::from(path);
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| mp3.starts_with(&v.path))
        .ok_or_else(|| format!("no vault owns {path}"))?;
    let note = mp3.with_extension("md");
    let target = if note.exists() {
        note
    } else {
        transcript::transcript_path(&mp3)
    };
    let rel = uri::vault_relative_no_ext(&target, Path::new(&vault.path))
        .ok_or_else(|| format!("recording is outside its vault: {}", target.display()))?;
    uri::launch(&uri::open_file_uri(&vault.id, &rel))
}

/// Open a finished recording's note (or transcript sidecar) — the
/// TranscriptionStatus "Open in Obsidian" row.
#[tauri::command]
pub fn open_transcript(path: String) -> Result<(), String> {
    open_recording_note(&path)
}

/// Open a recording's note from the Recordings list row.
#[tauri::command]
pub fn open_recording(path: String) -> Result<(), String> {
    open_recording_note(&path)
}
