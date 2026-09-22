//! The native half of R8's two-layer command authorization.
//!
//! Since Task 11's exhaustive app manifest (`build.rs`'s `ALL_COMMANDS` +
//! `capabilities/editor.json`), the `editor_*` commands are ALSO
//! capability-gated at the Tauri ACL layer — `editor.json` scopes them to
//! the `editor` window alone, and Tauri refuses the IPC call before any
//! command handler runs. This check is the SECOND, independent layer,
//! inside the handler: without it, a capability misconfiguration (a future
//! edit that widens `editor.json`'s `windows`, or a new command added to
//! `generate_handler!` and granted to the wrong file) would be the only
//! thing standing between another webview — the panel, the bubble, the
//! region overlay — and driving an editing session or discarding a
//! project. Every `editor_*` command therefore calls
//! `require_editor_window(&window)?` as its FIRST statement, and
//! `authz_guard.rs` scans the module and fails naming any command that does
//! not. `require_session` is the second half of R8's native check: the
//! `sessionId` a request carries must name a session this process opened.

use std::collections::HashMap;
use std::sync::MutexGuard;

use tauri::WebviewWindow;
use vault_buddy_core::editor::{EditorError, EditorErrorCode, EditorSession};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::EditorState;

/// The one window label the editor commands answer to (`tauri.conf.json`).
const EDITOR_LABEL: &str = "editor";

/// The label predicate, split out so it is unit-testable without a live
/// `WebviewWindow` (which only a running Tauri app can construct).
pub(crate) fn is_editor_label(label: &str) -> bool {
    label == EDITOR_LABEL
}

/// `unauthorizedSource` unless the caller is the editor window.
pub fn require_editor_window(window: &WebviewWindow) -> Result<(), EditorError> {
    if is_editor_label(window.label()) {
        return Ok(());
    }
    log::warn!(
        "editor command refused: called from window {:?}, not the editor",
        window.label()
    );
    Err(EditorError::new(
        EditorErrorCode::UnauthorizedSource,
        "Only the editor window may do this.",
    ))
}

/// The session registry, locked, but only once `session_id` is known to be
/// in it — `sessionGone` otherwise (a closed session, a stale id from a
/// webview reload, or an id this process never minted). The caller can then
/// `get`/`get_mut` it without an `Option` it would have to re-check.
pub fn require_session<'a>(
    state: &'a EditorState,
    session_id: &str,
) -> Result<MutexGuard<'a, HashMap<String, EditorSession>>, EditorError> {
    let sessions = lock_ignoring_poison(&state.sessions);
    if sessions.contains_key(session_id) {
        Ok(sessions)
    } else {
        Err(EditorError::new(
            EditorErrorCode::SessionGone,
            "This editing session has ended. Reopen the project to continue.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_editor_window_refuses_other_labels() {
        assert!(is_editor_label("editor"));
        for other in [
            "main",
            "panel",
            "bubble",
            "overlay",
            "region-indicator",
            "Editor",
            "editor ",
            "",
        ] {
            assert!(!is_editor_label(other), "{other:?} must be refused");
        }
    }

    #[test]
    fn require_session_refuses_an_unknown_id() {
        let state = EditorState::default();
        let Err(err) = require_session(&state, "ses-nope") else {
            panic!("an unknown session must be refused");
        };
        assert_eq!(err.code, EditorErrorCode::SessionGone);
    }
}
