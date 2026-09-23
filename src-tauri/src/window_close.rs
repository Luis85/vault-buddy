//! `CloseRequested` routing, extracted out of `lib.rs`'s `on_window_event`
//! closure (Phase 4 Task 1 fix wave, review findings I-1/I-2) — both to keep
//! that closure under the LOC cap and to give the invariant one place to
//! live. Before this extraction the arm matched ANY window's
//! `CloseRequested`, which was near-theoretical while every window but the
//! buddy was undecorated and `skipTaskbar` — until the editor
//! (`decorations: true`, a real titlebar X).
//!
//! - **I-1**: the finalize/mark-clean-shutdown logic below is gated to
//!   `"main"` only. Ungated, closing the editor could latch
//!   `diagnostics::MARKER_GATE` "clean" forever with no capture running
//!   (the gate never re-arms itself outside the updater's failure path, so
//!   every LATER native fault/kill/power-loss would then report no crash at
//!   next launch), or — with a capture running — silently stop it and quit
//!   the whole app.
//! - **I-2**: the editor's own close is turned into a hide, never a
//!   destroy. Config-declared windows are built once at startup, so
//!   destroying the editor would leave `get_webview_window("editor")`
//!   returning `None` for the rest of the process — no
//!   `WebviewWindowBuilder` re-create, because the config-derived-label
//!   tests and the one-shot capture exclusion assume every window stays
//!   declared in `tauri.conf.json`. This is NOT the hide-to-tray gesture
//!   `tray::COMPANION_LABELS` governs — the editor is deliberately absent
//!   from that list because hiding it FROM THE OUTSIDE would strand
//!   unsaved edits with no way back. The user clicking the editor's own X
//!   is an explicit instruction about that one window, which is why
//!   answering it with a hide is not a contradiction.
//! - **Task 37**: the editor's X no longer hides by itself. It emits
//!   `editor:closeRequested` to the editor window ALONE (`emit_to`, the
//!   `editor:open` shape) and the webview decides: a dirty session or a
//!   live render gets the close guard, anything else hides at once through
//!   `editor_hide_window`. Only if that emit fails does this fall back to
//!   hiding directly (`route_editor_close`), so the X can never do nothing.
//!
//! Every other window (panel/bubble/overlay) falls through to Tauri's
//! default close, untouched by either rule.

use tauri::{CloseRequestApi, Emitter, Manager, Window};

pub(crate) fn handle_close_requested(window: &Window, api: &CloseRequestApi) {
    match window.label() {
        "main" => handle_main_close(window, api),
        "editor" => {
            api.prevent_close();
            let app = window.app_handle();
            route_editor_close(
                || app.emit_to("editor", "editor:closeRequested", serde_json::json!({})),
                || window.hide(),
            );
        }
        _ => {}
    }
}

/// What the editor's close did (Task 37).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorCloseRoute {
    /// `editor:closeRequested` reached the editor webview, which now owns
    /// the decision (close guard, or `editor_hide_window` when clean).
    AskedTheEditor,
    /// The request could not be delivered, so the window was hidden
    /// directly — exactly the pre-Task-37 behaviour.
    HidFallback,
}

/// The editor X's routing, split out so it is unit-testable without a live
/// `Window`: ask the editor webview first, and ONLY if that request cannot
/// be delivered fall back to hiding. A successful emit never hides here —
/// that would close the window under the close guard it just asked for — and
/// a failed one always does: an X that does nothing, with no webview left
/// to answer, would strand the window on screen.
pub(crate) fn route_editor_close<E: std::fmt::Display, H: std::fmt::Display>(
    emit: impl FnOnce() -> Result<(), E>,
    hide: impl FnOnce() -> Result<(), H>,
) -> EditorCloseRoute {
    let Err(e) = emit() else {
        return EditorCloseRoute::AskedTheEditor;
    };
    log::warn!("could not ask the editor about its close, hiding it instead: {e}");
    if let Err(e) = hide() {
        log::warn!("could not hide the editor window on close: {e}");
    }
    EditorCloseRoute::HidFallback
}

fn handle_main_close(window: &Window, api: &CloseRequestApi) {
    let app = window.app_handle();
    if crate::shutdown_gate::shutdown_is_blocked(app) {
        // Alt+F4 / session shutdown bypass tray::quit — the recording must
        // still finalize, but that wait is unbounded and this callback runs
        // on the event loop: blocking would freeze the UI for the whole
        // encode. Hold this close, finalize on a worker thread, then
        // re-trigger it via the app handle.
        //
        // The export is the third term for the same reason it is one in
        // `tray::quit`: it is the only one of the three mid-write into a
        // vault, and its ffmpeg CHILD is a separate process nothing else on
        // the way out would stop (GAP-155). All three now come from one
        // place, `shutdown_gate::shutdown_is_blocked` — the updater's door
        // spelled none of them and nobody noticed (GAP-160).
        api.prevent_close();
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("close-finalize".into())
            .spawn(move || {
                // FIRST: bounded at a few seconds and it kills a child
                // process, where the two finalizes below are unbounded.
                crate::export_shutdown::cancel_if_exporting(&app);
                crate::capture_commands::finalize_if_recording(&app);
                crate::screen_commands::finalize_if_capturing(&app);
                // All three are dealt with, so every predicate in the gate
                // above is now false and the re-triggered CloseRequested
                // takes the else branch below (pass through to destruction)
                // — no loop. The export's is false either because the cancel
                // unwound or because its bounded wait expired, which is why
                // `cancel_if_exporting` proceeds on expiry rather than
                // looping: an Alt+F4 that never closes is worse than an
                // export abandoned after its bound.
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
            });
        if let Err(e) = spawned {
            // Never panic in a window-event handler (aborts across the
            // WebView2 FFI boundary, no crash record). The close stays
            // prevented: better an app that ignores one Alt+F4 than one
            // that exits stranding a .part.
            log::error!("could not spawn close-finalize thread: {e}");
        }
    } else {
        // Alt+F4 / session end: the window is about to be destroyed and the
        // process exits with it.
        log::info!("clean shutdown (window close)");
        crate::diagnostics::mark_clean_shutdown();
    }
}
