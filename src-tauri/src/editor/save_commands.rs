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
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::{
    self, sanitize, EditorError, EditorErrorCode, EditorOpenResult, EditorSession, Map, Record,
    WorkspaceEnvelope,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::prefs_commands::read_workspace;
use super::recovery::load_journal;
use super::session_commands::{missing_media, register_session, register_session_with};
use super::store_io::{
    self, commit_project, load_project, project_file_exists, source_base_of, ProjectSummaryDto,
    ProjectWriter, RealWriter,
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
/// ENOSPC maps to it too) and is checked on every platform; the raw OS 112
/// check is Windows' own `ERROR_DISK_FULL` code, gated `#[cfg(windows)]`
/// (fix round 1) because 112 names a DIFFERENT condition on other
/// platforms — `EHOSTDOWN` on Linux — so checking it unconditionally would
/// misclassify an unrelated Linux error as `diskFull`. Anything else (a
/// vanished volume, a path that became invalid mid-write, …) degrades to
/// `internal` — this task has no test evidence to classify it more
/// precisely.
fn map_write_error(e: io::Error) -> EditorError {
    #[cfg(windows)]
    let is_disk_full = e.kind() == io::ErrorKind::StorageFull || e.raw_os_error() == Some(112);
    #[cfg(not(windows))]
    let is_disk_full = e.kind() == io::ErrorKind::StorageFull;

    if is_disk_full {
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

/// The per-session save lock (`EditorState::save_locks`'s own doc): finds
/// — or, on a session's first save, mints — the `Arc<Mutex<()>>` keyed on
/// `session_id`. The map mutex is held only for this lookup/insert, never
/// across the save itself; the returned `Arc` is what the caller then locks
/// and holds for its whole read-revision-through-commit-through-mark_saved
/// sequence. `close_in`'s `discardProject` (`session_commands.rs`) takes
/// the SAME lock for the same reason — it must not remove the project
/// directory out from under an in-flight save (Task 12 fix round 2,
/// finding 2).
///
/// **Refuses `sessionGone` BEFORE minting anything** (fix round 2, finding
/// 4): a lock entry created for a session id that turns out not to exist
/// would never be pruned by `drop_session` (which only removes an entry for
/// a session it is ACTUALLY closing), so the map would grow with every
/// stale/bogus/already-closed id a caller ever passes. Checking first, via
/// the same `require_session` every other caller uses, means a save (or a
/// discard) on an unknown session leaves `save_locks` untouched.
///
/// **Check, insert, RE-check** (Task 12 review, carried to Task 37): the
/// session can be dropped between the first check and the insert —
/// `drop_session` prunes `save_locks` only for an entry that already
/// exists, so an insert landing after its prune would be an entry nothing
/// ever removes. Re-checking after the insert closes that: a session gone
/// by then has its fresh entry removed here and the caller gets
/// `sessionGone`. Neither lock is held while the other is taken, so this
/// adds no lock-ordering rule (`drop_session` takes `sessions` then the
/// `save_locks` map; this takes them one at a time).
pub(crate) fn session_save_lock(
    state: &EditorState,
    session_id: &str,
) -> Result<Arc<Mutex<()>>, EditorError> {
    drop(require_session(state, session_id)?);
    let lock = lock_ignoring_poison(&state.save_locks)
        .entry(session_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    if let Err(gone) = require_session(state, session_id).map(drop) {
        lock_ignoring_poison(&state.save_locks).remove(session_id);
        return Err(gone);
    }
    Ok(lock)
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
/// **Fix round 1, finding 2**: the WHOLE function body runs under
/// `session_save_lock`, held for the entire read-revision → commit →
/// mark_saved sequence — without it, two concurrent saves on the SAME
/// session can interleave (A reads revision 2, an edit advances the
/// session to 3, B reads revision 3, and depending on which write and
/// which `mark_saved` lands last, `persistedRevision` can end up claiming
/// a revision that disagrees with what is actually on disk). The
/// `sessions` mutex itself is still taken only twice, briefly, INSIDE that
/// lock: once to read the project + confirm the revision, once afterward
/// to mark it saved — never across the write. A revision mismatch is
/// `revisionConflict` — the same check `EditorSession::execute` makes,
/// applied to the save request instead of a command.
pub(crate) fn save_project_with(
    writer: &dyn ProjectWriter,
    state: &EditorState,
    root: &Path,
    session_id: &str,
    expected_revision: u64,
) -> Result<SaveReceipt, EditorError> {
    let session_lock = session_save_lock(state, session_id)?;
    let _save_guard = lock_ignoring_poison(&session_lock);

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

    // The record's `createdAt` is not owned by the in-memory session at
    // all -- it lives only in the last saved envelope, so this reads it
    // back rather than inventing it.
    //
    // Fix round 1, finding 1 (narrowed further in fix round 2, finding 1):
    // a `load_project` failure is NOT one thing. Round 1 only special-cased
    // `invalidProject`, which still left every OTHER failure -- a
    // `project.json` that exists but cannot be READ (`PermissionDenied`, a
    // Windows sharing violation), or a missing/unreadable/corrupt
    // `sources.json` beside a perfectly valid `project.json` -- degrading
    // and silently overwriting a file that IS there. The only condition
    // that may still degrade is `project.json` being GENUINELY ABSENT,
    // checked EXPLICITLY and FIRST via `project_file_exists` rather than
    // inferred from `load_project`'s error shape: every other failure
    // refuses the save with that error, unmodified, before anything is
    // written. The one remaining degrade is still logged rather than
    // swallowed (AGENTS.md's "no swallowed error" diagnostics invariant).
    let now = chrono::Local::now().to_rfc3339();
    let created_at = if project_file_exists(root, &project_id).map_err(|e| {
        internal(format!(
            "Could not save the project: could not check whether it was saved before: {e}"
        ))
    })? {
        match load_project(root, &project_id) {
            Ok((envelope, _sources)) => envelope.record.created_at,
            Err(e) => return Err(e),
        }
    } else {
        log::warn!(
            "editor_save_project: no last-saved envelope for project {project_id:?} \
             (project.json is absent), stamping createdAt fresh"
        );
        now.clone()
    };

    // Task 18 (F17): the envelope's `workspace` field is the sanitized LIVE
    // `workspace.json` this task introduced, read through the same helper
    // `editor_get_workspace` uses -- never a raw file read, and never the
    // stale "last saved envelope's own workspace" this used to carry
    // forward. `read_workspace` already degrades a missing/malformed file
    // to the sanitized empty blob, so a project with no saved preferences
    // yet still saves cleanly.
    // Task 18 fix round 1, finding 2: a transient `workspace.json` read
    // failure must not refuse the user's PROJECT save -- only
    // `editor_get_workspace`'s own path keeps the hard error.
    // `read_workspace` already degrades a missing/malformed file
    // internally; this degrades every OTHER failure (permission denied, a
    // Windows sharing violation -- exactly the class a debounced
    // `editor_save_workspace` write racing this very read can produce,
    // GAP-169) the same way: to the sanitized empty blob, logged rather
    // than swallowed (AGENTS.md's diagnostics invariant), never propagated
    // through the `?` that used to sit here.
    let workspace = read_workspace(root, &project_id).unwrap_or_else(|e| {
        log::warn!(
            "editor_save_project: could not read workspace.json for project {project_id:?}, \
             embedding the sanitized empty blob instead of refusing the project save: {}",
            e.message
        );
        serde_json::json!({})
    });

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
    let current = lock_ignoring_poison(&state.sessions)
        .get_mut(session_id)
        .map(|session| {
            session.mark_saved(revision);
            session.snapshot().revision
        });
    // Task 37: a save of the session's CURRENT revision leaves nothing to
    // recover, so its journal goes (still under the save lock, so no
    // journal write can land after this delete). A save of an OLDER
    // revision (an edit landed during the write) keeps it: that edit exists
    // only in memory and in the journal.
    if current == Some(revision) {
        state.journal.forget(session_id);
        if let Err(e) = super::recovery::remove_journal(root, &project_id) {
            log::warn!(
                "editor_save_project: saved {project_id:?} but could not remove its                  recovery journal: {e}"
            );
        }
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
    use_recovery: bool,
) -> Result<EditorOpenResult, EditorError> {
    let _open = lock_ignoring_poison(&state.open);
    let (envelope, sources) = load_project(root, project_file_id)?;
    let workspace = sanitize(&envelope.workspace);
    let committed = envelope.record.revision;
    let (projection, recovered) = if use_recovery {
        // Task 37: the journal is the working copy; the store's committed
        // revision stays the persisted one, so the session opens DIRTY. The
        // working revision never falls to or below the committed one (a
        // journal older than the last save must not make time run
        // backwards, the Task 12 monotonic-revision ruling).
        let journal = load_journal(root, project_file_id)?;
        let revision = journal.session_revision.max(committed + 1);
        register_session_with(state, journal.project, |id, project| {
            EditorSession::resume_recovered(id, project, revision, committed)
        })
    } else {
        (register_session(state, envelope.project, committed), false)
    };
    let missing = missing_media(root, &projection.project, &sources);
    Ok(EditorOpenResult {
        snapshot: projection.snapshot,
        project: projection.project,
        workspace,
        missing,
        source_base: source_base_of(&sources),
        recovered,
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

/// ASYNC: reads `project.json` (and, with `use_recovery`, `recovery.json`
/// as the working copy — Task 37) and registers (or reuses) a session.
#[tauri::command]
pub async fn editor_open_project(
    window: WebviewWindow,
    app: AppHandle,
    project_file_id: String,
    use_recovery: bool,
) -> Result<EditorOpenResult, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        open_project_session(
            &app.state::<EditorState>(),
            &root,
            &project_file_id,
            use_recovery,
        )
    })
    .await
}

#[cfg(test)]
#[path = "save_commands_tests.rs"]
mod tests;
