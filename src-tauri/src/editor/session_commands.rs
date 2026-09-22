//! The editor's session lifecycle over IPC (F-01, F-13): open a staged
//! capture into a project + session, fetch a snapshot, execute a command,
//! close the session, hide the window.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (R8) — `authz_guard.rs`
//! fails naming any that does not. The logic behind each command lives in
//! an `AppHandle`-free function (`open_staged_in`, `execute_in`, …) so it
//! is unit-testable on a tempdir, the `clear_staged` / `discard_conflict`
//! precedent.
//!
//! **Lock order** (`EditorState`'s two maps): `by_project` BEFORE
//! `sessions`, whenever both are held. Neither is ever held across disk
//! I/O: every function here does its disk work first and takes the maps
//! only for the in-memory register/apply/remove.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::{
    migrate, new_entity_id, new_project_id, sanitize, validate_project, EditorError,
    EditorErrorCode, EditorOpenResult, EditorProjection, EditorSession, ExecuteRequest,
    MissingMedia, Project, WorkspaceEnvelope,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use super::authz::{require_editor_window, require_session};
use super::project_store::{
    pin_staged, pinned_project, project_dir, resolve_source, unpin_staged, SourceLocator,
    SourceMediaKind, SourceRecord,
};
use super::store_io::{create_project, list_projects, load_project, load_sources, remove_project};
use super::EditorState;
use crate::editor_commands::is_safe_base;

/// The asset id `migrate::from_staged` gives the capture itself, and so the
/// key its `sources.json` entry lives under.
const STAGED_ASSET_ID: &str = "src";

/// F7's refusal, verbatim from the brief.
const UNKNOWN_LENGTH: &str = "This recording's original data is gone — its length is unknown, \
     so it cannot be edited. You can still discard it.";

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

/// A project on disk, as `open_staged_in` found or made it.
pub(crate) struct OpenedProject {
    pub envelope: WorkspaceEnvelope,
    pub sources: BTreeMap<String, SourceRecord>,
}

/// Find — or, failing that, mint — the project editing staged capture
/// `base` (R6). `root` is the app's local data dir (the store lives at
/// `root/editor-projects`); `staging_dir` is the staging directory.
///
/// In order, and every refusal lands BEFORE anything is created or pinned:
/// an unsafe base, a missing sidecar, a sidecar whose own `base` disagrees
/// with the name it was read from, and (F7) a recovered or zero-length
/// capture — migrated as-is it would become an empty project, pinned for
/// good, whose capture nobody could ever discard again.
///
/// Then: a pin naming a project that still exists reopens it (a duplicated
/// `editor:open` never mints a second project); otherwise a project whose
/// `sources.json` already names this capture is ADOPTED and re-pinned — the
/// crash-between-create-and-pin case, where the project landed and the pin
/// did not; only if neither exists is a new one migrated, created, and THEN
/// pinned.
pub(crate) fn open_staged_in(
    root: &Path,
    staging_dir: &Path,
    base: &str,
) -> Result<OpenedProject, EditorError> {
    if !is_safe_base(base) {
        log::warn!("editor_open_staged: refused an unsafe base {base:?}");
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "That capture name is not one of ours.",
        ));
    }
    let sidecar = staging::read_sidecar(&staging_dir.join(staging::sidecar_file_name(base)))
        .ok_or_else(|| {
            err(
                EditorErrorCode::SourceMissing,
                "That staged capture is no longer on disk.",
            )
        })?;
    if sidecar.base != base {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "That capture's details do not match its name.",
        ));
    }
    if crate::staged_commands::summary_is_recovered(&sidecar.extra) || sidecar.duration_ms == 0 {
        return Err(err(EditorErrorCode::InvalidRequest, UNKNOWN_LENGTH));
    }

    if let Some(pid) = pinned_project(&sidecar) {
        if project_dir(root, &pid).is_some_and(|d| d.is_dir()) {
            return load_opened(root, &pid);
        }
        log::warn!(
            "editor_open_staged: {base:?} is pinned to missing project {pid:?}; re-adopting"
        );
    }

    if let Some(orphan) = list_projects(root)
        .into_iter()
        .find(|p| p.source_base.as_deref() == Some(base))
    {
        let opened = load_opened(root, &orphan.project_file_id)?;
        pin(staging_dir, base, &orphan.project_file_id)?;
        return Ok(opened);
    }

    let project_id = new_project_id();
    let migration = migrate::from_staged(
        &migrate::StagedInput {
            base,
            vault_id: &sidecar.vault_id,
            source_title: &sidecar.source_title,
            duration_ms: sidecar.duration_ms,
            width: sidecar.width,
            height: sidecar.height,
            has_audio: !sidecar.inputs.is_empty(),
            legacy_timeline: sidecar.timeline.as_ref(),
            stems: &[],
            webcam: None,
        },
        &project_id,
    );
    if migration.dropped_segments > 0 {
        log::warn!(
            "editor_open_staged: {base:?} had {} backwards segment(s), dropped in migration",
            migration.dropped_segments
        );
    }
    validate_project(&migration.project)?;
    let size = std::fs::metadata(staging_dir.join(staging::mp4_file_name(base)))
        .map(|m| m.len())
        .unwrap_or(0);
    let mut sources = BTreeMap::new();
    sources.insert(
        STAGED_ASSET_ID.to_string(),
        SourceRecord {
            locator: SourceLocator::Staging {
                base: base.to_string(),
            },
            sha256: None,
            size,
            duration_ms: sidecar.duration_ms,
            width: Some(sidecar.width),
            height: Some(sidecar.height),
            has_audio: !sidecar.inputs.is_empty(),
            has_video: true,
            media_kind: SourceMediaKind::Video,
        },
    );
    create_project(root, &migration.project, &sources)
        .map_err(|e| internal(format!("Could not create the project: {e}")))?;
    pin(staging_dir, base, &project_id)?;
    load_opened(root, &project_id)
}

fn load_opened(root: &Path, id: &str) -> Result<OpenedProject, EditorError> {
    let (envelope, sources) = load_project(root, id)?;
    Ok(OpenedProject { envelope, sources })
}

fn pin(staging_dir: &Path, base: &str, project_id: &str) -> Result<(), EditorError> {
    pin_staged(staging_dir, base, project_id)
        .map_err(|e| internal(format!("Could not link the capture to its project: {e}")))
}

/// The sources a project references whose file is not on disk. A `Builtin`
/// source has no file by design and is never "missing".
pub(crate) fn missing_media(
    root: &Path,
    project: &Project,
    sources: &BTreeMap<String, SourceRecord>,
) -> Vec<MissingMedia> {
    sources
        .iter()
        .filter(|(_, r)| r.locator != SourceLocator::Builtin)
        .filter(|(_, r)| !resolve_source(root, &project.id, r).is_some_and(|p| p.is_file()))
        .map(|(asset_id, r)| MissingMedia {
            asset_id: asset_id.clone(),
            name: project
                .assets
                .iter()
                .find(|a| &a.id == asset_id)
                .map_or_else(|| asset_id.clone(), |a| a.name.clone()),
            expected_size: r.size,
            expected_duration_ms: r.duration_ms,
        })
        .collect()
}

/// Register a session over `project`, or return the LIVE one already open on
/// it — a second open must not fork the project into two sessions whose
/// saves would race. A fresh session is marked saved at its first revision:
/// what it holds is exactly what is on disk.
pub(crate) fn register_session(state: &EditorState, project: Project) -> EditorProjection {
    let mut by_project = lock_ignoring_poison(&state.by_project);
    let mut sessions = lock_ignoring_poison(&state.sessions);
    if let Some(live) = by_project
        .get(&project.id)
        .and_then(|sid| sessions.get(sid))
    {
        return EditorProjection::of(live);
    }
    let session_id = new_entity_id("ses");
    let project_id = project.id.clone();
    let mut session = EditorSession::new(session_id.clone(), project);
    session.mark_saved(session.snapshot().revision);
    let projection = EditorProjection::of(&session);
    sessions.insert(session_id.clone(), session);
    by_project.insert(project_id, session_id);
    projection
}

/// `open_staged_in` + `register_session` + the open result.
pub(crate) fn open_staged_session(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    base: &str,
) -> Result<EditorOpenResult, EditorError> {
    let opened = open_staged_in(root, staging_dir, base)?;
    let workspace = sanitize(&opened.envelope.workspace);
    let projection = register_session(state, opened.envelope.project);
    let missing = missing_media(root, &projection.project, &opened.sources);
    Ok(EditorOpenResult {
        snapshot: projection.snapshot,
        project: projection.project,
        workspace,
        missing,
        source_base: Some(base.to_string()),
        recovered: false,
    })
}

/// The in-memory apply behind `editor_execute`: the session mutex is held
/// for the apply and the projection clone, nothing else.
pub(crate) fn execute_in(
    state: &EditorState,
    request: &ExecuteRequest,
) -> Result<EditorProjection, EditorError> {
    let mut sessions = require_session(state, &request.session_id)?;
    let session = sessions
        .get_mut(&request.session_id)
        .ok_or_else(|| internal("session vanished under its own lock"))?;
    session.execute(request)?;
    Ok(EditorProjection::of(session))
}

pub(crate) fn snapshot_in(
    state: &EditorState,
    session_id: &str,
) -> Result<EditorProjection, EditorError> {
    let sessions = require_session(state, session_id)?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| internal("session vanished under its own lock"))?;
    Ok(EditorProjection::of(session))
}

/// How `editor_close_session` leaves the project behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseDisposition {
    Keep,
    DiscardRecovery,
    DiscardProject,
}

fn drop_session(state: &EditorState, session_id: &str) {
    let mut by_project = lock_ignoring_poison(&state.by_project);
    let mut sessions = lock_ignoring_poison(&state.sessions);
    if let Some(session) = sessions.remove(session_id) {
        let project_id = &session.project().id;
        if by_project.get(project_id).map(String::as_str) == Some(session_id) {
            by_project.remove(project_id);
        }
    }
}

/// Close a session. `discardProject` UNPINS the staged capture first and
/// only then removes the project directory: a failure between the two
/// leaves an unpinned orphan the next open adopts back, whereas the
/// reverse order would leave a pin naming a deleted project, and a pinned
/// capture refuses Discard. The recording itself is never touched (R6).
/// On any failure the session stays open so the user can retry.
pub(crate) fn close_in(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    session_id: &str,
    disposition: CloseDisposition,
) -> Result<(), EditorError> {
    let project_id = {
        let sessions = require_session(state, session_id)?;
        sessions
            .get(session_id)
            .map(|s| s.project().id.clone())
            .ok_or_else(|| internal("session vanished under its own lock"))?
    };
    match disposition {
        CloseDisposition::Keep => {}
        CloseDisposition::DiscardRecovery => {
            return Err(err(
                EditorErrorCode::InvalidRequest,
                "Discarding recovered changes is not available yet.",
            ));
        }
        CloseDisposition::DiscardProject => {
            let sources = load_sources(root, &project_id)?;
            for record in sources.values() {
                if let SourceLocator::Staging { base } = &record.locator {
                    unpin_staged(staging_dir, base, &project_id).map_err(|e| {
                        internal(format!("Could not unlink the capture {base:?}: {e}"))
                    })?;
                }
            }
            remove_project(root, &project_id)?;
        }
    }
    drop_session(state, session_id);
    Ok(())
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

/// ASYNC: reads a sidecar and may create a project directory, write two
/// files and rewrite the sidecar.
#[tauri::command]
pub async fn editor_open_staged(
    window: WebviewWindow,
    app: AppHandle,
    staged_base: String,
) -> Result<EditorOpenResult, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        let staging_dir = staging::staging_dir(&root);
        open_staged_session(
            &app.state::<EditorState>(),
            &root,
            &staging_dir,
            &staged_base,
        )
    })
    .await
}

/// `knownRevision` is accepted for the contract (a later task may answer
/// "unchanged" without the graph); today the full projection is returned.
#[tauri::command]
pub async fn editor_get_snapshot(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    known_revision: Option<u64>,
) -> Result<EditorProjection, EditorError> {
    require_editor_window(&window)?;
    let _ = known_revision;
    snapshot_in(&app.state::<EditorState>(), &session_id)
}

/// ASYNC on the blocking pool: apply + validate can walk a large graph.
#[tauri::command]
pub async fn editor_execute(
    window: WebviewWindow,
    app: AppHandle,
    request: ExecuteRequest,
) -> Result<EditorProjection, EditorError> {
    require_editor_window(&window)?;
    blocking(move || execute_in(&app.state::<EditorState>(), &request)).await
}

/// ASYNC: `discardProject` unlinks a project directory.
#[tauri::command]
pub async fn editor_close_session(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    disposition: CloseDisposition,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        let staging_dir = staging::staging_dir(&root);
        close_in(
            &app.state::<EditorState>(),
            &root,
            &staging_dir,
            &session_id,
            disposition,
        )
    })
    .await
}

/// SYNC: a window call, so it runs on the main thread; it never blocks.
#[tauri::command]
pub fn editor_hide_window(window: WebviewWindow) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    window
        .hide()
        .map_err(|e| internal(format!("Could not hide the editor: {e}")))
}

#[cfg(test)]
#[path = "session_commands_tests.rs"]
mod tests;
