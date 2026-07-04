mod commands;
mod tray;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        // Remember where the user parked the buddy across restarts. Only the
        // position: the window size is managed dynamically by the panel
        // open/close logic and must never be restored from disk.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
                .build(),
        )
        .manage(commands::PanelOffset::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_vaults,
            commands::open_vault,
            commands::open_daily_note,
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
