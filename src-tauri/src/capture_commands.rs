use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vault_buddy_capture::session::{CaptureSession, Outcome, SessionParams};
use vault_buddy_core::{capture_config, capture_paths, discovery};

pub enum StopReason {
    User,
}

pub struct ActiveCapture {
    pub stop_tx: Sender<StopReason>,
    pub vault_id: String,
    pub started_at_ms: u64,
}

#[derive(Default)]
pub struct CaptureState(pub Mutex<Option<ActiveCapture>>);

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
    let mut guard = state.0.lock().unwrap();
    if guard.is_some() {
        return Err("A recording is already running.".to_string());
    }

    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let vault_path = std::path::PathBuf::from(&vault.path);
    if !vault_path.is_dir() {
        return Err(format!("Vault folder not found: {}", vault.path));
    }

    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let meeting = cfg.mode == capture_config::RecordingMode::Meeting;
    let label = cfg.mode.label();

    // Device validation happens on the worker thread BEFORE any file is
    // created (spec: start failures stay file-free).
    let (stop_tx, stop_rx) = mpsc::channel::<StopReason>();
    let (done_tx, done_rx) = mpsc::channel::<Result<Outcome, String>>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let app2 = app.clone();
    let vault_name = vault.name.clone();
    // Hand-editable config must never escape the vault (PRD guarantee).
    let root = capture_paths::safe_recording_root(&vault_path, cfg.effective_recording_folder())?;

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
        let open = match vault_buddy_capture::devices::open_sources(meeting) {
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
        let _ = ready_tx.send(Ok(()));

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

    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = app2.emit(
                "capture:failed",
                serde_json::json!({ "message": e.clone() }),
            );
            return Err(e);
        }
        Err(_) => {
            // Startup hung (e.g. a wedged audio driver). The worker may
            // still create the .part and start the session AFTER this
            // return — never leave that recording detached: a janitor
            // thread waits for the worker's one ready signal and, if a
            // session did start, stops it and surfaces the outcome so no
            // audio is silently stranded.
            let msg = "Recording did not start in time.".to_string();
            let app4 = app2.clone();
            std::thread::spawn(move || {
                if let Ok(Ok(())) = ready_rx.recv() {
                    log::warn!("capture: late start after timeout — stopping and draining");
                    let _ = stop_tx.send(StopReason::User);
                    match done_rx.recv() {
                        Ok(Ok(outcome)) => emit_saved(&app4, &outcome),
                        Ok(Err(e)) => log::warn!("capture: late-start cleanup failed: {e}"),
                        Err(_) => log::warn!("capture: late-start cleanup: worker vanished"),
                    }
                }
                // worker replied Err (or vanished): nothing was created.
            });
            let _ = app2.emit(
                "capture:failed",
                serde_json::json!({ "message": msg.clone() }),
            );
            return Err(msg);
        }
    }

    let payload = StatusPayload {
        recording: true,
        vault_id: Some(id.clone()),
        started_at_ms: Some(now_ms()),
    };
    *guard = Some(ActiveCapture {
        stop_tx,
        vault_id: id,
        started_at_ms: payload.started_at_ms.unwrap(),
    });
    drop(guard);

    // Monitor thread: the ONLY consumer of the session outcome. Covers
    // user/menu/shutdown stops AND self-finalization (all sources lost) —
    // the state clears and the outcome surfaces no matter who ended it.
    let app3 = app.clone();
    std::thread::spawn(move || {
        let result = done_rx
            .recv()
            .unwrap_or_else(|_| Err("capture thread vanished".to_string()));
        *app3.state::<CaptureState>().0.lock().unwrap() = None;
        match result {
            Ok(outcome) => emit_saved(&app3, &outcome),
            Err(e) => {
                log::error!("capture: finalize failed: {e}");
                let _ = app3.emit(
                    "capture:failed",
                    serde_json::json!({ "message": e.clone() }),
                );
                toast(&app3, "Recording failed", &e);
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

/// Ask the device thread to stop and wait until the monitor thread has
/// cleared the state (i.e. the outcome landed and events were emitted).
/// `wait: None` means wait forever — shutdown paths use it so the app can
/// never exit while a recording is still finalizing (a slow vault or a
/// stuck fsync must not strand the capture as .part).
fn request_stop_and_wait(app: &AppHandle, wait: Option<Duration>) {
    // Bound to a local so the guard below can borrow it across statements —
    // `app.state::<CaptureState>()` is otherwise a temporary that would be
    // dropped at the end of the `let guard = …;` statement.
    let capture_state = app.state::<CaptureState>();
    let stop_tx = {
        let guard = capture_state.0.lock().unwrap();
        guard.as_ref().map(|active| active.stop_tx.clone())
    };
    let Some(stop_tx) = stop_tx else { return };
    let _ = stop_tx.send(StopReason::User);
    let started = std::time::Instant::now();
    let mut last_log = std::time::Instant::now();
    loop {
        if capture_state.0.lock().unwrap().is_none() {
            return;
        }
        if let Some(limit) = wait {
            if started.elapsed() >= limit {
                log::warn!("capture: stop wait timed out");
                return;
            }
        }
        if last_log.elapsed() >= Duration::from_secs(15) {
            log::warn!("capture: still finalizing…");
            last_log = std::time::Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
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
pub fn finalize_if_recording(app: &AppHandle) {
    if is_recording(app) {
        log::info!("capture: finalizing active recording before shutdown");
        // Unbounded: quitting must block until the save lands — exiting
        // on a timeout would kill the worker and strand the .part.
        request_stop_and_wait(app, None);
    }
}

/// Startup recovery over every discovered vault's effective recording
/// root; fresh orphans trigger one rescan after the staleness window.
pub fn run_recovery(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let pass = |stale: Duration| -> bool {
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
                    let Ok(root) = capture_paths::safe_recording_root(
                        std::path::Path::new(&vault.path),
                        &folder,
                    ) else {
                        log::warn!("recovery: skipping unsafe configured folder {folder:?}");
                        continue;
                    };
                    if !root.is_dir() {
                        continue;
                    }
                    if let Err(e) = capture_paths::assert_root_inside_vault(
                        std::path::Path::new(&vault.path),
                        &root,
                    ) {
                        log::warn!("recovery: skipping root: {e}");
                        continue;
                    }
                    for action in vault_buddy_capture::recovery::recover_root(
                        &root,
                        &vault.name,
                        stale,
                        v.create_note,
                    ) {
                        use vault_buddy_capture::recovery::RecoveryAction;
                        match action {
                            RecoveryAction::Recovered { mp3 } => {
                                let name = mp3
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                toast(&app, "Recording recovered", &name);
                            }
                            RecoveryAction::Fresh(_) => fresh_found = true,
                            RecoveryAction::DeletedEmpty(_) => {}
                        }
                    }
                }
            }
            fresh_found
        };
        if pass(Duration::from_secs(60)) {
            std::thread::sleep(Duration::from_secs(90));
            pass(Duration::from_secs(60));
        }
    });
}
