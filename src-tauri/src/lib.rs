mod capture_commands;
mod commands;
mod tray;

use tauri::{Emitter, Manager};

pub fn run() {
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
        .plugin(tauri_plugin_log::Builder::new().build())
        // Remember where the user parked the buddy across restarts. Only the
        // position: the window size is managed dynamically by the panel
        // open/close logic and must never be restored from disk.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
                .build(),
        )
        // In-app updates: the settings panel checks GitHub Releases'
        // latest.json and installs signed updates; process gives it the
        // relaunch after install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(commands::PanelOffset::default())
        .manage(capture_commands::CaptureState::default())
        .manage(capture_commands::ConfigWriteLock::default())
        // Alt+F4 / session shutdown destroy the window without going through
        // tray::quit, and the window-state plugin saves POSITION on
        // destruction — restore the unshifted home position first so a
        // panel-open-at-the-edge close can't persist the shifted point.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
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
                    std::thread::spawn(move || {
                        capture_commands::finalize_if_recording(&app);
                        // The recording is finalized, so is_recording is
                        // now false and the re-triggered CloseRequested
                        // takes the else branch below (restore + pass
                        // through to destruction) — no loop.
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.close();
                        }
                    });
                } else {
                    tray::restore_home_position(app);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_vaults,
            commands::open_vault,
            commands::open_daily_note,
            commands::prepare_update_install,
            commands::set_panel_offset,
            commands::set_window_geometry,
            commands::show_buddy_menu,
            capture_commands::start_capture,
            capture_commands::stop_capture,
            capture_commands::capture_status,
            capture_commands::get_capture_config,
            capture_commands::set_capture_config,
            capture_commands::list_audio_devices
        ])
        .setup(|app| {
            tray::create_tray(app.handle())?;
            capture_commands::run_recovery(app.handle());
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
            // steals focus.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                use tauri_plugin_window_state::{AppHandleExt, StateFlags};
                // Checkpoint of the buddy's parked position. Exit paths that
                // save state can silently fail or be bypassed (the updater
                // kills the process via std::process::exit) — persisting
                // within a second of any move means the state file always
                // holds a recent correct position, whatever the exit path.
                let mut last_pos: Option<(i32, i32)> = None;
                let mut ticks: u32 = 0;
                let mut saved_once = false;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    ticks = ticks.saturating_add(1);
                    let Some(window) = handle.get_webview_window("main") else {
                        continue;
                    };
                    if !window.is_visible().unwrap_or(false) {
                        continue;
                    }
                    let _ = window.set_always_on_top(true);
                    // Never persist while the panel has the window shifted —
                    // only the unshifted home position may reach disk.
                    let offset = *handle.state::<commands::PanelOffset>().0.lock().unwrap();
                    if offset != (0, 0) {
                        continue;
                    }
                    if let Ok(pos) = window.outer_position() {
                        let pos = (pos.x, pos.y);
                        let moved = last_pos.is_some() && last_pos != Some(pos);
                        // The early ticks must not write: a save that lands
                        // before the window-state plugin's restore would
                        // poison its cache with the pre-restore default
                        // position. But a drag within that window would be
                        // absorbed into the baseline and lost until the next
                        // move — so once restore has certainly landed, one
                        // unconditional save persists whatever the baseline
                        // is. Any successful save (moved or initial) counts.
                        let initial = !saved_once && ticks >= 3;
                        if moved || initial {
                            match handle.save_window_state(StateFlags::POSITION) {
                                Ok(()) => saved_once = true,
                                Err(e) => log::warn!("position checkpoint failed: {e}"),
                            }
                        }
                        last_pos = Some(pos);
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Vault Buddy");
}
