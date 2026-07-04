mod commands;
mod tray;

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
            commands::set_panel_offset
        ])
        .setup(|app| {
            tray::create_tray(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Vault Buddy");
}
