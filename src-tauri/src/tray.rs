use crate::commands::PanelOffset;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, PhysicalPosition,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

/// Hide the companion; the tray "Show / Hide" brings it back.
///
/// THE single hide chokepoint: the buddy is the recording indicator, and
/// hiding it mid-capture would violate the spec's no-hidden-recordings
/// requirement — so this is the only place allowed to call window.hide()
/// on the buddy, and any future hide path must route through here to
/// inherit the guard.
pub fn hide_buddy(app: &AppHandle) {
    if crate::capture_commands::is_recording(app) {
        log::info!("hide ignored: recording in progress");
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// If the panel is open the frontend may have shifted the window to unfold
/// toward free screen space; move back to the unshifted home position so
/// that is what any position save persists. Takes the offset (zeroing it)
/// so running both the close handler and the quit path can't double-add.
pub fn restore_home_position(app: &AppHandle) {
    let (dx, dy) = std::mem::take(&mut *app.state::<PanelOffset>().0.lock().unwrap());
    if (dx, dy) != (0, 0) {
        if let Some(window) = app.get_webview_window("main") {
            if let Ok(pos) = window.outer_position() {
                let _ = window.set_position(PhysicalPosition::new(pos.x + dx, pos.y + dy));
            }
        }
    }
}

/// Persist the buddy's home position and exit. Shared by the tray menu and
/// the buddy's right-click menu.
pub fn quit(app: &AppHandle) {
    // Mid-meeting quits must save through the normal stop flow, not strand
    // a .part. But finalizing can take arbitrarily long (slow vault, stuck
    // fsync) and this runs inside a native menu callback — waiting here
    // would freeze the event loop (dead tray, dead buddy) for the whole
    // encode. Park the wait on a worker thread and let it drive the exit
    // once the save has landed; the menu callback returns immediately.
    if crate::capture_commands::is_recording(app) {
        let app = app.clone();
        std::thread::spawn(move || {
            crate::capture_commands::finalize_if_recording(&app);
            finish_quit(&app);
        });
        return;
    }
    finish_quit(app);
}

/// Final shutdown steps, shared by the immediate path and the
/// finalize-first worker thread above. Safe to run off the main thread:
/// in Tauri 2, Window::destroy and AppHandle::exit proxy their work to the
/// event loop (tauri-runtime-wry sends destroy through the event-loop
/// proxy) rather than requiring it, and the window-state plugin reads
/// positions through the same thread-safe dispatchers.
fn finish_quit(app: &AppHandle) {
    restore_home_position(app);
    // app.exit bypasses window destruction, which is what the window-state
    // plugin normally saves on — save explicitly.
    let _ = app.save_window_state(StateFlags::POSITION);
    // Destroy the webview before exiting so WebView2 can unregister its
    // window class in order — otherwise dev consoles log a harmless
    // "Failed to unregister class Chrome_WidgetWin_0" (ERROR_CLASS_HAS_WINDOWS).
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
    }
    app.exit(0);
}

/// Programmatic 32×32 RGBA icon: the buddy's violet disc, plus a red
/// recording dot when active — no asset pipeline needed for a state that
/// is pure signal.
fn buddy_icon(recording: bool) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE / 2) as i32;
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let idx = ((y as u32 * SIZE + x as u32) * 4) as usize;
            let dx = x - center;
            let dy = y - center;
            if dx * dx + dy * dy <= (center - 2) * (center - 2) {
                rgba[idx..idx + 4].copy_from_slice(&[0x7c, 0x5c, 0xff, 0xff]);
            }
            if recording {
                // red dot bottom-right
                let rx = x - (SIZE as i32 - 9);
                let ry = y - (SIZE as i32 - 9);
                if rx * rx + ry * ry <= 36 {
                    rgba[idx..idx + 4].copy_from_slice(&[0xe0, 0x2e, 0x2e, 0xff]);
                }
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE, SIZE)
}

fn tray_menu(app: &AppHandle, recording: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", !recording, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    if recording {
        let stop = MenuItem::with_id(
            app,
            "tray-stop-recording",
            "⏹ Stop recording",
            true,
            None::<&str>,
        )?;
        Menu::with_items(app, &[&stop, &toggle, &quit_item])
    } else {
        Menu::with_items(app, &[&toggle, &quit_item])
    }
}

/// Swap the tray icon, tooltip, and menu to reflect recording state. Called
/// when a capture starts and when it finishes (successfully or not).
pub fn set_recording(app: &AppHandle, recording: bool) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_icon(Some(buddy_icon(recording)));
        let _ = tray.set_tooltip(Some(if recording {
            "Vault Buddy — recording"
        } else {
            "Vault Buddy"
        }));
        if let Ok(menu) = tray_menu(app, recording) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = tray_menu(app, false)?;

    TrayIconBuilder::with_id("main-tray")
        .icon(buddy_icon(false))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(true) {
                        // hide_buddy carries the recording guard; the item
                        // being disabled while recording (tray_menu) is
                        // belt-and-suspenders on top of it.
                        hide_buddy(app);
                    } else {
                        let _ = window.show();
                    }
                }
            }
            "tray-stop-recording" => {
                // Stopping waits up to 15s for the finalize — never block
                // the menu callback (and the event loop) on it.
                let app = app.clone();
                std::thread::spawn(move || {
                    crate::capture_commands::stop_from_menu(&app);
                });
            }
            "quit" => quit(app),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
