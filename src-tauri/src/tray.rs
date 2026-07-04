use crate::commands::PanelOffset;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, PhysicalPosition,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

/// Hide the companion; the tray "Show / Hide" brings it back.
pub fn hide_to_tray(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// Persist the buddy's home position and exit. Shared by the tray menu and
/// the buddy's right-click menu.
pub fn quit(app: &AppHandle) {
    // If the panel is open the frontend may have shifted the window to
    // unfold toward free screen space; move back to the unshifted home
    // position so that is what gets saved.
    let (dx, dy) = *app.state::<PanelOffset>().0.lock().unwrap();
    if (dx, dy) != (0, 0) {
        if let Some(window) = app.get_webview_window("main") {
            if let Ok(pos) = window.outer_position() {
                let _ = window.set_position(PhysicalPosition::new(pos.x + dx, pos.y + dy));
            }
        }
    }
    // app.exit bypasses window destruction, which is what the window-state
    // plugin normally saves on — save explicitly.
    let _ = app.save_window_state(StateFlags::POSITION);
    app.exit(0);
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &quit_item])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("bundled icon missing"),
        )
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                if let Some(window) = app.get_webview_window("main") {
                    let visible = window.is_visible().unwrap_or(true);
                    let _ = if visible {
                        window.hide()
                    } else {
                        window.show()
                    };
                }
            }
            "quit" => quit(app),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
