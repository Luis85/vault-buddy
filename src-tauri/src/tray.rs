use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::capture_guard::{CaptureGuard, CaptureKind};

/// Every window the app owns, in destroy order — the buddy LAST, because the
/// quit path saves its position and destroying it first would race that save.
///
/// This is the QUIT walk. It must cover every configured window including the
/// editor: all of them share WebView2's `Chrome_WidgetWin_0` class, and
/// leaving even one alive fails the unregister with
/// `ERROR_CLASS_HAS_WINDOWS (1412)`.
pub const ALL_WINDOW_LABELS: [&str; 5] = ["panel", "bubble", "overlay", "editor", "main"];

/// The companion surfaces — every window hide-to-tray takes down.
///
/// Deliberately NOT `ALL_WINDOW_LABELS`: the editor is a real application
/// window that can hold unsaved edits, and hiding it would strand that work
/// off-screen with no way back (spec 5.1, the same class as GAP-82's
/// panel-hide problem). Hide-to-tray is a companion gesture; the editor is
/// closed by the user, by a successful save, or by an explicit discard.
pub const COMPANION_LABELS: [&str; 4] = ["panel", "bubble", "overlay", "main"];

/// Windows whose position the window-state plugin must NOT persist: every
/// window except the buddy. The panel, bubble and overlay are all positioned
/// fresh — while hidden — every time they are shown, and the editor opens at
/// its configured default, so a restored position is junk that only buys a
/// startup restore and a `Moved` handler holding the plugin's cache lock.
pub const POSITION_DENYLIST: [&str; 4] = ["panel", "bubble", "overlay", "editor"];

/// Hide the companion (and its panel/bubble); the tray "Show / Hide" brings
/// the buddy back.
///
/// THE single hide chokepoint: the buddy is the recording indicator, and
/// hiding it mid-capture would violate the spec's no-hidden-recordings
/// requirement — so this is the only place allowed to call window.hide()
/// on the buddy, and any future hide path must route through here to
/// inherit the guard.
///
/// The editor is deliberately absent: see `COMPANION_LABELS`.
///
/// So is the EXPORT. This gate has the same two-predicate shape as `quit`
/// below, which grew a third term for `export_shutdown::export_blocks_shutdown`
/// — do not "unify" the two. The buddy is the RECORDING indicator, and hide is
/// refused mid-capture so a recording can never run with nothing on screen
/// saying so. An export needs no indicator: it renders its own progress in the
/// editor window, which hide-to-tray does not touch. Blocking hide for the
/// minutes an export runs would pin the app on screen during exactly the
/// operation a user wants to walk away from. A structural test in
/// `export_shutdown` pins this asymmetry in both directions.
pub fn hide_buddy(app: &AppHandle) {
    if crate::capture_commands::recording_blocks_shutdown(app)
        || crate::screen_commands::capture_blocks_shutdown(app)
    {
        log::info!("hide ignored: a capture is in progress");
        return;
    }
    for label in COMPANION_LABELS {
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
    //
    // An in-flight EXPORT is the third thing this must not abandon, and the
    // only one of the three that is mid-write into a vault: quitting under it
    // used to run `finish_quit` immediately, destroying every window and
    // calling exit(0) while the `screen-export` thread was committing video →
    // note → staged-removal, and leaving the ffmpeg CHILD — a separate
    // process — writing into staging after the app was gone (GAP-155).
    //
    // All three terms live in one place, `shutdown_gate::shutdown_is_blocked`
    // — the updater's own door spelled none of them and nobody noticed
    // (GAP-160), so the disjunction is no longer written out per door.
    if crate::shutdown_gate::shutdown_is_blocked(app) {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("shutdown-finalize".into())
            .spawn(move || {
                // FIRST: it kills a child process and is bounded at a few
                // seconds, while the two finalizes below are unbounded — an
                // export left running behind them would go on writing into
                // the vault for as long as they take.
                crate::export_shutdown::cancel_if_exporting(&app);
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

    // The DESTROY walk must cover every configured window, editor included:
    // all of them share WebView2's Chrome_WidgetWin_0 class, and leaving one
    // alive fails the unregister with ERROR_CLASS_HAS_WINDOWS (1412).
    #[test]
    fn the_quit_walk_destroys_every_configured_window() {
        let configured = configured_labels();
        let mut walked: Vec<String> = ALL_WINDOW_LABELS.iter().map(|s| s.to_string()).collect();
        let mut expected = configured.clone();
        walked.sort();
        expected.sort();
        assert_eq!(
            walked, expected,
            "every window in tauri.conf.json must be destroyed on quit"
        );
        // The buddy is destroyed last: it is the window whose position the
        // quit path saves, and destroying it first would race that save.
        assert_eq!(ALL_WINDOW_LABELS.last(), Some(&"main"));
    }

    // The HIDE walk must cover every configured window EXCEPT the editor.
    // Hide-to-tray is a companion-surface gesture; the editor is a real
    // application window that can hold unsaved edits, and hiding it would
    // strand that work off-screen with no way back (spec 5.1, the GAP-82
    // class). Asserted as a derived set, not a literal, so a window added to
    // the config lands in exactly one of the two lists on purpose.
    #[test]
    fn the_hide_walk_covers_every_companion_window_and_not_the_editor() {
        let mut companions: Vec<String> = COMPANION_LABELS.iter().map(|s| s.to_string()).collect();
        let mut expected: Vec<String> = configured_labels()
            .into_iter()
            .filter(|l| l != "editor")
            .collect();
        companions.sort();
        expected.sort();
        assert_eq!(
            companions, expected,
            "the hide walk must cover every companion window and never the editor"
        );
        assert!(
            !COMPANION_LABELS.contains(&"editor"),
            "hiding the editor would strand unsaved edits off-screen"
        );
    }

    // The editor is a real application window: it is NOT transparent, IS
    // decorated and resizable, and is NOT always-on-top. Those four are what
    // make it unlike the three companion surfaces, and a later edit that
    // quietly makes it another always-on-top transparent panel would change
    // what the window IS without changing its name.
    #[test]
    fn the_editor_window_is_a_real_application_window() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let editor = conf["app"]["windows"]
            .as_array()
            .expect("app.windows")
            .iter()
            .find(|w| w["label"] == "editor")
            .expect("an editor window must be declared");
        assert_eq!(editor["transparent"], false);
        assert_eq!(editor["decorations"], true);
        assert_eq!(editor["resizable"], true);
        assert_eq!(editor["alwaysOnTop"], false);
        assert_eq!(editor["skipTaskbar"], false, "it is alt-tabbable by design");
        assert_eq!(
            editor["visible"], false,
            "created hidden, like panel and bubble"
        );
        // Spec 5.1's geometry block, unpinned before this: a shrink to
        // e.g. 100x100 passed every other assertion here.
        assert_eq!(editor["width"], 960);
        assert_eq!(editor["height"], 640);
        assert_eq!(editor["minWidth"], 720);
        assert_eq!(editor["minHeight"], 480);
    }

    // Without "editor" in this capability's `windows` array, the editor
    // webview gets no permissions at all and every `invoke` from it fails
    // (the brief's own stated consequence) -- silently, since nothing on
    // Linux can construct and drive the real webview to observe it.
    #[test]
    fn the_editor_window_is_granted_the_default_capability() {
        let cap: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json"))
                .expect("capabilities/default.json");
        let windows = cap["windows"]
            .as_array()
            .expect("capabilities/default.json must declare a windows array")
            .iter()
            .map(|w| w.as_str().expect("window label"))
            .collect::<Vec<_>>();
        assert!(
            windows.contains(&"editor"),
            "the editor window must be granted the default capability, or every invoke() \
             from it fails; windows was: {windows:?}"
        );
    }

    // The editor webview reads its staged capture through the asset
    // protocol, and this scope IS the security boundary: `$APPLOCALDATA`
    // resolves to the app's own local-data dir, so this grants the webview
    // read access to the staging directory and nothing else. Widening it to
    // `$APPLOCALDATA/*` (or adding a vault root) would let the editor
    // webview read arbitrary files via the asset handler -- a change this
    // test exists to make loud rather than a silent config edit.
    #[test]
    fn the_asset_protocol_scope_is_pinned_to_the_staging_directory_alone() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let scope = conf["app"]["security"]["assetProtocol"]["scope"]
            .as_array()
            .expect("assetProtocol.scope must be an array");
        let scope: Vec<&str> = scope
            .iter()
            .map(|v| v.as_str().expect("scope entry"))
            .collect();
        assert_eq!(
            scope,
            vec!["$APPLOCALDATA/screen-captures/*"],
            "the asset protocol scope must name the staging directory exactly -- never a \
             vault path or a wider glob like $APPLOCALDATA/*"
        );
        assert_eq!(
            conf["app"]["security"]["assetProtocol"]["enable"], true,
            "the asset protocol must be enabled for the editor to read its staged capture"
        );
        // Spec 5.1 names this CSP clause mandatory for preview playback: without
        // it, a later edit or merge can silently drop the editor's <video>
        // element's ability to load the staged capture through the asset
        // handler, and every gate here stays green -- the failure surfaces
        // only on a real Windows run of the editor.
        let csp = conf["app"]["security"]["csp"]
            .as_str()
            .expect("app.security.csp must be a string");
        assert!(
            csp.contains("media-src 'self' asset: http://asset.localhost"),
            "the CSP must allow media-src from the asset protocol, or the \
             editor's <video> preview is blocked; csp was: {csp}"
        );
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
        let destroy_walks = production.matches("in ALL_WINDOW_LABELS").count();
        let hide_walks = production.matches("in COMPANION_LABELS").count();
        assert_eq!(
            destroy_walks, 1,
            "expected exactly one quit-time destroy walk over ALL_WINDOW_LABELS; found {destroy_walks}"
        );
        assert_eq!(
            hide_walks, 1,
            "expected exactly one hide walk over COMPANION_LABELS; found {hide_walks}"
        );
        // The three constants are the only places a label may be spelled out.
        let spellings = production.matches("\"bubble\"").count();
        assert_eq!(
            spellings, 3,
            "window labels belong in ALL_WINDOW_LABELS / COMPANION_LABELS / \
             POSITION_DENYLIST and nowhere else in tray.rs; found {spellings} \
             spellings of \"bubble\""
        );
        // "bubble" is in all three constants, so it can't by itself catch a
        // walk that adds a THIRD, editor-specific list back in (e.g. a
        // second, inline `get_webview_window("editor")` hide appended
        // inside `hide_buddy`, alongside — not instead of — the
        // COMPANION_LABELS walk: that still satisfies every assertion above,
        // since `hide_walks` counts occurrences of "in COMPANION_LABELS" and
        // an inline single-window hide has no such text). "editor" appears
        // in exactly two places today — ALL_WINDOW_LABELS and
        // POSITION_DENYLIST, deliberately never COMPANION_LABELS — so a
        // third occurrence anywhere in production code is exactly that
        // shape of regression.
        let editor_spellings = production.matches("\"editor\"").count();
        assert_eq!(
            editor_spellings, 2,
            "\"editor\" belongs only in ALL_WINDOW_LABELS and \
             POSITION_DENYLIST, never COMPANION_LABELS and never a bespoke \
             inline hide/destroy inside a function body; found \
             {editor_spellings} spellings"
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
