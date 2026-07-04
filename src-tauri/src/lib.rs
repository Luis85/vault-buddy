mod commands;
mod tray;

use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        // Registered first (per the plugin's docs) so a second launch bails
        // before any other plugin runs. Two instances would mean two buddies,
        // two trays, and both processes racing the window-state file. The
        // callback runs in the surviving instance: reveal the buddy — a
        // relaunch attempt means the user was looking for it.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
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
        // Alt+F4 / session shutdown destroy the window without going through
        // tray::quit, and the window-state plugin saves POSITION on
        // destruction — restore the unshifted home position first so a
        // panel-open-at-the-edge close can't persist the shifted point.
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                tray::restore_home_position(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_vaults,
            commands::open_vault,
            commands::open_daily_note,
            commands::prepare_update_install,
            commands::set_panel_offset,
            commands::set_window_geometry,
            commands::show_buddy_menu
        ])
        .setup(|app| {
            tray::create_tray(app.handle())?;
            // Items of the buddy's right-click popup menu (the tray handles
            // its own menu; ids are distinct so neither handles the other's).
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "buddy-hide" => tray::hide_to_tray(app),
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
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if let Some(window) = handle.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.set_always_on_top(true);
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Vault Buddy");
}
