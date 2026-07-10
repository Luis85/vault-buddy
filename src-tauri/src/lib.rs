mod capture_commands;
mod commands;
mod diagnostics;
// pub: the detect_pandoc command + PandocStatus DTO aren't wired into
// generate_handler! yet (that's Task 6 of the document-import slice) — pub
// keeps them part of the crate's public API so dead_code doesn't fire on
// code that is genuinely about to be used, not actually dead.
pub mod document_commands;
mod mcp_commands;
mod search_commands;
mod task_commands;
mod transcription;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vault_buddy_core::sync_util::lock_ignoring_poison;

/// How long the window must sit still before the upkeep tick may touch it.
/// A live OS move loop floods the main thread with Moved events; window
/// work colliding with that flood is what used to deadlock the app
/// (see `window_upkeep_tick`).
const QUIESCE_MS: u64 = 2_000;

/// Consecutive unserviced upkeep ticks before the watchdog reports the main
/// thread as wedged. Upkeep closures are normally dispatched within the
/// second — even during a drag, whose modal loop still pumps posted work.
const MAIN_THREAD_STALL_TICKS: u32 = 10;

/// Instant of the last window Moved event; `None` until the window first
/// moves. Stamped by the window-event hook on the main thread, read by the
/// upkeep tick so every window touch stays away from a window in motion.
/// A plain `Mutex<Option<Instant>>` (the codebase's shared-state idiom — see
/// `MARKER_GATE`) rather than a hand-encoded atomic sentinel.
static LAST_MOVE: Mutex<Option<Instant>> = Mutex::new(None);

fn stamp_window_moved() {
    *lock_ignoring_poison(&LAST_MOVE) = Some(Instant::now());
}

fn ms_since_last_move() -> Option<u64> {
    // Copy the Option out of the guard (Instant is Copy) before mapping —
    // Option::map takes self by value and can't move out of a MutexGuard.
    (*lock_ignoring_poison(&LAST_MOVE)).map(|at| at.elapsed().as_millis() as u64)
}

/// Instant until which the panel's focus-out check must NOT hide the panel.
/// Stamped by a Ctrl-open (`open_search_result` with `keep_open`): Obsidian
/// grabs foreground focus while handling the `obsidian://` URI, which blurs
/// the panel and would close it moments after the user explicitly asked it to
/// stay up for multi-open. The pin expires on its own; the check still only
/// ever HIDES — a fresh pin merely declines one hide — so it can never fight
/// `toggle_panel` into a reopen. Written by a sync command and read by a
/// `run_on_main_thread` closure (both main thread); the Mutex is the
/// codebase's shared-state idiom (see `LAST_MOVE`), not a cross-thread need.
static PANEL_PIN_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// How long a Ctrl-open holds the panel against the focus-out check.
/// Obsidian's foreground grab lands well under a second after the URI
/// launch; a few seconds absorbs a slow cold start without making the panel
/// feel stuck open on ordinary click-aways afterwards.
const PANEL_PIN_MS: u64 = 3_000;

/// Pin the panel open across Obsidian's imminent focus grab (Ctrl-open).
pub(crate) fn pin_panel_open() {
    *lock_ignoring_poison(&PANEL_PIN_UNTIL) =
        Some(Instant::now() + std::time::Duration::from_millis(PANEL_PIN_MS));
}

/// True while a Ctrl-open pin is fresh — the focus-out check consults this.
fn panel_pinned_open() -> bool {
    matches!(
        *lock_ignoring_poison(&PANEL_PIN_UNTIL),
        Some(until) if Instant::now() < until
    )
}

/// Hide the panel once focus has really left the app. Clicking from the panel
/// to the buddy (or back) fires the source window's blur BEFORE the
/// destination's focus lands, so a check run at blur time would see neither
/// window focused and wrongly hide a panel that is merely handing focus to the
/// buddy. The check must therefore be deferred until focus settles.
///
/// It cannot be deferred with `run_on_main_thread` alone: that runs the closure
/// INLINE when called from the main thread, and window events are dispatched on
/// the main thread — so the closure would run synchronously inside the blur
/// event, before focus settles. A real delay on a worker thread is required;
/// only then is the check marshaled back to the main thread (where window
/// getters/`hide` are valid). The check only ever HIDES — never shows — so it
/// can never fight `toggle_panel` into a reopen: a buddy click that closes the
/// panel via `toggle_panel` leaves this deferred check a no-op.
fn schedule_focus_out_check(app: &tauri::AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("focus-out-check".into())
        .spawn(move || {
            // Let the OS focus transition (WM_KILLFOCUS → WM_SETFOCUS) complete
            // before sampling focus. Imperceptible to the user; a click-away
            // just closes the panel a fraction of a second later.
            std::thread::sleep(std::time::Duration::from_millis(120));
            let checked = app.clone();
            let _ = app.run_on_main_thread(move || {
                use tauri::Manager;
                let focused = |label: &str| {
                    checked
                        .get_webview_window(label)
                        .and_then(|w| w.is_focused().ok())
                        .unwrap_or(false)
                };
                if !focused("main") && !focused("panel") {
                    // A fresh Ctrl-open pin means this blur IS Obsidian's
                    // foreground grab from the URI the user just launched —
                    // decline the hide, the user asked the panel to stay for
                    // multi-open. (Only-hide invariant intact: a pin never
                    // shows anything.)
                    if panel_pinned_open() {
                        return;
                    }
                    if let Some(panel) = checked.get_webview_window("panel") {
                        if panel.is_visible().unwrap_or(false) {
                            let _ = panel.hide();
                        }
                    }
                }
            });
        });
    // This runs inside the window-event handler on the main thread; a panic
    // there aborts across the WebView2 FFI boundary. A spawn failure must not
    // do that — dropping one click-away check is harmless (the panel's next
    // blur reschedules), so log it instead of `.expect`.
    if let Err(e) = spawned {
        log::warn!("could not spawn focus-out-check thread: {e}");
    }
}

/// Show the greeting bubble beside the buddy, a beat after launch so the buddy
/// is visibly settled before it greets. The buddy's parked position is restored
/// synchronously in `setup` (before this runs), so — unlike the old design —
/// there is nothing to wait out and no need to re-pin the bubble repeatedly: a
/// single settle then show suffices. The frontend pulls both the facing and the
/// bubble anchor on mount (`get_buddy_facing` / `get_bubble_anchor`), so one
/// post-show facing emit is enough to cover a buddy webview that happened to
/// mount before the restore landed. Best-effort: a spawn failure just skips the
/// greeting.
fn schedule_show_bubble(app: &tauri::AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("show-bubble".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let a = app.clone();
            let _ = app.run_on_main_thread(move || {
                commands::show_bubble(&a);
                // The restored position need not have surfaced as a Moved event,
                // so emit the facing once here — the sprite then faces correctly
                // even if the buddy webview mounted before the restore landed.
                commands::emit_buddy_facing(&a);
            });
        });
    if let Err(e) = spawned {
        log::warn!("could not spawn show-bubble thread: {e}");
    }
}

pub fn run() {
    // Before anything else: a panic during builder construction or in any
    // thread should still be captured on disk.
    diagnostics::install_panic_hook();
    // SEH/signal-level net under the panic hook: catches native faults the
    // Rust hook can never see. Installed this early so even plugin/builder
    // construction is covered.
    diagnostics::install_native_crash_handler();

    tauri::Builder::default()
        // Registered first (per the plugin's docs) so a second launch bails
        // before any other plugin runs. Two instances would mean two buddies,
        // two trays, both processes racing the window-state file — and a
        // second recovery scan racing a live recording. The callback runs in
        // the surviving instance: reveal the buddy — a relaunch attempt
        // means the user was looking for it.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        // Recording saved/failed toasts.
        .plugin(tauri_plugin_notification::init())
        // Persist to a rotating file in the app log dir — the bare `.build()`
        // logged only to stdout, which is invisible in a release GUI build,
        // so crashes left no trail. `targets` REPLACES the plugin defaults
        // (which are Stdout + an unnamed LogDir): set them explicitly to
        // Stdout (kept for `tauri dev`) + a single `vault-buddy.log`, so we
        // don't also spawn a second, default-named log file. 5 MB + KeepOne
        // bounds disk while keeping the one rotated-out file that usually
        // holds the crash preceding a restart. Local timestamps so lines
        // match the user's clock.
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("vault-buddy".into()),
                    }),
                ])
                .level(log::LevelFilter::Info)
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build(),
        )
        // Remember where the user parked the buddy across restarts. Only the
        // position: the window size is managed dynamically by the panel
        // open/close logic and must never be restored from disk.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
                // The panel and bubble are transient — positioned fresh beside
                // the buddy every time — so persisting their positions is
                // pointless (it only wrote garbage coords to the state file).
                .with_denylist(&["panel", "bubble"])
                // The plugin's implicit restore of the buddy lands a beat AFTER
                // the visible window is first painted at the OS default — the
                // startup "buddy jumps from the default corner to home" bug.
                // Skip it and restore explicitly in `setup`, before showing.
                .skip_initial_state("main")
                .build(),
        )
        // In-app updates: the settings panel checks GitHub Releases'
        // latest.json and installs signed updates; process gives it the
        // relaunch after install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Launch-at-login registration, surfaced in Buddy settings via the
        // get_autostart/set_autostart commands (registry-backed on Windows).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(capture_commands::CaptureState::default())
        .manage(transcription::TranscriptionState::default())
        .manage(capture_commands::ConfigWriteLock::default())
        .manage(mcp_commands::McpServerState::default())
        // Alt+F4 / session shutdown destroy the window without going through
        // tray::quit, and the window-state plugin saves POSITION on
        // destruction.
        .on_window_event(|window, event| match event {
            // Every move re-arms the upkeep tick's quiescence gate — window
            // work must never collide with a window in motion.
            tauri::WindowEvent::Moved(_) => {
                stamp_window_moved();
                // The buddy faces toward the screen center: a move can carry it
                // across the midline, flipping the sprite (emit is deduped, so
                // this is cheap on the Moved flood). The greeting bubble tracks
                // the buddy too — while visible, reposition it so it stays
                // beside the buddy instead of stranding. Keyed on the buddy
                // window so the bubble's own resulting Moved can't recurse; both
                // run here on the main thread and touch no shared lock, so they
                // cannot recreate the off-main save-vs-Moved deadlock.
                if window.label() == "main" {
                    commands::emit_buddy_facing(window.app_handle());
                    commands::reposition_bubble_if_visible(window.app_handle());
                }
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                if capture_commands::is_recording(app) {
                    // Alt+F4 / session shutdown bypass tray::quit — the
                    // recording must still finalize, but that wait is
                    // unbounded and this callback runs on the event loop:
                    // blocking would freeze the UI for the whole encode.
                    // Hold this close, finalize on a worker thread, then
                    // re-trigger it via the app handle.
                    api.prevent_close();
                    let app = app.clone();
                    std::thread::Builder::new()
                        .name("close-finalize".into())
                        .spawn(move || {
                            capture_commands::finalize_if_recording(&app);
                            // The recording is finalized, so is_recording is
                            // now false and the re-triggered CloseRequested
                            // takes the else branch below (pass through to
                            // destruction) — no loop.
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.close();
                            }
                        })
                        .expect("failed to spawn close-finalize thread");
                } else {
                    // Alt+F4 / session end: the window is about to be
                    // destroyed and the process exits with it.
                    log::info!("clean shutdown (window close)");
                    diagnostics::mark_clean_shutdown();
                }
            }
            // Only the panel's OWN blur can mean "clicked away from the
            // panel". Scheduling on every window's blur spawned a worker
            // thread per blur (the buddy blurs constantly) and, worse, the
            // buddy blurs AS the panel takes focus on open — a check fired
            // from that could hide the just-opened panel before its focus
            // landed. Keying on the panel's blur removes both.
            tauri::WindowEvent::Focused(false) if window.label() == "panel" => {
                schedule_focus_out_check(window.app_handle())
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_vaults,
            commands::open_vault,
            commands::open_daily_note,
            commands::prepare_update_install,
            commands::toggle_panel,
            commands::close_panel,
            commands::close_bubble,
            commands::announce,
            commands::get_buddy_facing,
            commands::get_bubble_anchor,
            commands::start_buddy_drag,
            commands::show_buddy_menu,
            commands::open_logs_folder,
            commands::rearm_crash_detection,
            commands::get_autostart,
            commands::set_autostart,
            capture_commands::start_capture,
            capture_commands::stop_capture,
            capture_commands::capture_status,
            transcription::transcribe_recording_now,
            transcription::retranscribe,
            transcription::cancel_transcription,
            transcription::transcription_queue_status,
            capture_commands::open_transcript,
            capture_commands::list_recordings,
            capture_commands::open_recording,
            capture_commands::get_capture_config,
            capture_commands::set_capture_config,
            capture_commands::list_audio_devices,
            capture_commands::pause_capture,
            capture_commands::resume_capture,
            capture_commands::rename_capture,
            task_commands::get_tasks_config,
            task_commands::set_tasks_config,
            task_commands::list_tasks,
            task_commands::add_task,
            task_commands::set_task_status,
            task_commands::count_open_tasks,
            search_commands::search_vaults,
            search_commands::open_search_result,
            mcp_commands::get_mcp_config,
            mcp_commands::set_mcp_config,
            mcp_commands::regenerate_mcp_token,
        ])
        .setup(|app| {
            // Give the panic hook the real log dir; until now it falls back to
            // the temp dir.
            if let Ok(dir) = app.path().app_log_dir() {
                diagnostics::set_log_dir(dir);
            }
            log::info!(
                "Vault Buddy v{} starting (pid {})",
                env!("CARGO_PKG_VERSION"),
                std::process::id()
            );
            // install_native_crash_handler ran before this logger existed —
            // replay any install failure it stashed, now that logging works.
            diagnostics::report_startup_diagnostics();
            if let Ok(dir) = app.path().app_log_dir() {
                // A panic before setup wrote its record to the temp dir —
                // fold it in where "Open logs folder" points.
                match vault_buddy_core::app_diagnostics::adopt_stray_crash_log(
                    &diagnostics::stray_crash_file(),
                    &dir,
                ) {
                    Ok(true) => log::info!("adopted a pre-setup crash record into crash.log"),
                    Ok(false) => {}
                    Err(e) => log::warn!("could not adopt stray crash log: {e}"),
                }
                // The panic hook only sees Rust panics; the marker catches
                // every other ending too (native fault, kill, power loss).
                if let vault_buddy_core::app_diagnostics::PreviousRun::Unclean(previous) =
                    vault_buddy_core::app_diagnostics::check_previous_run(&dir)
                {
                    // Freshness is judged against the stale marker's mtime,
                    // so this must run before write_running_marker re-stamps.
                    let (headline, body) =
                        if vault_buddy_core::app_diagnostics::crash_record_looks_fresh(&dir) {
                            log::warn!(
                                "previous session did not shut down cleanly ({previous}); \
                                 crash.log holds a matching record"
                            );
                            (
                                "Vault Buddy crashed last time",
                                "Details are in crash.log — tray → Open logs folder",
                            )
                        } else {
                            log::warn!(
                                "previous session did not shut down cleanly ({previous}) and \
                                 no crash record was written — a native fault (graphics/\
                                 WebView2/audio driver) or a kill; the tail of vault-buddy.log \
                                 shows its last moments. For native dumps enable WER LocalDumps."
                            );
                            (
                                "Vault Buddy didn't shut down cleanly",
                                "No crash record was written (native fault or kill) — \
                                 see vault-buddy.log via tray → Open logs folder",
                            )
                        };
                    let _ = app
                        .notification()
                        .builder()
                        .title(headline)
                        .body(body)
                        .show();
                }
                if let Err(e) = vault_buddy_core::app_diagnostics::write_running_marker(
                    &dir,
                    env!("CARGO_PKG_VERSION"),
                ) {
                    log::warn!("could not write the run marker: {e}");
                }
            }
            // Restore the buddy to its parked position and only THEN show it.
            // The window is created hidden (visible:false) and the plugin's
            // implicit restore is skipped (skip_initial_state), so it never
            // paints at the OS default corner first — removing the startup
            // "buddy jumps from the default to home" flash, and letting the
            // greeting (scheduled below) land against the real home position.
            if let Some(main) = app.get_webview_window("main") {
                use tauri_plugin_window_state::{StateFlags, WindowExt};
                if let Err(e) = main.restore_state(StateFlags::POSITION) {
                    log::warn!("could not restore the buddy position: {e}");
                }
                if let Err(e) = main.show() {
                    log::warn!("could not show the buddy window: {e}");
                }
            }
            tray::create_tray(app.handle())?;
            schedule_show_bubble(app.handle());
            capture_commands::run_recovery(app.handle());
            document_commands::run_import_recovery(app.handle());
            transcription::run_transcription(app.handle());
            mcp_commands::start_if_enabled(app.handle());
            // Items of the buddy's right-click popup menu (the tray handles
            // its own menu; ids are distinct so neither handles the other's).
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "buddy-hide" => tray::hide_buddy(app),
                "buddy-quit" => tray::quit(app),
                // the animation/dragging settings live in the frontend
                // (localStorage); hand the toggles back to it
                "buddy-animation" => {
                    let _ = app.emit("buddy-toggle-animation", ());
                }
                "buddy-dragging" => {
                    let _ = app.emit("buddy-toggle-dragging", ());
                }
                _ => {}
            });
            // Windows re-shuffles the topmost band when other topmost
            // windows appear (taskbar previews, flyouts), which can drop the
            // buddy behind the taskbar. No event reaches us when that
            // happens, so periodically re-assert always-on-top — a cheap
            // z-order-only SetWindowPos that never moves, resizes, or
            // steals focus. The same tick checkpoints the buddy's parked
            // position: exit paths that save state can silently fail or be
            // bypassed (the updater kills the process via
            // std::process::exit), so the state file must always hold a
            // recent correct position, whatever the exit path.
            //
            // This thread is a pure metronome: it only sleeps, heartbeats
            // the run marker, and posts the window work to the MAIN thread.
            // It must never touch the window itself — saving window state
            // off-main while the window was being dragged deadlocked the
            // main thread against the window-state plugin's cache lock and
            // froze the whole app (see window_upkeep_tick).
            let handle = app.handle().clone();
            // Held only for state flips, never across a window call — this
            // mutex cannot recreate the lock-plus-blocking-wait pattern the
            // metronome design removes.
            let checkpointer = Arc::new(Mutex::new(
                vault_buddy_core::checkpoint::PositionCheckpointer::new(),
            ));
            let upkeep_pending = Arc::new(AtomicBool::new(false));
            let mut ticks: u32 = 0;
            let mut stalled: u32 = 0;
            std::thread::Builder::new()
                .name("topmost-checkpoint".into())
                .spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    // Isolate the whole tick: a panic in the metronome body
                    // (e.g. a future edit to the heartbeat, or the log
                    // backend panicking while formatting) must not kill this
                    // thread — losing it silently stops the run-marker
                    // heartbeat, always-on-top re-assert, and position
                    // checkpoint for the rest of the session. `continue`
                    // can't cross the closure boundary, so skips are early
                    // returns inside `metronome_tick`.
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        metronome_tick(
                            &handle,
                            &checkpointer,
                            &upkeep_pending,
                            &mut ticks,
                            &mut stalled,
                        );
                    }));
                    if outcome.is_err() {
                        log::error!("metronome tick panicked; continuing");
                    }
                })
                .expect("failed to spawn topmost-checkpoint thread");
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            // Building the app is fatal, but `.expect` would panic with no
            // persisted reason — log it first so the cause survives in the
            // app log. This fires before any run loop exists (that's
            // `.run()` below) — building the app itself failed.
            log::error!("fatal: Tauri app failed to build: {e}");
            std::process::exit(1);
        })
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // Every event-loop exit — whatever future path triggered it —
                // stamps clean. The enumerated stamps on quit/close/update
                // remain for the std::process::exit path that bypasses this.
                log::info!("clean shutdown (event loop exit)");
                diagnostics::mark_clean_shutdown();
            }
        });
}

/// One iteration of the metronome loop: heartbeat the run marker, then post
/// the window work to the main thread with backpressure so at most one
/// upkeep closure is ever outstanding. Split out of the loop so the whole
/// body runs inside `catch_unwind` (skips are early returns, not `continue`).
fn metronome_tick(
    handle: &tauri::AppHandle,
    checkpointer: &Arc<Mutex<vault_buddy_core::checkpoint::PositionCheckpointer>>,
    upkeep_pending: &Arc<AtomicBool>,
    ticks: &mut u32,
    stalled: &mut u32,
) {
    *ticks = ticks.saturating_add(1);
    // Re-stamp the run marker every ~15s, whatever the window or main thread
    // are doing: a hidden buddy or a busy UI is still a running session and
    // must keep heartbeating. This is a backstop once re-armed — see
    // `heartbeat_running_marker`'s doc for why a premature "clean" stamp
    // needs an explicit re-arm, not just this.
    if ticks.is_multiple_of(15) {
        crate::diagnostics::heartbeat_running_marker();
    }
    if upkeep_pending.load(Ordering::Acquire) {
        // The previous tick's closure was never serviced. Don't stack more
        // work behind it; report a wedge once it is clearly not a transient
        // stall — this exact silence used to be an invisible mid-drag
        // deadlock, so it must reach the log.
        *stalled = stalled.saturating_add(1);
        if *stalled == MAIN_THREAD_STALL_TICKS {
            log::error!(
                "main thread has not serviced window upkeep for \
                 ~{MAIN_THREAD_STALL_TICKS}s — the UI may be wedged; \
                 last window move {:?} ms ago",
                ms_since_last_move()
            );
        }
        return;
    }
    if *stalled >= MAIN_THREAD_STALL_TICKS {
        log::info!("main thread responsive again after ~{stalled}s of window-upkeep backlog");
    }
    *stalled = 0;
    upkeep_pending.store(true, Ordering::Release);
    let handle2 = handle.clone();
    let cp = checkpointer.clone();
    let pending = upkeep_pending.clone();
    let posted = handle.run_on_main_thread(move || {
        // A panic here would unwind into the native event loop (a process
        // abort on Windows) — isolate it, and always clear the pending flag
        // so one bad tick can't wedge the backpressure gate forever.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            window_upkeep_tick(&handle2, &cp);
        }));
        if outcome.is_err() {
            log::error!("window upkeep tick panicked; continuing");
        }
        pending.store(false, Ordering::Release);
    });
    if posted.is_err() {
        // Event loop gone (shutdown in progress) — not a stall. Clear the
        // gate so a late tick before teardown doesn't false-report a wedge.
        log::warn!("window upkeep post skipped: event loop unavailable");
        upkeep_pending.store(false, Ordering::Release);
    }
}

/// One round of window upkeep: re-assert always-on-top and checkpoint the
/// parked position.
///
/// MUST run on the main thread. Saving window state takes the window-state
/// plugin's cache lock and then reads window geometry, and the plugin's own
/// Moved listener takes the same lock on the main thread. Run off-main, a
/// save colliding with a drag's Moved flood deadlocked both threads — the
/// background thread held the cache lock while waiting for the main thread
/// to answer a geometry query, the main thread sat in the Moved listener
/// waiting for the cache lock — and the app froze mid-drag with no crash
/// record (nothing panicked, nothing faulted; the frozen process was killed
/// externally). On the main thread the same lock pair is serialized by
/// construction, so the deadlock is gone regardless. The Moved-age gate plus
/// the button-down gate keep even main-thread window work away from a live
/// drag, and only a settled position is ever persisted, so a save never
/// coincides with a move in practice.
fn window_upkeep_tick(
    handle: &tauri::AppHandle,
    checkpointer: &Mutex<vault_buddy_core::checkpoint::PositionCheckpointer>,
) {
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};

    if !vault_buddy_core::checkpoint::is_quiescent(ms_since_last_move(), QUIESCE_MS) {
        return;
    }
    // The Moved-age gate above misses a drag the user pauses for >2s with the
    // button still held (the move loop is live but emits no Moved events). We
    // run on the main thread here, so a direct button-state read is valid and
    // catches exactly that case — never touch a window the user is dragging.
    #[cfg(windows)]
    if commands::primary_button_down() {
        return;
    }
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    // Windows re-shuffles the topmost band when other topmost windows appear
    // (taskbar previews, flyouts), which can drop the buddy behind the
    // taskbar. No event reaches us, so re-assert always-on-top every tick — a
    // cheap z-order-only SetWindowPos that never moves, resizes, or steals
    // focus. Log a failure instead of swallowing it: a persistent failure is
    // how the buddy silently sinks behind the taskbar. (Mid-drag ticks are
    // skipped above, but the window is moving itself then — its z-order
    // cannot be usurped while it owns the move loop.)
    if let Err(e) = window.set_always_on_top(true) {
        log::warn!("always-on-top re-assert failed: {e}");
    }
    if let Ok(pos) = window.outer_position() {
        // The checkpointer defers the first save past the window-state
        // plugin's restore and asks for one only once a changed position has
        // settled; failed writes stay dirty and are retried next tick.
        if lock_ignoring_poison(checkpointer).observe((pos.x, pos.y)) {
            match handle.save_window_state(StateFlags::POSITION) {
                Ok(()) => lock_ignoring_poison(checkpointer).mark_saved(),
                Err(e) => log::warn!("position checkpoint failed: {e}"),
            }
        }
    }
}
