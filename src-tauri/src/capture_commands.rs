use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vault_buddy_capture::session::{CaptureSession, Outcome, SessionParams};
use vault_buddy_core::{capture_config, capture_paths, discovery};
use vault_buddy_transcribe::engine::WhisperTranscriber;
use vault_buddy_transcribe::model::{download_model, model_path, ModelTier};
use vault_buddy_transcribe::{transcribe_recording, TranscribeOptions};

pub enum StopReason {
    User,
}

pub struct ActiveCapture {
    pub stop_tx: Sender<StopReason>,
    pub vault_id: String,
    pub started_at_ms: u64,
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
}

#[derive(Default)]
struct TranscriptionQueue {
    pending: VecDeque<TranscriptionJob>,
    /// Paths currently queued or in flight — dedupes the save-time enqueue
    /// against the startup/late-recovery scans.
    known: HashSet<PathBuf>,
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
    let _ = app.emit("capture:failed", serde_json::json!({ "message": message }));
    toast(app, "Recording failed", message);
}

/// Clear the active-capture reservation and wake everyone blocked in
/// `request_stop_and_wait`. Every site that resets the state to None must
/// go through here, or stop-waiters sleep until their next timeout.
fn clear_active(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    *state.0.lock().unwrap() = None;
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
    let guard = state.0.lock().unwrap();
    match guard.as_ref() {
        Some(active) => StatusPayload {
            recording: true,
            vault_id: Some(active.vault_id.clone()),
            started_at_ms: Some(active.started_at_ms),
        },
        None => StatusPayload {
            recording: false,
            vault_id: None,
            started_at_ms: None,
        },
    }
}

#[tauri::command]
pub fn start_capture(
    app: AppHandle,
    state: tauri::State<CaptureState>,
    id: String,
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

    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let uses_loopback = cfg.mode.uses_loopback();
    let label = cfg.mode.label();
    // Hand-editable config must never escape the vault (PRD guarantee).
    let root = capture_paths::safe_recording_root(&vault_path, cfg.effective_recording_folder())?;

    // Device validation happens on the worker thread BEFORE any file is
    // created (spec: start failures stay file-free).
    let (stop_tx, stop_rx) = mpsc::channel::<StopReason>();
    let (done_tx, done_rx) = mpsc::channel::<Result<Outcome, String>>();
    // Ok carries the reserved .part path so the reservation below learns
    // which file the live session owns.
    let (ready_tx, ready_rx) = mpsc::channel::<Result<PathBuf, String>>();

    // Reserve the state up front: the lock is held only for the is-running
    // check plus the insert, which closes the double-start window without
    // serializing device setup (or any I/O) under the mutex.
    {
        let mut guard = state.0.lock().unwrap();
        if guard.is_some() {
            return Err("A recording is already running.".to_string());
        }
        *guard = Some(ActiveCapture {
            stop_tx: stop_tx.clone(),
            vault_id: id.clone(),
            started_at_ms: now_ms(),
            part: None,
        });
    }

    let vault_name = vault.name.clone();
    let vault_path2 = vault_path.clone();

    // Live source-loss warnings: forwarded to the panel while recording.
    let (warn_tx, warn_rx) = mpsc::channel::<String>();
    let app_warn = app.clone();
    std::thread::spawn(move || {
        while let Ok(message) = warn_rx.recv() {
            let _ = app_warn.emit("capture:warning", serde_json::json!({ "message": message }));
        }
    });

    std::thread::spawn(move || {
        let open = match vault_buddy_capture::devices::open_sources(uses_loopback) {
            Ok(o) => o,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
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
            recorded_at: now.to_rfc3339(),
            flush_every: Duration::from_secs(1),
            fsync_every: Duration::from_secs(30),
            warn_tx: Some(warn_tx),
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

        // Own the streams here; poll for user stop or self-finalization.
        let streams = open.streams;
        loop {
            match stop_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(StopReason::User) | Err(RecvTimeoutError::Disconnected) => break,
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

    // Wait for device readiness WITHOUT the lock — concurrent starts are
    // already rejected by the reservation above.
    let started_at_ms = match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(part)) => {
            // Recording is live: pin the .part the session owns (recovery
            // must never treat it as an orphan) and restamp the start time
            // now that device setup is done — that is what the UI timer
            // should count from.
            let started_at_ms = now_ms();
            if let Some(active) = state.0.lock().unwrap().as_mut() {
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
            let _ = stop_tx.send(StopReason::User);
            let app4 = app.clone();
            std::thread::spawn(move || {
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
                crate::tray::set_recording(&app4, false);
            });
            emit_failed(&app, &msg);
            return Err(msg);
        }
    };

    let monitor_vault_id = id.clone();
    let payload = StatusPayload {
        recording: true,
        vault_id: Some(id),
        started_at_ms: Some(started_at_ms),
    };

    // Monitor thread: the ONLY consumer of the session outcome. Covers
    // user/menu/shutdown stops AND self-finalization (all sources lost) —
    // the state clears and the outcome surfaces no matter who ended it.
    let app3 = app.clone();
    std::thread::spawn(move || {
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
        crate::tray::set_recording(&app3, false);
    });

    // Indicator hardening: recording buddy must be visible.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
    crate::tray::set_recording(&app, true);
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
    let mut guard = capture_state.0.lock().unwrap();
    let Some(active) = guard.as_ref() else { return };
    let _ = active.stop_tx.send(StopReason::User);
    let deadline = wait.map(|limit| std::time::Instant::now() + limit);
    while guard.is_some() {
        match deadline {
            Some(deadline) => {
                let now = std::time::Instant::now();
                if now >= deadline {
                    log::warn!("capture: stop wait timed out");
                    return;
                }
                guard = capture_state
                    .1
                    .wait_timeout(guard, deadline - now)
                    .unwrap()
                    .0;
            }
            None => {
                // Unbounded waits still wake every 15s, purely to keep the
                // "still finalizing…" heartbeat in the shutdown logs.
                let (g, timeout) = capture_state
                    .1
                    .wait_timeout(guard, Duration::from_secs(15))
                    .unwrap();
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
    if state.0.lock().unwrap().is_none() {
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
    app.state::<CaptureState>().0.lock().unwrap().is_some()
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
    std::thread::spawn(move || {
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
                let guard = state.0.lock().unwrap();
                if let Some(active) = guard.as_ref() {
                    log::info!(
                        "recovery: postponed while a recording is active (live part: {})",
                        active
                            .part
                            .as_deref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "not yet reserved".to_string())
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
                let roots: Vec<String> = match &v.recording_folder {
                    Some(folder) => vec![folder.clone()],
                    None => vec!["Meetings".to_string(), "Voice Notes".to_string()],
                };
                for folder in roots {
                    let Ok(root) =
                        capture_paths::safe_recording_root(Path::new(&vault.path), &folder)
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
                                    let _ = vault_buddy_core::transcript::write_placeholder(&mp3);
                                    enqueue_transcription(
                                        &app,
                                        TranscriptionJob {
                                            mp3,
                                            vault_id: vault.id.clone(),
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
    });
}

/// Startup + on-demand worker: drains the transcription queue, postponing
/// while a recording is active. The loaded whisper model is cached across
/// jobs of the same tier. Mirrors `run_recovery`'s shape (own thread, coarse
/// is-recording gate).
pub fn run_transcription(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
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
            // (won't rescan); failure leaves a `failed` one (a later launch's
            // scan or a manual retry re-queues it).
            {
                let state = app.state::<TranscriptionState>();
                state.inner.lock().unwrap().known.remove(&job.mp3);
            }
        }
    });
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
        let roots: Vec<String> = match &v.recording_folder {
            Some(folder) => vec![folder.clone()],
            None => vec!["Meetings".to_string(), "Voice Notes".to_string()],
        };
        for folder in roots {
            let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), &folder)
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
    if !cfg.transcribe {
        return; // disabled since it was queued
    }
    let tier = ModelTier::from_str(&cfg.transcription_model);
    let _ = app.emit(
        "capture:transcribing",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy() }),
    );
    let _ = vault_buddy_core::transcript::write_placeholder(&job.mp3);

    let model = match ensure_model(app, tier) {
        Ok(p) => p,
        Err(e) => return fail_transcription(app, &job.mp3, &format!("model unavailable: {e}")),
    };
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
    match transcribe_recording(&job.mp3, transcriber, &opts, &generated_at) {
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
        Err(e) => fail_transcription(app, &job.mp3, &e),
    }
}

/// Ensure the tier's model is on disk, downloading with progress if not.
fn ensure_model(app: &AppHandle, tier: ModelTier) -> Result<PathBuf, String> {
    if let Some(p) = model_path(tier) {
        if p.exists() {
            return Ok(p);
        }
    }
    log::info!("transcribe: downloading model {}", tier.as_str());
    let app = app.clone();
    let mut last_emit: u64 = 0;
    download_model(tier, &mut |received, total| {
        // Throttle: an event every ~4 MB (and the final byte).
        if received.saturating_sub(last_emit) >= 4_000_000 || Some(received) == total {
            last_emit = received;
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({ "model": tier.as_str(), "received": received, "total": total }),
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
    enqueue_transcription(&app, TranscriptionJob { mp3, vault_id });
    Ok(())
}
