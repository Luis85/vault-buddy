use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

/// Hide the companion (and its panel/bubble); the tray "Show / Hide" brings
/// the buddy back.
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
    for label in ["panel", "bubble", "main"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
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
        std::thread::Builder::new()
            .name("shutdown-finalize".into())
            .spawn(move || {
                crate::capture_commands::finalize_if_recording(&app);
                finish_quit(&app);
            })
            .expect("failed to spawn shutdown-finalize thread");
        return;
    }
    finish_quit(app);
}

/// Final shutdown steps, shared by the immediate path and the
/// finalize-first worker thread above. The window tail is marshalled onto
/// the MAIN thread: saving window state takes the window-state plugin's
/// cache lock and then reads window geometry, and the plugin's Moved
/// listener takes the same lock on the main thread — run from the
/// shutdown-finalize worker while the user drags the still-visible buddy,
/// the two would wedge forever (the same deadlock that froze the app
/// mid-drag from the old off-main checkpoint save). From a menu callback
/// this executes inline (already on the main thread), so the immediate
/// path behaves exactly as before.
fn finish_quit(app: &AppHandle) {
    log::info!("clean shutdown (quit)");
    crate::diagnostics::mark_clean_shutdown();
    let app2 = app.clone();
    let posted = app.run_on_main_thread(move || {
        // app.exit bypasses window destruction, which is what the window-state
        // plugin normally saves on — save explicitly. Log a failure: the
        // process is dead moments later, so this line is the only evidence a
        // buddy that respawns at a stale position leaves behind (the update
        // path logs the same failure for the same reason).
        if let Err(e) = app2.save_window_state(StateFlags::POSITION) {
            log::error!("quit: saving window state failed: {e}");
        }
        // Destroy EVERY webview before exiting so WebView2 can unregister the
        // shared `Chrome_WidgetWin_0` window class. All three windows share
        // that class; leaving even one alive fails the unregister with
        // ERROR_CLASS_HAS_WINDOWS (1412), logged as
        // "Failed to unregister class Chrome_WidgetWin_0" on shutdown.
        for label in ["panel", "bubble", "main"] {
            if let Some(window) = app2.get_webview_window(label) {
                let _ = window.destroy();
            }
        }
        app2.exit(0);
    });
    if posted.is_err() {
        // Event loop already gone — nothing left to save through; the clean
        // marker is stamped, so just end the process.
        log::warn!("quit: main thread unreachable — exiting directly");
        std::process::exit(0);
    }
}

/// Recording indicator states the tray can show. Paused is deliberately
/// its own visual (steady amber vs. red) — a user glancing at the tray
/// must be able to tell "capturing audio right now" from "not capturing,
/// but a session is open".
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayCaptureState {
    Idle,
    Recording,
    Paused,
}

/// Programmatic 32×32 RGBA icon: the buddy's violet disc, plus a red
/// recording dot (or amber when paused) — no asset pipeline needed for a
/// state that is pure signal.
fn buddy_icon(state: TrayCaptureState) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE / 2) as i32;
    let dot: Option<[u8; 4]> = match state {
        TrayCaptureState::Idle => None,
        TrayCaptureState::Recording => Some([0xe0, 0x2e, 0x2e, 0xff]), // red
        TrayCaptureState::Paused => Some([0xf5, 0x9e, 0x0b, 0xff]),    // amber
    };
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let idx = ((y as u32 * SIZE + x as u32) * 4) as usize;
            let dx = x - center;
            let dy = y - center;
            if dx * dx + dy * dy <= (center - 2) * (center - 2) {
                rgba[idx..idx + 4].copy_from_slice(&[0x7c, 0x5c, 0xff, 0xff]);
            }
            if let Some(color) = dot {
                // dot bottom-right
                let rx = x - (SIZE as i32 - 9);
                let ry = y - (SIZE as i32 - 9);
                if rx * rx + ry * ry <= 36 {
                    rgba[idx..idx + 4].copy_from_slice(&color);
                }
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE, SIZE)
}

fn tray_menu(app: &AppHandle, state: TrayCaptureState) -> tauri::Result<Menu<tauri::Wry>> {
    let active = state != TrayCaptureState::Idle;
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", !active, None::<&str>)?;
    let logs = MenuItem::with_id(app, "open-logs", "Open logs folder", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    if active {
        let pause_resume = if state == TrayCaptureState::Paused {
            MenuItem::with_id(
                app,
                "tray-resume-recording",
                "▶ Resume recording",
                true,
                None::<&str>,
            )?
        } else {
            MenuItem::with_id(
                app,
                "tray-pause-recording",
                "⏸ Pause recording",
                true,
                None::<&str>,
            )?
        };
        let stop = MenuItem::with_id(
            app,
            "tray-stop-recording",
            "⏹ Stop recording",
            true,
            None::<&str>,
        )?;
        Menu::with_items(app, &[&pause_resume, &stop, &toggle, &logs, &quit_item])
    } else {
        Menu::with_items(app, &[&toggle, &logs, &quit_item])
    }
}

/// Swap the tray icon, tooltip, and menu to reflect capture state. Called
/// on start, pause, resume, and finish (successful or not).
pub fn set_capture_state(app: &AppHandle, state: TrayCaptureState) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_icon(Some(buddy_icon(state)));
        let _ = tray.set_tooltip(Some(match state {
            TrayCaptureState::Idle => "Vault Buddy",
            TrayCaptureState::Recording => "Vault Buddy — recording",
            TrayCaptureState::Paused => "Vault Buddy — paused",
        }));
        if let Ok(menu) = tray_menu(app, state) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = tray_menu(app, TrayCaptureState::Idle)?;

    TrayIconBuilder::with_id("main-tray")
        .icon(buddy_icon(TrayCaptureState::Idle))
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
                std::thread::Builder::new()
                    .name("tray-stop".into())
                    .spawn(move || {
                        crate::capture_commands::stop_from_menu(&app);
                    })
                    .expect("failed to spawn tray-stop thread");
            }
            "tray-pause-recording" => crate::capture_commands::pause_from_menu(app),
            "tray-resume-recording" => crate::capture_commands::resume_from_menu(app),
            "open-logs" => crate::diagnostics::open_log_dir(app),
            "quit" => quit(app),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
