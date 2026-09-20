//! Spec 5.3's capture exclusion, Tauri side: which windows, when, and on
//! which thread. The Win32 call itself is
//! `vault_buddy_screen::exclusion::set_display_affinity` -- see that
//! module's doc for why it is over there and not here.
//!
//! **Why this is paired state with a single site each way.** A leaked
//! exclusion is invisible from inside this app: the buddy still shows, the
//! panel still works, nothing logs. What breaks is the user's NEXT Teams
//! call or OBS recording, where their Vault Buddy windows are simply gone.
//! There is no plausible bug report for that, so the code shape has to make
//! it impossible -- one apply, one clear, both pinned by a structural test
//! over the whole shell source tree.

use tauri::{AppHandle, Manager};

/// Every window the app owns. Kept in step with `tauri.conf.json` by a test
/// that reads the config -- phase 4's `editor` window will fail that test
/// until it is added here, which is the point.
pub(crate) const EXCLUDED_LABELS: &[&str] = &["main", "panel", "bubble", "overlay"];

/// Hide every app window from screen capture, for the duration of a
/// capture. THE ONE APPLY SITE is `screen_capture_worker`'s start path.
pub(crate) fn apply(app: &AppHandle) {
    set_affinity(app, true);
}

/// Put every app window back in view of other applications' captures. THE
/// ONE CLEAR SITE is `screen_commands::clear_active_screen`, the chokepoint
/// every screen-capture teardown -- clean stop, self-finalize, and all of
/// the start path's early returns -- already funnels through.
pub(crate) fn clear(app: &AppHandle) {
    set_affinity(app, false);
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
    // to exactly one apply and one clear is the only thing that makes that
    // impossible to get wrong later.
    //
    // Scans the WHOLE shell source tree, not one file: phase 2's
    // equivalent guard scanned only `screen_commands.rs` and went
    // half-blind the moment the lifecycle was split into
    // `screen_capture_worker.rs`.
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
            clears[0].ends_with("screen_commands.rs"),
            "the clear must live in clear_active_screen -- the single chokepoint every \
             screen-capture teardown already funnels through -- but it is in {}",
            clears[0]
        );
    }
}
