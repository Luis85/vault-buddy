//! The workspace view-preference surface (Task 18; F-48, F-25, F-14) — the
//! `workspace.json` half of a project's own directory
//! (`%LOCALAPPDATA%\...\editor-projects\<id>\workspace.json`), split out of
//! `save_commands.rs` precisely because it is the OPPOSITE kind of write:
//! selection, playhead, panel layout and the light/dark toggle are not
//! edits, so `editor_save_workspace` never touches a session's revision or
//! its undo/redo history at all — it reads/writes one small JSON file
//! keyed only by the project directory a session names, never the session's
//! in-memory `EditorSession` itself. `session_id` still gates every call
//! (R8: the caller must own a live session over the project it is
//! addressing), it just never becomes a `require_session(...).get_mut(...)`
//! the way `session_commands::execute_in` does.
//!
//! Guide progress — the app-wide preference Task 55 first put here — moved
//! to its own module, `guide_commands.rs`, with Task 57's progress-file
//! export/import. This module keeps `local_data`/`blocking`, which its
//! siblings share.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST — `authz_guard.rs` fails
//! naming any that does not.

use std::path::Path;

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::{sanitize, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::project_store::project_dir;
use super::redact::redact_path;
use super::store_io::read_bounded;
use super::EditorState;

pub(crate) const WORKSPACE_FILE: &str = "workspace.json";

/// The wire size cap this task's brief names. Checked against the RAW
/// incoming payload before it is ever sanitized or written — a hostile or
/// runaway client document (a pasted megabyte string into a text field
/// that has no business holding one) is refused outright rather than
/// spending the sanitize pass on it. In practice `sanitize`'s own output is
/// bounded far below this anyway (18 known fields, the biggest being
/// `selection_clip_ids` capped at `MAX_CLIPS` ids of at most 100 chars
/// each), so this check exists for the INPUT, not to protect the file on
/// disk from `sanitize`'s own output.
const MAX_WORKSPACE_JSON_BYTES: usize = 64 * 1024;

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

/// The project id a live session names — `sessionGone` for anything else.
/// Deliberately does NOT touch `EditorSession::execute`/`mark_saved` or any
/// other revision-bearing path: a workspace read/write only needs to know
/// WHERE the project's directory is, never what revision it is at.
pub(crate) fn project_id_for(state: &EditorState, session_id: &str) -> Result<String, EditorError> {
    let sessions = require_session(state, session_id)?;
    sessions
        .get(session_id)
        .map(|s| s.project().id.clone())
        .ok_or_else(|| internal("session vanished under its own lock"))
}

/// Read and sanitize `workspace.json`. A missing file — a project that
/// predates this task, or one that has never had a view preference saved —
/// degrades to the sanitized empty blob rather than an error, the same
/// posture `store_io::load_project`'s callers get for every OTHER missing
/// piece of an otherwise-valid project. A malformed file (hand-edited, or
/// truncated by a crash `write_atomic_replacing` did not fully protect
/// against) degrades the same way rather than blocking the project from
/// opening — `sanitize` itself already tolerates a wrong-typed field by
/// field; this is one level up, tolerating the whole document being
/// unparseable JSON.
pub(crate) fn read_workspace(root: &Path, project_id: &str) -> Result<Value, EditorError> {
    let dir = project_dir(root, project_id).ok_or_else(|| {
        err(
            EditorErrorCode::InvalidRequest,
            format!("{project_id:?} is not a valid project id"),
        )
    })?;
    let path = dir.join(WORKSPACE_FILE);
    let empty = || Value::Object(serde_json::Map::new());
    // Bounded and no-follow (final review M4): a file over the size the
    // save path writes degrades like a malformed one, unread.
    let raw: Value = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => empty(),
        _ => match read_bounded(&path, MAX_WORKSPACE_JSON_BYTES as u64) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!(
                    "editor workspace: {} is not valid JSON, degrading to empty ({e})",
                    redact_path(&path)
                );
                empty()
            }),
            Err(e) if e.code == EditorErrorCode::InvalidProject => {
                log::warn!("editor workspace: {}; degrading to empty", e.message);
                empty()
            }
            Err(e) => return Err(internal(e.message)),
        },
    };
    Ok(serde_json::to_value(sanitize(&raw)).expect("Workspace always serializes"))
}

pub(crate) fn get_workspace_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
) -> Result<Value, EditorError> {
    let project_id = project_id_for(state, session_id)?;
    read_workspace(root, &project_id)
}

/// Sanitize and persist a workspace blob. **Never touches the session's
/// revision or history** (this module's own doc) — the write goes straight
/// to `workspace.json` via `write_atomic_replacing` (temp + fsync +
/// replacing rename, the same rails every other project-store file rides),
/// with no `EditorSession::mark_saved`/`execute` call anywhere in this
/// path.
///
/// **MUTATION CHECK** (this task's brief, `oversized_workspace_is_refused`):
/// dropping the size check — or moving it to AFTER `sanitize` — lets an
/// oversized incoming document through to `write_atomic_replacing`
/// (sanitize's own bounds only cap what it KEEPS, not what it is handed;
/// checking the raw size first is what actually refuses a runaway payload
/// rather than merely truncating it silently to whatever sanitize kept).
pub(crate) fn save_workspace_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    workspace: Value,
) -> Result<(), EditorError> {
    let project_id = project_id_for(state, session_id)?;
    let raw_len = serde_json::to_vec(&workspace)
        .map(|b| b.len())
        .unwrap_or(usize::MAX);
    if raw_len > MAX_WORKSPACE_JSON_BYTES {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            format!(
                "The saved workspace is {raw_len} bytes, exceeding the {MAX_WORKSPACE_JSON_BYTES} byte maximum"
            ),
        ));
    }
    let sanitized = sanitize(&workspace);
    let dir = project_dir(root, &project_id)
        .ok_or_else(|| internal(format!("{project_id:?} is not a valid project id")))?;
    let json = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| internal(format!("Could not encode the workspace: {e}")))?;
    // Under the session's save lock, with the session still live once it
    // is held (final review I1): a discard removes the folder under that
    // lock, and a write racing it would leave a temp file in — or recreate
    // `workspace.json` inside — a folder being removed.
    let lock = super::save_commands::session_save_lock(state, session_id)?;
    let _guard = lock_ignoring_poison(&lock);
    drop(require_session(state, session_id)?);
    write_atomic_replacing(&dir.join(WORKSPACE_FILE), &json)
        .map_err(|e| internal(format!("Could not save the workspace: {e}")))
}

pub(crate) fn local_data(app: &AppHandle) -> Result<std::path::PathBuf, EditorError> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| internal(format!("Could not resolve the app data directory: {e}")))
}

pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| internal(format!("The editor task failed: {e}")))?
}

/// ASYNC: a `read_dir`-adjacent single-file read off the main thread.
#[tauri::command]
pub async fn editor_get_workspace(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
) -> Result<Value, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || get_workspace_in(&app.state::<EditorState>(), &root, &session_id)).await
}

/// ASYNC: an fsync'd temp-then-replacing-rename write off the main thread.
#[tauri::command]
pub async fn editor_save_workspace(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    workspace: Value,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || save_workspace_in(&app.state::<EditorState>(), &root, &session_id, workspace))
        .await
}

#[cfg(test)]
mod tests {
    use vault_buddy_core::editor::EditorSession;

    use super::*;
    use crate::editor::project_store::minimal_project;
    use crate::editor::store_io::{create_project, load_project};

    /// Registers a live session over a freshly created project — the
    /// minimum every test here needs `project_id_for` to resolve.
    fn opened_session(root: &Path, state: &EditorState, project_id: &str) -> String {
        create_project(root, &minimal_project(project_id), &Default::default()).unwrap();
        let session_id = format!("ses-{project_id}");
        let session = EditorSession::resume(session_id.clone(), minimal_project(project_id), 1);
        state
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), session);
        session_id
    }

    #[test]
    fn get_workspace_of_a_project_with_no_saved_prefs_reads_the_sanitized_empty_blob() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let session_id = opened_session(root.path(), &state, "proj1");

        let value = get_workspace_in(&state, root.path(), &session_id).unwrap();

        assert_eq!(value, serde_json::json!({}));
    }

    // Final review M4: `workspace.json` was read whole, however large. A
    // file over the bound (the save path never writes one) degrades like a
    // malformed one, to the sanitized empty blob, without being read.
    #[test]
    fn an_oversized_workspace_file_degrades_to_empty_without_being_read() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let session_id = opened_session(root.path(), &state, "proj1");
        let mut bytes = br#"{"theme":"light"}"#.to_vec();
        bytes.resize(MAX_WORKSPACE_JSON_BYTES + 1, b' ');
        let dir = project_dir(root.path(), "proj1").unwrap();
        std::fs::write(dir.join(WORKSPACE_FILE), bytes).unwrap();

        let value = get_workspace_in(&state, root.path(), &session_id).unwrap();

        assert_eq!(value, serde_json::json!({}));
    }

    #[test]
    fn get_workspace_refuses_an_unknown_session() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();

        let err = get_workspace_in(&state, root.path(), "ses-nope").unwrap_err();

        assert_eq!(err.code, EditorErrorCode::SessionGone);
    }

    // This task's own named RED case: saving a workspace must sanitize what
    // it is handed (an unknown/mistyped key never reaches disk) and must
    // NEVER advance — or even touch — the project's revision or history.
    #[test]
    fn workspace_save_sanitizes_and_never_changes_the_revision() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let session_id = opened_session(root.path(), &state, "proj1");

        save_workspace_in(
            &state,
            root.path(),
            &session_id,
            serde_json::json!({
                "snap": false,
                "theme": "light",
                "bogus_future_field": "must not round-trip",
                "timeline_zoom": "not-a-number",
            }),
        )
        .unwrap();

        let stored = read_workspace(root.path(), "proj1").unwrap();
        assert_eq!(
            stored,
            serde_json::json!({ "snap": false, "theme": "light" }),
            "an unrecognized/mistyped key must never reach disk"
        );

        // MUTATION CHECK: a `save_workspace_in` that (wrongly) bumped the
        // session's revision, or rewrote `project.json`, would turn this
        // assertion red — the on-disk envelope's revision must be exactly
        // what `create_project` minted it at.
        let (envelope, _sources) = load_project(root.path(), "proj1").unwrap();
        assert_eq!(envelope.record.revision, 1);
    }

    #[test]
    fn oversized_workspace_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let session_id = opened_session(root.path(), &state, "proj1");
        // A single field far past the 64 KiB cap. `library_tab` is a plain
        // string field `sanitize` would otherwise keep whole, so this is
        // not refused for being unrecognized — only for being oversized.
        let huge = "x".repeat(70_000);

        let err = save_workspace_in(
            &state,
            root.path(),
            &session_id,
            serde_json::json!({ "library_tab": huge }),
        )
        .unwrap_err();

        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(
            err.message.contains("byte"),
            "expected a size-specific message, got: {}",
            err.message
        );
        // MUTATION CHECK: a refusal that still wrote the file (checking the
        // size AFTER the write, or not at all) leaves `workspace.json` on
        // disk despite the error — the read must still degrade to empty.
        let stored = read_workspace(root.path(), "proj1").unwrap();
        assert_eq!(stored, serde_json::json!({}));
    }

    #[test]
    fn save_workspace_refuses_an_unknown_session() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();

        let err =
            save_workspace_in(&state, root.path(), "ses-nope", serde_json::json!({})).unwrap_err();

        assert_eq!(err.code, EditorErrorCode::SessionGone);
    }
}
