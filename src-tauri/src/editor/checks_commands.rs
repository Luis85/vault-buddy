//! `editor_get_checks` (Task 54; F-45; ADR §3.3): the before-you-share
//! findings for a live session -- `core::editor::checks::run_checks` over
//! the session's project plus the three facts only the shell knows. Its own
//! module (pre-flight F31) rather than another command in the already
//! crowded `media_commands.rs`.
//!
//! **Every fact comes from one existing source, never a second copy:**
//! - which originals are missing is `session_commands::missing_media` --
//!   the SAME list `EditorOpenResult.missing`, the media library's
//!   "Reconnect…" and `editor_relink_media` report;
//! - which assets have a sound track is each `sources.json` record's
//!   `hasAudio` -- what `CommandContext::assets_with_audio` gives
//!   `detachAudio` and what the render plan's inputs carry;
//! - how many webcam takes are unfinished is `EditorState::takes`, the
//!   registry every take call goes through (the webview's own
//!   `webcamTakes.ts` is a mirror for its close guard, not the authority).
//!
//! Read-only: nothing in the session, the store or the project changes. An
//! unreadable `sources.json` is an error, never a quietly short list -- a
//! check that could not look must not read as a pass.
//!
//! The `#[tauri::command]` takes `window` and calls `require_editor_window`
//! first (R8, `authz_guard.rs`); the work runs on the blocking pool.

use std::collections::BTreeSet;
use std::path::Path;

use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::checks::{run_checks, CheckFinding};
use vault_buddy_core::editor::{EditorError, EditorErrorCode, Project};

use super::authz::{require_editor_window, require_session};
use super::prefs_commands::{blocking, local_data};
use super::session_commands::missing_media;
use super::store_io::load_sources;
use super::EditorState;

/// The session's project as it stands, cloned so no lock is held while
/// `sources.json` is read and every source is stat'ed.
fn live_project(state: &EditorState, session_id: &str) -> Result<Project, EditorError> {
    let sessions = require_session(state, session_id)?;
    sessions
        .get(session_id)
        .map(|s| s.project().clone())
        .ok_or_else(|| {
            EditorError::new(
                EditorErrorCode::Internal,
                "session vanished under its own lock",
            )
        })
}

/// The `AppHandle`-free body of `editor_get_checks`.
pub(crate) fn checks_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
) -> Result<Vec<CheckFinding>, EditorError> {
    let project = live_project(state, session_id)?;
    let sources = load_sources(root, &project.id)?;
    let missing: BTreeSet<String> = missing_media(root, &project, &sources)
        .into_iter()
        .map(|m| m.asset_id)
        .collect();
    let with_audio: BTreeSet<String> = sources
        .iter()
        .filter(|(_, record)| record.has_audio)
        .map(|(id, _)| id.clone())
        .collect();
    let pending_takes = state.takes.open_takes(session_id).len();
    Ok(run_checks(&project, &missing, pending_takes, &with_audio))
}

/// ASYNC: reads `sources.json` and stats every source off the main thread.
#[tauri::command]
pub async fn editor_get_checks(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
) -> Result<Vec<CheckFinding>, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || checks_in(&app.state::<EditorState>(), &root, &session_id)).await
}

#[cfg(test)]
#[path = "checks_commands_tests.rs"]
mod tests;
