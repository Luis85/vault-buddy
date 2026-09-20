//! Spec 5.3's capture exclusion, Tauri side: which windows, when, and on
//! which thread. The Win32 call itself is
//! `vault_buddy_screen::exclusion::set_display_affinity` -- see that
//! module's doc for why it is over there and not here.
//!
//! **Why this is paired state with a single site each way.** A leaked
//! exclusion is invisible from inside this app: the buddy still shows, the
//! panel still works, nothing logs. What breaks is the user's NEXT Teams
//! call or OBS recording, where their Vault Buddy windows are simply gone
//! -- until Vault Buddy is restarted, because `WDA_*` is per-HWND and dies
//! with the window. Bounded, but there is no plausible bug report for it,
//! so the shape is one apply and one clear.
//!
//! **What the tests below pin, exactly -- and what they do not.** An
//! over-claiming invariant comment is worse than none here, so:
//!
//! - the DIRECTION: `apply` carries `EXCLUDE` and `clear` carries
//!   `INCLUDE`, and those constants really are the documented Win32
//!   values. Swapping the two wrappers is otherwise invisible: the windows
//!   are in frame during the capture (as they were before this module
//!   existed) and every window is left hidden afterwards.
//! - the COUNT and the PLACE: exactly one call each, over the whole shell
//!   source tree, with the apply inside `start_screen_capture_blocking`
//!   and the clear inside `clear_active_screen` -- each pinned to its
//!   enclosing function by byte offset, because both plausible
//!   relocations leak permanently (see the test's own comment).
//! - NOT an aliased call. The scan matches the literal
//!   `capture_exclusion::apply(` / `capture_exclusion::clear(`, so
//!   `use crate::capture_exclusion::clear as lift;` plus `lift(app)` adds
//!   a site it cannot see. Known limit, inherited from the
//!   `config_lock_guard.rs` precedent and deliberately not closed: the
//!   REMOVAL direction is still caught (the count drops to zero), and an
//!   extra clear over-clears, which is the safe direction.

use tauri::{AppHandle, Manager};

/// Every window the app owns. Kept in step with `tauri.conf.json` by a test
/// that reads the config -- phase 4's `editor` window will fail that test
/// until it is added here, which is the point.
pub(crate) const EXCLUDED_LABELS: &[&str] = &["main", "panel", "bubble", "overlay"];

/// The two affinity intents, named so that the DIRECTION is a value a test
/// can read rather than a bare `true` / `false` at the call site. The
/// wrappers below are each other's inverse and are three lines apart; a
/// swap compiles, runs, and leaks the exclusion for the process lifetime.
pub(crate) const EXCLUDE: bool = true;
pub(crate) const INCLUDE: bool = false;

/// Hide every app window from screen capture, for the duration of a
/// capture. THE ONE APPLY SITE is `screen_capture_worker`'s start path.
pub(crate) fn apply(app: &AppHandle) {
    set_affinity(app, EXCLUDE);
}

/// Put every app window back in view of other applications' captures. THE
/// ONE CLEAR SITE is `screen_commands::clear_active_screen`, the chokepoint
/// every screen-capture teardown -- clean stop, self-finalize, and all of
/// the start path's early returns -- already funnels through.
pub(crate) fn clear(app: &AppHandle) {
    set_affinity(app, INCLUDE);
}

fn set_affinity(app: &AppHandle, excluded: bool) {
    let handle = app.clone();
    // Window handles are read on the main thread like every other window
    // API in this app, and the call is fire-and-forget: the caller is a
    // worker thread that must not wait on the event loop, and an exclusion
    // that lands a few milliseconds late costs at most a frame or two of
    // buddy in the recording (docs/Gaps.md, recorded in task 8).
    if let Err(e) = app.run_on_main_thread(move || {
        for label in EXCLUDED_LABELS {
            let Some(window) = handle.get_webview_window(label) else {
                continue;
            };
            let Some(hwnd) = window_handle(&window, label) else {
                continue;
            };
            if let Err(e) = vault_buddy_screen::exclusion::set_display_affinity(hwnd, excluded) {
                // Windows 10 before 2004 has no WDA_EXCLUDEFROMCAPTURE.
                // Spec 5.3: log and carry on with the buddy visible in
                // frame; this must never be the reason a capture cannot
                // start.
                log::warn!("capture exclusion: could not set affinity for {label}: {e}");
            }
        }
    }) {
        log::warn!("capture exclusion: could not reach the main thread: {e}");
    }
}

#[cfg(windows)]
fn window_handle(window: &tauri::WebviewWindow, label: &str) -> Option<isize> {
    match window.hwnd() {
        Ok(h) => Some(h.0 as isize),
        Err(e) => {
            log::warn!("capture exclusion: no window handle for {label}: {e}");
            None
        }
    }
}

#[cfg(not(windows))]
fn window_handle(_window: &tauri::WebviewWindow, label: &str) -> Option<isize> {
    log::debug!("capture exclusion: no-op off Windows ({label})");
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Every window label declared in `tauri.conf.json`.
    fn declared_window_labels() -> Vec<String> {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        conf["app"]["windows"]
            .as_array()
            .expect("app.windows is an array")
            .iter()
            .map(|w| {
                w["label"]
                    .as_str()
                    .expect("every window has a label")
                    .to_string()
            })
            .collect()
    }

    // THE point of this test, and why it reads the config rather than
    // repeating a list: phase 4 adds the `editor` window and phase 5 may
    // add more. A window that exists but is not excluded appears in every
    // screen recording the user makes, which is a bug nobody will
    // attribute to the window having been added.
    #[test]
    fn every_declared_window_is_excluded_from_capture() {
        let declared = declared_window_labels();
        assert!(
            declared.len() >= 4,
            "expected at least main/panel/bubble/overlay, found {declared:?}"
        );
        for label in &declared {
            assert!(
                EXCLUDED_LABELS.contains(&label.as_str()),
                "window {label:?} is declared in tauri.conf.json but is not in \
                 EXCLUDED_LABELS, so it will appear in every screen recording"
            );
        }
    }

    // And the other direction: a label left behind after a window is
    // removed is a silent no-op that makes the list stop describing the app.
    #[test]
    fn no_excluded_label_names_a_window_that_does_not_exist() {
        let declared = declared_window_labels();
        for label in EXCLUDED_LABELS {
            assert!(
                declared.iter().any(|d| d == label),
                "EXCLUDED_LABELS names {label:?}, which no longer exists in tauri.conf.json"
            );
        }
    }

    /// Recursively collect every `.rs` file under `dir`, skipping this
    /// test's OWN file -- which necessarily names the functions being
    /// searched for. The `config_lock_guard.rs` precedent, including its
    /// `CARGO_MANIFEST_DIR` root (not the CWD) and its vacuity self-check.
    fn rust_files(dir: &Path, self_name: &std::ffi::OsStr, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, self_name, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
                && path.file_name() != Some(self_name)
            {
                out.push(path);
            }
        }
    }

    fn shell_sources() -> Vec<(PathBuf, String)> {
        let shell_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let self_name = Path::new(file!())
            .file_name()
            .expect("file!() always has a file name")
            .to_owned();
        let mut files = Vec::new();
        rust_files(&shell_src, &self_name, &mut files);
        // A self-check on the walk, not on the invariant: an empty walk
        // would make every assertion below vacuously true.
        assert!(
            files.len() > 5,
            "scan under {shell_src:?} found only {} file(s) -- the walk is broken, not the \
             invariant",
            files.len()
        );
        files
            .into_iter()
            .map(|p| {
                let text = std::fs::read_to_string(&p)
                    .unwrap_or_else(|e| panic!("could not read {p:?}: {e}"));
                (p, text)
            })
            .collect()
    }

    /// The production half of a shell source file: everything before its
    /// inline `#[cfg(test)]` module, so a needle spelled out in a test
    /// cannot satisfy an assertion about production code.
    fn production_half(src: &str) -> &str {
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }

    /// Byte offset of `needle`, with a message that names it -- the
    /// building block of the enclosing-function assertions below
    /// (`screen_capture_worker.rs`'s own structural test uses the same
    /// `find(a) < find(b)` idiom).
    fn offset_of(src: &str, needle: &str) -> usize {
        src.find(needle)
            .unwrap_or_else(|| panic!("expected to find {needle:?} in the production source"))
    }

    // Swapping `apply` and `clear` compiles, runs, and looks -- from
    // inside Vault Buddy -- like nothing happened: the windows are in
    // frame during the capture, exactly as they were before this module
    // existed. Then the capture ends and every window is set to
    // WDA_EXCLUDEFROMCAPTURE and stays there until the process exits, so
    // the user's next Teams share silently lacks their Vault Buddy
    // windows. Nothing else in the suite can see that, hence this test.
    //
    // Pinned two ways: the constants carry the documented Win32 values,
    // and each wrapper really passes the one it is named for.
    #[test]
    fn each_wrapper_carries_the_direction_it_is_named_for() {
        assert_eq!(vault_buddy_screen::exclusion::affinity_value(EXCLUDE), 17);
        assert_eq!(vault_buddy_screen::exclusion::affinity_value(INCLUDE), 0);

        let src = production_half(include_str!("capture_exclusion.rs"));
        let apply_fn = offset_of(src, "fn apply(");
        let clear_fn = offset_of(src, "fn clear(");
        let set_fn = offset_of(src, "fn set_affinity(");
        let exclude_call = offset_of(src, "set_affinity(app, EXCLUDE)");
        let include_call = offset_of(src, "set_affinity(app, INCLUDE)");
        assert!(
            apply_fn < exclude_call && exclude_call < clear_fn,
            "`apply` must be the EXCLUDE wrapper -- it is what hides the windows while a \
             capture runs"
        );
        assert!(
            clear_fn < include_call && include_call < set_fn,
            "`clear` must be the INCLUDE wrapper -- it is what puts the windows back in \
             view of every OTHER application's recording"
        );
    }

    /// One entry per occurrence, naming the file it was found in, so a
    /// failure says WHERE the extra call site is rather than only that the
    /// count is wrong.
    fn call_sites(needle: &str) -> Vec<String> {
        let mut hits = Vec::new();
        for (path, text) in shell_sources() {
            for _ in 0..text.matches(needle).count() {
                hits.push(path.display().to_string());
            }
        }
        hits
    }

    // A LEAKED exclusion does not break Vault Buddy -- it makes the user's
    // windows invisible in OTHER applications' recordings (Teams, OBS, a
    // colleague's screen share) with no error and no log line. Pairing it
    // to exactly one apply and one clear is what makes that hard to get
    // wrong later.
    //
    // Scans the WHOLE shell source tree, not one file: phase 2's
    // equivalent guard scanned only `screen_commands.rs` and went
    // half-blind the moment the lifecycle was split into
    // `screen_capture_worker.rs`.
    //
    // The counts and the file names are NOT enough on their own, which is
    // why each call is also pinned to its ENCLOSING function by byte
    // offset. Both relocations a future author would find natural stay
    // within the same counts, and both leak permanently:
    //
    //   - the apply moved into `start_screen_capture`'s async tail, where
    //     the tray state and the buddy-show already live. Under spec 14's
    //     self-finalize (the recorded window closes during the 15 s
    //     blocking start) the `screen-capture-monitor` thread is already
    //     running, so it clears and posts `clear` BEFORE the blocking
    //     start returns and the tail posts `apply`. Every window ends up
    //     excluded with no capture running.
    //   - the clear moved into `stop_screen_capture`, still in
    //     `screen_commands.rs`. A self-finalized capture never calls that
    //     command, so the exclusion is never lifted.
    //
    // Known limit (the `config_lock_guard.rs` precedent's): the scan
    // matches a literal path, so an aliased re-export would add a site it
    // cannot see. Removing a site is still caught, and an extra clear
    // over-clears -- the safe direction. See the module doc.
    #[test]
    fn the_capture_exclusion_is_applied_and_cleared_from_exactly_one_place_each() {
        let applies = call_sites("capture_exclusion::apply(");
        let clears = call_sites("capture_exclusion::clear(");
        assert_eq!(
            applies.len(),
            1,
            "expected exactly one apply site, found {applies:?}"
        );
        assert_eq!(
            clears.len(),
            1,
            "expected exactly one clear site, found {clears:?}"
        );
        assert!(
            applies[0].ends_with("screen_capture_worker.rs"),
            "the apply must live in start_screen_capture_blocking, after the reservation is \
             installed and before any thread is spawned -- but it is in {}",
            applies[0]
        );
        assert!(
            clears[0].ends_with("screen_commands.rs"),
            "the clear must live in clear_active_screen -- the single chokepoint every \
             screen-capture teardown already funnels through -- but it is in {}",
            clears[0]
        );

        let worker = production_half(include_str!("screen_capture_worker.rs"));
        let blocking_start = offset_of(worker, "fn start_screen_capture_blocking(");
        let apply_call = offset_of(worker, "crate::capture_exclusion::apply(");
        let after_blocking_start = offset_of(worker, "\nfn describe_screen_error(");
        assert!(
            blocking_start < apply_call && apply_call < after_blocking_start,
            "the apply must sit INSIDE start_screen_capture_blocking. Moved into \
             start_screen_capture's async tail it can land after the monitor thread has \
             already cleared a self-finalized capture, leaving every window excluded with \
             nothing recording"
        );

        let commands = production_half(include_str!("screen_commands.rs"));
        let chokepoint = offset_of(commands, "fn clear_active_screen(");
        let clear_call = offset_of(commands, "crate::capture_exclusion::clear(");
        let after_chokepoint = offset_of(commands, "\npub fn is_capturing(");
        assert!(
            chokepoint < clear_call && clear_call < after_chokepoint,
            "the clear must sit INSIDE clear_active_screen. Moved into stop_screen_capture \
             it is skipped entirely by a self-finalized capture, which never goes through \
             that command"
        );
    }
}
