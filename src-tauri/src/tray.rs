use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::capture_guard::{CaptureGuard, CaptureKind};

/// Every window label in `tauri.conf.json`, in the order the hide and quit
/// paths walk them — the buddy (`main`) LAST, so the accessory windows never
/// outlive the thing they hang off. Both walks below mean "every window",
/// and a window added to the config but not to a walk reappears as the two
/// bugs those walks exist to prevent (an overlay that survives hide-to-tray;
/// a webview left alive at exit that fails WebView2's class unregister), so
/// a test below derives this list from the config instead of trusting it.
pub const ALL_WINDOW_LABELS: [&str; 4] = ["panel", "bubble", "overlay", "main"];

/// Windows the window-state plugin must NOT persist a position for: every
/// one except the buddy. The panel, bubble and overlay are all positioned
/// fresh — while hidden — every time they are shown, so a restored position
/// is junk that only buys a startup restore and a `Moved` handler holding the
/// plugin's cache lock.
pub const POSITION_DENYLIST: [&str; 3] = ["panel", "bubble", "overlay"];

/// Hide the companion (and its panel/bubble); the tray "Show / Hide" brings
/// the buddy back.
///
/// THE single hide chokepoint: the buddy is the recording indicator, and
/// hiding it mid-capture would violate the spec's no-hidden-recordings
/// requirement — so this is the only place allowed to call window.hide()
/// on the buddy, and any future hide path must route through here to
/// inherit the guard.
pub fn hide_buddy(app: &AppHandle) {
    if crate::capture_commands::recording_blocks_shutdown(app)
        || crate::screen_commands::capture_blocks_shutdown(app)
    {
        log::info!("hide ignored: a capture is in progress");
        return;
    }
    for label in ALL_WINDOW_LABELS {
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
    if crate::capture_commands::recording_blocks_shutdown(app)
        || crate::screen_commands::capture_blocks_shutdown(app)
    {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("shutdown-finalize".into())
            .spawn(move || {
                crate::capture_commands::finalize_if_recording(&app);
                crate::screen_commands::finalize_if_capturing(&app);
                finish_quit(&app);
            });
        if let Err(e) = spawned {
            // Menu callbacks run on the main thread — never panic here. Not
            // quitting is the safe degrade: the recording keeps running and
            // the user can retry Quit.
            log::error!("could not spawn shutdown-finalize thread: {e}");
        }
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
        // shared `Chrome_WidgetWin_0` window class. ALL of them share that
        // class; leaving even one alive fails the unregister with
        // ERROR_CLASS_HAS_WINDOWS (1412), logged as
        // "Failed to unregister class Chrome_WidgetWin_0" on shutdown.
        for label in ALL_WINDOW_LABELS {
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

/// Which capture domain the tray's Pause/Resume/Stop items act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuTarget {
    Audio,
    Screen,
}

/// The tray menu shows ONE set of capture controls, but two domains can
/// have put it into its `active` state (`set_capture_state(Recording)` is
/// called from the audio start path AND from the screen one). Routing them
/// unconditionally to `capture_commands` left the screen case with a stop
/// that returned instantly (no audio reservation) and a pause that only
/// logged — while "Show / Hide" was disabled, so the tray offered nothing
/// that worked. `CaptureGuard` is the one place that knows which domain is
/// live, so the items follow it. Idle falls to audio: the items are not
/// rendered then, and the audio path's "No recording is running." refusal
/// is the right answer to a stale click.
fn menu_target(active: Option<CaptureKind>) -> MenuTarget {
    match active {
        Some(CaptureKind::Screen) => MenuTarget::Screen,
        _ => MenuTarget::Audio,
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
                // Stopping waits for the finalize (15 s audio, 30 s screen)
                // — never block the menu callback (and the event loop) on
                // it. The guard read happens on the worker too: it is O(1)
                // either way, and taking it there keeps the callback free of
                // every lock.
                let app = app.clone();
                let spawned =
                    std::thread::Builder::new()
                        .name("tray-stop".into())
                        .spawn(
                            move || match menu_target(app.state::<CaptureGuard>().active()) {
                                MenuTarget::Screen => crate::screen_commands::stop_from_menu(&app),
                                MenuTarget::Audio => crate::capture_commands::stop_from_menu(&app),
                            },
                        );
                if let Err(e) = spawned {
                    // Dropping one stop request is harmless — the user
                    // retries from the still-visible tray item.
                    log::warn!("could not spawn tray-stop thread: {e}");
                }
            }
            // Pause/resume stay INLINE on the main thread in both domains:
            // each takes its domain's state mutex for O(1) work and sends on
            // an unbounded channel, so neither can block the event loop.
            "tray-pause-recording" => match menu_target(app.state::<CaptureGuard>().active()) {
                MenuTarget::Screen => crate::screen_commands::pause_from_menu(app),
                MenuTarget::Audio => crate::capture_commands::pause_from_menu(app),
            },
            "tray-resume-recording" => match menu_target(app.state::<CaptureGuard>().active()) {
                MenuTarget::Screen => crate::screen_commands::resume_from_menu(app),
                MenuTarget::Audio => crate::capture_commands::resume_from_menu(app),
            },
            "open-logs" => crate::diagnostics::open_log_dir(app),
            "quit" => quit(app),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every window label `tauri.conf.json` declares, in config order.
    fn configured_labels() -> Vec<String> {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        conf["app"]["windows"]
            .as_array()
            .expect("app.windows")
            .iter()
            .map(|w| w["label"].as_str().expect("label").to_string())
            .collect()
    }

    #[test]
    fn the_hide_and_quit_walk_covers_every_configured_window() {
        // The bug this pins, twice over. `hide_buddy` is THE hide chokepoint:
        // a window missing from it survives hide-to-tray, so tray -> Hide
        // would leave a full-monitor, always-on-top, undecorated overlay up
        // with the buddy gone. And `finish_quit` must DESTROY every webview
        // or WebView2 cannot unregister the shared `Chrome_WidgetWin_0`
        // class -- ERROR_CLASS_HAS_WINDOWS (1412) on every single quit.
        // Derived from the config rather than restated, so window number
        // five fails here instead of shipping both bugs again.
        let mut configured = configured_labels();
        let mut walked: Vec<String> = ALL_WINDOW_LABELS.iter().map(|s| s.to_string()).collect();
        configured.sort();
        walked.sort();
        assert_eq!(
            walked, configured,
            "every window in tauri.conf.json must be hidden and destroyed"
        );
        // The buddy goes last: it is the visible anchor, and the accessory
        // windows should never outlive it on screen.
        assert_eq!(ALL_WINDOW_LABELS.last(), Some(&"main"));
    }

    #[test]
    fn every_transparent_window_disables_its_shadow() {
        // A drop shadow on a transparent, undecorated window paints a frame
        // around something that is supposed to have none. The buddy, panel
        // and bubble all turn it off; the overlay shipped without the field
        // (Tauri defaults it ON) purely because spec 5.2's config block omits
        // it. Nobody can verify this on Linux, so pin it in the config.
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        for window in conf["app"]["windows"].as_array().expect("app.windows") {
            if window["transparent"] == serde_json::Value::Bool(true) {
                assert_eq!(
                    window["shadow"],
                    serde_json::Value::Bool(false),
                    "window {} is transparent but leaves `shadow` at Tauri's default",
                    window["label"]
                );
            }
        }
    }

    #[test]
    fn only_the_buddy_persists_a_position() {
        // The window-state plugin restores POSITION for any window NOT on
        // its denylist and installs a `Moved` handler that takes the
        // plugin's cache mutex -- the lock at the origin of the documented
        // drag-crash deadlock. Every window but the buddy is positioned
        // fresh while hidden, so persisting anything for them writes junk
        // coordinates and buys a startup restore that exists to be
        // overwritten.
        let expected: Vec<&str> = ALL_WINDOW_LABELS
            .iter()
            .copied()
            .filter(|l| *l != "main")
            .collect();
        assert_eq!(POSITION_DENYLIST.to_vec(), expected);
        // ...and the builder actually uses the constant, rather than a
        // literal list that would drift from it again.
        let lib = include_str!("lib.rs");
        assert!(
            lib.contains("with_denylist(&crate::tray::POSITION_DENYLIST)"),
            "lib.rs must feed the window-state plugin POSITION_DENYLIST"
        );
    }

    #[test]
    fn no_window_walk_restates_the_label_list() {
        // Structural, because the failure mode is a SECOND list left behind:
        // the overlay was added to tauri.conf.json and to three hard-coded
        // walks it never reached. Both walks in this file must read the one
        // constant.
        let src = include_str!("tray.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        let walks = production.matches("in ALL_WINDOW_LABELS").count();
        assert_eq!(
            walks, 2,
            "expected the hide and quit walks to iterate ALL_WINDOW_LABELS; found {walks}"
        );
        // The two constants are the only places a label may be spelled out.
        let spellings = production.matches("\"bubble\"").count();
        assert_eq!(
            spellings, 2,
            "window labels belong in ALL_WINDOW_LABELS / POSITION_DENYLIST and \
             nowhere else in tray.rs; found {spellings} spellings of \"bubble\""
        );
    }

    #[test]
    fn a_live_screen_capture_routes_the_tray_controls_to_the_screen_domain() {
        // The bug this pins: `set_capture_state(Recording)` from the screen
        // start path activates the tray's "⏸ Pause recording" / "⏹ Stop
        // recording" items AND disables "Show / Hide". Routed to the audio
        // domain they are dead — stop finds no audio reservation and returns
        // immediately, pause only logs an error the user never sees — so the
        // tray advertises a stop it cannot perform while every other control
        // is disabled.
        assert_eq!(menu_target(Some(CaptureKind::Screen)), MenuTarget::Screen);
        assert_eq!(menu_target(Some(CaptureKind::Audio)), MenuTarget::Audio);
        // Idle keeps today's behaviour: the items are not shown, and if one
        // is somehow clicked the audio path's own "no recording" refusal is
        // the right (and only harmless) answer.
        assert_eq!(menu_target(None), MenuTarget::Audio);
    }

    #[test]
    fn every_capture_menu_handler_dispatches_on_the_guard() {
        // Structural, because the failure mode is a handler left behind:
        // stop, pause and resume each need the dispatch, and fixing one
        // while the other two keep calling straight into capture_commands
        // reproduces exactly half of the dead-control bug.
        let src = include_str!("tray.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        let dispatches = production.matches("menu_target(").count();
        assert_eq!(
            dispatches, 4,
            "expected the menu_target definition plus one dispatch in each of the three \
             capture menu handlers (stop, pause, resume); found {dispatches}"
        );
    }
}
