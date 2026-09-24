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
//! **Guide progress (Task 55; F-46, F-47; R16, R18)** is the other
//! preference this module owns, and the one that is NOT per project:
//! `editor-prefs\guide-progress.json` under the app's local data root,
//! read and written by `editor_get_guide_progress`/
//! `editor_save_guide_progress` with no session at all. What the document
//! may hold is `core::editor::guide`'s to decide (a closed schema, the
//! lessons compiled in from the webview's own `steps.json`, 16 KiB); this
//! module only finds the file, reads it bounded and no-follow, and writes it
//! on the same `write_atomic_replacing` rails as `workspace.json`.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST — `authz_guard.rs` fails
//! naming any that does not.

use std::path::Path;

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::guide::{self, GuideProgress};
use vault_buddy_core::editor::{sanitize, EditorError, EditorErrorCode};

use super::authz::{require_editor_window, require_session};
use super::project_store::project_dir;
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
    let raw: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!(
                "editor workspace: {} is not valid JSON, degrading to empty ({e})",
                path.display()
            );
            Value::Object(serde_json::Map::new())
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(serde_json::Map::new()),
        Err(e) => return Err(internal(format!("Cannot read {}: {e}", path.display()))),
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
    write_atomic_replacing(&dir.join(WORKSPACE_FILE), &json)
        .map_err(|e| internal(format!("Could not save the workspace: {e}")))
}

/// The app-wide preference folder under the app's local data root —
/// deliberately OUTSIDE `editor-projects\`: guide progress belongs to the
/// person, not to any project (R16).
pub(crate) const PREFS_DIR: &str = "editor-prefs";
pub(crate) const GUIDE_PROGRESS_FILE: &str = "guide-progress.json";

/// Reads at most one byte past the bound, never following a link: the file
/// and its folder must be plain. `Ok(None)` = nothing saved yet.
fn read_progress_bytes(root: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let dir = root.join(PREFS_DIR);
    let path = dir.join(GUIDE_PROGRESS_FILE);
    for (p, want_dir) in [(&dir, true), (&path, false)] {
        match std::fs::symlink_metadata(p) {
            // `symlink_metadata` reports a link as neither file nor folder.
            Ok(m) if (want_dir && m.is_dir()) || (!want_dir && m.is_file()) => {}
            Ok(_) => return Err(std::io::Error::other("not a plain file or folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)?
        .take(guide::MAX_GUIDE_PROGRESS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// The saved guide progress, or fresh progress. A missing file is a first
/// run; a malformed or oversized one is logged (never its content) and read
/// as fresh — the next save replaces it. Storage that is THERE but cannot
/// be read (permission, a link wearing the name) is the one error: the
/// webview then keeps its progress for the session only and says so,
/// rather than starting over and later overwriting what it could not read.
pub(crate) fn read_guide_progress(root: &Path) -> Result<GuideProgress, EditorError> {
    let bytes = match read_progress_bytes(root) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(GuideProgress::default()),
        Err(e) => {
            log::warn!("editor guide progress: cannot read it ({e})");
            return Err(internal(format!("Guide progress could not be read: {e}")));
        }
    };
    Ok(guide::parse_stored(&bytes).unwrap_or_else(|| {
        log::warn!(
            "editor guide progress: the saved file is oversized or malformed, starting fresh"
        );
        GuideProgress::default()
    }))
}

fn map_prefs_write_error(e: std::io::Error) -> EditorError {
    #[cfg(windows)]
    let full = e.kind() == std::io::ErrorKind::StorageFull || e.raw_os_error() == Some(112);
    #[cfg(not(windows))]
    let full = e.kind() == std::io::ErrorKind::StorageFull;
    if full {
        err(
            EditorErrorCode::DiskFull,
            "Not enough disk space to save guide progress.",
        )
    } else if e.kind() == std::io::ErrorKind::PermissionDenied {
        err(
            EditorErrorCode::WriteDenied,
            "Guide progress could not be saved: permission denied.",
        )
    } else {
        internal(format!("Guide progress could not be saved: {e}"))
    }
}

/// `editor-prefs\`, created on first save. The local-data root may not
/// exist yet on a first run, so it gets `create_dir_all`; the prefs folder
/// itself `create_dir`, and it must then be a real folder — a link wearing
/// its name is refused rather than written through.
fn ensure_prefs_dir(root: &Path) -> Result<std::path::PathBuf, EditorError> {
    std::fs::create_dir_all(root).map_err(map_prefs_write_error)?;
    let dir = root.join(PREFS_DIR);
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(map_prefs_write_error(e)),
    }
    let meta = std::fs::symlink_metadata(&dir).map_err(map_prefs_write_error)?;
    if !meta.is_dir() {
        return Err(err(
            EditorErrorCode::WriteDenied,
            "Guide progress could not be saved: its folder is not a plain folder.",
        ));
    }
    Ok(dir)
}

/// Validates (strictly — `guide::validate_for_save`: 16 KiB, a closed
/// schema, known lesson ids only) BEFORE anything is created, then writes
/// through `write_atomic_replacing`. A refused save leaves the last good
/// file byte-identical.
pub(crate) fn save_guide_progress_in(root: &Path, progress: Value) -> Result<(), EditorError> {
    let progress = guide::validate_for_save(&progress)?;
    let json = serde_json::to_string_pretty(&progress)
        .map_err(|e| internal(format!("Could not encode guide progress: {e}")))?;
    let dir = ensure_prefs_dir(root)?;
    write_atomic_replacing(&dir.join(GUIDE_PROGRESS_FILE), &json).map_err(map_prefs_write_error)
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

/// ASYNC: one bounded file read off the main thread. App-wide, so it names
/// no session — the guide is the person's, not a project's (R16). Errors
/// only when the storage exists and cannot be read (`read_guide_progress`).
#[tauri::command]
pub async fn editor_get_guide_progress(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<GuideProgress, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || read_guide_progress(&root)).await
}

/// ASYNC: an fsync'd temp-then-replacing-rename write off the main thread.
/// Takes the RAW document so the 16 KiB bound and the closed schema are
/// this module's refusal (`invalidRequest`), not Tauri's opaque decode one.
#[tauri::command]
pub async fn editor_save_guide_progress(
    window: WebviewWindow,
    app: AppHandle,
    progress: Value,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || save_guide_progress_in(&root, progress)).await
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

    fn progress(overrides: Value) -> Value {
        let mut base = serde_json::json!({
            "contentRevision": 1,
            "currentStepId": "layout",
            "reviewed": ["welcome", "tracks"],
            "explored": ["tracks"],
            "invitationDismissed": true,
            "active": true,
            "collapsed": true,
            "completed": false,
            "preferences": { "dimming": false, "motion": "reduced" }
        });
        for (k, v) in overrides.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    fn progress_file(root: &Path) -> std::path::PathBuf {
        root.join(PREFS_DIR).join(GUIDE_PROGRESS_FILE)
    }

    #[test]
    fn guide_progress_round_trips_through_the_app_wide_prefs_folder() {
        let root = tempfile::tempdir().unwrap();

        save_guide_progress_in(root.path(), progress(serde_json::json!({}))).unwrap();

        assert!(progress_file(root.path()).is_file());
        let read = read_guide_progress(root.path()).unwrap();
        assert_eq!(read.current_step_id.as_deref(), Some("layout"));
        assert_eq!(read.reviewed, ["welcome", "tracks"]);
        assert!(read.collapsed);
        // App-wide: nothing lands in the project store.
        assert!(!root.path().join("editor-projects").exists());
    }

    // This task's named Rust case: a save naming a lesson this build does
    // not know, or carrying anything path-shaped, is refused and writes
    // NOTHING — the last good file stays byte-identical.
    #[test]
    fn guide_progress_rejects_unknown_ids_and_paths() {
        let root = tempfile::tempdir().unwrap();
        save_guide_progress_in(root.path(), progress(serde_json::json!({}))).unwrap();
        let before = std::fs::read(progress_file(root.path())).unwrap();

        let refused = [
            progress(serde_json::json!({ "currentStepId": "trim" })),
            progress(serde_json::json!({ "currentStepId": r"C:\Users\me\capture.mp4" })),
            progress(serde_json::json!({ "reviewed": [r"..\..\project.json"] })),
            progress(serde_json::json!({ "explored": ["welcome", "/tmp/x"] })),
            progress(serde_json::json!({ "mediaPath": r"C:\Users\me\capture.mp4" })),
        ];
        for raw in refused {
            let err = save_guide_progress_in(root.path(), raw.clone())
                .expect_err(&format!("{raw} must be refused"));
            assert_eq!(err.code, EditorErrorCode::InvalidRequest);
            assert!(!err.message.contains("Users"), "{}", err.message);
        }
        assert_eq!(std::fs::read(progress_file(root.path())).unwrap(), before);
    }

    #[test]
    fn a_missing_malformed_or_oversized_file_reads_as_fresh_progress() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            read_guide_progress(root.path()).unwrap(),
            GuideProgress::default()
        );

        std::fs::create_dir(root.path().join(PREFS_DIR)).unwrap();
        std::fs::write(progress_file(root.path()), "{ not json").unwrap();
        assert_eq!(
            read_guide_progress(root.path()).unwrap(),
            GuideProgress::default()
        );

        let mut huge = progress(serde_json::json!({})).to_string();
        huge.push_str(&" ".repeat(guide::MAX_GUIDE_PROGRESS_BYTES));
        std::fs::write(progress_file(root.path()), huge).unwrap();
        assert_eq!(
            read_guide_progress(root.path()).unwrap(),
            GuideProgress::default()
        );
    }

    // Storage that is there but cannot be read is NOT fresh progress: the
    // webview must learn it, so it can say "Session only" instead of
    // quietly starting over and then overwriting what it could not read.
    #[test]
    fn a_progress_path_that_is_not_a_plain_file_is_an_error_not_fresh_progress() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(progress_file(root.path())).unwrap();

        let err = read_guide_progress(root.path()).unwrap_err();

        assert_eq!(err.code, EditorErrorCode::Internal);
        assert_eq!(
            err.message,
            "Guide progress could not be read: not a plain file or folder"
        );
    }

    #[test]
    fn a_stored_lesson_this_build_does_not_know_resumes_at_the_first_lesson() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(PREFS_DIR)).unwrap();
        let stored = progress(
            serde_json::json!({ "currentStepId": "trim", "reviewed": ["trim", "welcome"] }),
        );
        std::fs::write(progress_file(root.path()), stored.to_string()).unwrap();

        let read = read_guide_progress(root.path()).unwrap();

        assert_eq!(read.current_step_id.as_deref(), Some("welcome"));
        assert_eq!(read.reviewed, ["welcome"]);
        assert_eq!(read.preferences.motion, guide::GuideMotion::Reduced);
    }

    #[test]
    fn get_workspace_of_a_project_with_no_saved_prefs_reads_the_sanitized_empty_blob() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let session_id = opened_session(root.path(), &state, "proj1");

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
