//! Save, list and reopen tutorial projects (F-40, F-44) — the durable half
//! of the editor's session lifecycle. `editor/session_commands.rs` opens a
//! staged capture into a session and applies commands to it, but nothing
//! there writes `project.json` past `create_project`'s one-time mint; this
//! module is what turns a session's in-memory edits into a persisted
//! revision, lists what is on disk, and reopens a project by id once its
//! session has closed.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (R8) — `authz_guard.rs`
//! fails naming any that does not. As in `session_commands.rs`, the logic
//! behind each command lives in an `AppHandle`-free function
//! (`save_project_in`/`open_project_session`) so it is unit-testable on a
//! tempdir.

use std::io;
use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::{
    self, sanitize, EditorError, EditorErrorCode, EditorOpenResult, Map, Record, WorkspaceEnvelope,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::session_commands::{missing_media, register_session};
use super::store_io::{
    self, commit_project, load_project, source_base_of, ProjectSummaryDto, ProjectWriter,
    RealWriter,
};
use super::EditorState;

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

fn local_data(app: &AppHandle) -> Result<std::path::PathBuf, EditorError> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| internal(format!("Could not resolve the app data directory: {e}")))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| internal(format!("The editor task failed: {e}")))?
}

/// `editor_save_project`'s result (Contract reference `SaveReceipt`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReceipt {
    pub session_id: String,
    pub saved_revision: u64,
    pub project_file_id: String,
}

/// Maps a `project.json` write's `io::Error` to a wire code the frontend
/// can act on distinctly from every other failure: **disk full** and
/// **permission denied** are the two ordinary states a save can hit on a
/// real machine. `ErrorKind::StorageFull` is the portable case (Linux's
/// ENOSPC maps to it too); the raw OS 112 check is Windows' own
/// `ERROR_DISK_FULL` code, checked directly rather than assumed to already
/// be folded into `StorageFull` on every platform this crate might run on.
/// Anything else (a vanished volume, a path that became invalid mid-write,
/// …) degrades to `internal` — this task has no test evidence to classify
/// it more precisely.
fn map_write_error(e: io::Error) -> EditorError {
    if e.kind() == io::ErrorKind::StorageFull || e.raw_os_error() == Some(112) {
        EditorError::new(
            EditorErrorCode::DiskFull,
            format!("Not enough disk space to save the project: {e}"),
        )
    } else if e.kind() == io::ErrorKind::PermissionDenied {
        EditorError::new(
            EditorErrorCode::WriteDenied,
            format!("Permission denied while saving the project: {e}"),
        )
    } else {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Could not save the project: {e}"),
        )
    }
}

/// The `AppHandle`-free half of `editor_save_project`, injectable with a
/// `ProjectWriter` so tests can simulate a disk-full/permission-denied
/// write without touching the real filesystem's own failure modes.
///
/// **Order is the whole atomicity guarantee this task's mutation check
/// exists to pin**: the envelope is built and written FIRST, and
/// `mark_saved` runs only once `commit_project` returns `Ok` — a save that
/// fails must leave the session's `persistedRevision` exactly where it was,
/// never claim a revision that was never actually written to disk.
///
/// The session mutex is held only twice, briefly, never across the write:
/// once to read the project + confirm the revision, once afterward to mark
/// it saved. A revision mismatch is `revisionConflict` — the same check
/// `EditorSession::execute` makes, applied to the save request instead of a
/// command.
pub(crate) fn save_project_with(
    writer: &dyn ProjectWriter,
    state: &EditorState,
    root: &Path,
    session_id: &str,
    expected_revision: u64,
) -> Result<SaveReceipt, EditorError> {
    let (project, revision) = {
        let sessions = require_session(state, session_id)?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| internal("session vanished under its own lock"))?;
        let snap = session.snapshot();
        if snap.revision != expected_revision {
            return Err(EditorError::new(
                EditorErrorCode::RevisionConflict,
                format!(
                    "expected revision {expected_revision} but the session is at {}",
                    snap.revision
                ),
            ));
        }
        (session.project().clone(), snap.revision)
    };
    let project_id = project.id.clone();

    // The workspace preference blob and the record's `createdAt` are not
    // owned by the in-memory session at all -- both live only in the last
    // saved envelope, so this reads it back rather than inventing either.
    // Task 18 replaces this "last saved workspace or {}" read with the
    // sanitized LIVE workspace once a command exists to change it; until
    // then, carrying the on-disk value forward is the only honest choice.
    // A missing/unreadable `project.json` (the project vanished out from
    // under a live session) degrades to a fresh `{}` workspace and `now` as
    // `createdAt` rather than failing the save outright.
    let now = chrono::Local::now().to_rfc3339();
    let (workspace, created_at) = match load_project(root, &project_id) {
        Ok((envelope, _sources)) => (envelope.workspace, envelope.record.created_at),
        Err(_) => (serde_json::json!({}), now.clone()),
    };

    let envelope = WorkspaceEnvelope {
        schema: editor::WORKSPACE_SCHEMA.to_string(),
        project,
        workspace,
        record: Record {
            id: project_id.clone(),
            revision,
            created_at,
            updated_at: now.clone(),
            // Products arrive in Task 46 (render/publish); nothing before
            // it can populate this array, so every save writes it empty.
            products: Vec::new(),
            extra: Map::new(),
        },
        saved_at: now,
        extra: Map::new(),
    };

    commit_project(writer, root, &project_id, &envelope).map_err(map_write_error)?;

    // MUTATION CHECK: calling `mark_saved` before the write above (or
    // unconditionally, ignoring the write's Result) makes
    // `injected_write_failure_keeps_the_last_good_file` fail — the session
    // would report a revision as persisted that a failed write never
    // actually landed on disk.
    let mut sessions = lock_ignoring_poison(&state.sessions);
    if let Some(session) = sessions.get_mut(session_id) {
        session.mark_saved(revision);
    }
    Ok(SaveReceipt {
        session_id: session_id.to_string(),
        saved_revision: revision,
        project_file_id: project_id,
    })
}

pub(crate) fn save_project_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    expected_revision: u64,
) -> Result<SaveReceipt, EditorError> {
    save_project_with(&RealWriter, state, root, session_id, expected_revision)
}

/// The `AppHandle`-free half of `editor_open_project`: load an existing
/// project by id and register — or reuse — a session over it. Held under
/// the SAME `open` lock and live-session reuse as `open_staged_session`
/// (`session_commands.rs`'s module doc): two callers opening the same
/// project by id must land on one session, not two racing ones, exactly
/// the reason that lock exists for the staged-capture path.
pub(crate) fn open_project_session(
    state: &EditorState,
    root: &Path,
    project_file_id: &str,
) -> Result<EditorOpenResult, EditorError> {
    let _open = lock_ignoring_poison(&state.open);
    let (envelope, sources) = load_project(root, project_file_id)?;
    let workspace = sanitize(&envelope.workspace);
    let projection = register_session(state, envelope.project);
    let missing = missing_media(root, &projection.project, &sources);
    Ok(EditorOpenResult {
        snapshot: projection.snapshot,
        project: projection.project,
        workspace,
        missing,
        source_base: source_base_of(&sources),
        recovered: false,
    })
}

/// ASYNC: reads (and, on success, rewrites) `project.json` off the main
/// thread.
#[tauri::command]
pub async fn editor_save_project(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    expected_revision: u64,
) -> Result<SaveReceipt, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        save_project_in(
            &app.state::<EditorState>(),
            &root,
            &session_id,
            expected_revision,
        )
    })
    .await
}

/// ASYNC: a `read_dir` of the store plus one `project.json` parse per
/// project — `store_io::list_projects`, already sorted `updatedAt` desc.
#[tauri::command]
pub async fn editor_list_projects(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Vec<ProjectSummaryDto>, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || Ok(store_io::list_projects(&root))).await
}

/// `use_recovery` is Task 37's — `recovery.json` does not exist yet, so
/// anything other than `false` is `invalidRequest` rather than silently
/// ignored (R20: nothing is faked). Split out as a pure check so it is
/// unit-testable without a live `WebviewWindow`.
fn refuse_recovery_flag(use_recovery: bool) -> Result<(), EditorError> {
    if use_recovery {
        Err(err(
            EditorErrorCode::InvalidRequest,
            "Recovery is not available yet.",
        ))
    } else {
        Ok(())
    }
}

/// ASYNC: reads `project.json` and registers (or reuses) a session over it.
#[tauri::command]
pub async fn editor_open_project(
    window: WebviewWindow,
    app: AppHandle,
    project_file_id: String,
    use_recovery: bool,
) -> Result<EditorOpenResult, EditorError> {
    require_editor_window(&window)?;
    refuse_recovery_flag(use_recovery)?;
    let root = local_data(&app)?;
    blocking(move || open_project_session(&app.state::<EditorState>(), &root, &project_file_id))
        .await
}

#[cfg(test)]
#[path = "save_commands_tests.rs"]
mod tests;
