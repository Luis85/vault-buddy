mod commands;
mod tray;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::list_vaults,
            commands::open_vault,
            commands::open_daily_note
        ])
        .setup(|app| {
            tray::create_tray(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Vault Buddy");
}
