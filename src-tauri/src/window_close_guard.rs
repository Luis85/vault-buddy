//! Structural pin for the editor-close fixes (Phase 4 Task 1 fix wave,
//! review findings I-1 and I-2), which live in `window_close.rs`. Neither
//! behaviour can be exercised by constructing a real `tauri::Window` and
//! firing a native `CloseRequested` event on a headless Linux test runner,
//! so — like `config_lock_guard.rs` and `tray.rs`'s own label-list tests —
//! this reads `window_close.rs`'s own source text and fails if either
//! invariant regresses:
//!
//! - I-1: the arm that finalizes a live capture and marks a clean shutdown
//!   must be gated to the `"main"` window only. Ungated, a click on the
//!   editor's titlebar X (spec 5.1's `decorations: true`, the first
//!   non-main window with a close button) either latches
//!   `diagnostics::MARKER_GATE` "clean" forever with no capture running
//!   (blinding every later native fault/kill/power-loss detection), or —
//!   with a capture running — silently stops it and quits the app.
//! - I-2: the editor's own close handling must `prevent_close()` and
//!   `hide()` rather than let the window be destroyed. Config-declared
//!   windows are built once at startup; a destroyed editor makes
//!   `get_webview_window("editor")` return `None` for the rest of the
//!   process, with no way back short of restarting the app.

#[cfg(test)]
mod tests {
    /// Collapse every run of whitespace to a single space, so a
    /// rustfmt-wrapped multi-line match arm still reads as one contiguous
    /// string (the `config_lock_guard.rs` idiom) — without deleting
    /// whitespace outright, which would fuse adjacent keywords together.
    fn normalize_whitespace(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last_was_space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                if !last_was_space {
                    out.push(' ');
                }
                last_was_space = true;
            } else {
                out.push(c);
                last_was_space = false;
            }
        }
        out
    }

    // I-1: reverting the label gate (e.g. matching `_ =>` instead of
    // `"main" =>` in `handle_close_requested`'s dispatch, or inlining the
    // finalize/mark-clean-shutdown body directly in that catch-all) makes
    // this exact marker vanish from the source.
    #[test]
    fn the_finalize_and_mark_clean_shutdown_path_is_gated_to_the_main_window() {
        let src = normalize_whitespace(include_str!("window_close.rs"));
        let marker = r#""main" => handle_main_close(window, api),"#;
        assert!(
            src.contains(marker),
            "the CloseRequested path that finalizes a capture and calls \
             diagnostics::mark_clean_shutdown() must be reached only for \
             the main window — an ungated path lets a click on the \
             editor's titlebar X latch a false 'clean' shutdown marker \
             (MARKER_GATE latches forever once tripped) or silently stop a \
             live capture and quit the app"
        );
        // Belt-and-suspenders on the arm itself: `handle_main_close` must
        // still be the one doing the finalizing/marking, not some other
        // function reachable from a non-"main" label.
        assert!(
            src.contains("fn handle_main_close"),
            "handle_main_close must exist as the main-only close path"
        );
    }

    // I-2: removing `api.prevent_close()` or `window.hide()` from the
    // editor's own arm — or deleting the arm entirely, falling back to the
    // catch-all `_ => {}` that lets the window be destroyed — fails this.
    #[test]
    fn the_editor_close_is_prevented_and_hidden_rather_than_destroyed() {
        let src = normalize_whitespace(include_str!("window_close.rs"));
        let marker = r#""editor" =>"#;
        let pos = src
            .find(marker)
            .expect("an editor-specific branch must exist in handle_close_requested");
        let body_start = pos + marker.len();
        let body = &src[body_start..(body_start + 400).min(src.len())];
        assert!(
            body.contains("api.prevent_close()"),
            "closing the editor must be PREVENTED, not allowed to destroy \
             the window — config-declared windows are built once at \
             startup, so a destroyed editor cannot be reopened for the \
             rest of the process"
        );
        assert!(
            body.contains("window.hide()"),
            "a prevented editor close must still hide the window, or the \
             user's click on the X does nothing at all"
        );
        assert!(
            !body.contains("mark_clean_shutdown") && !body.contains("finalize_if_recording"),
            "the editor's own close branch must never mark a clean \
             shutdown or finalize a capture — that behaviour belongs to \
             the main window's path alone"
        );
    }
}
