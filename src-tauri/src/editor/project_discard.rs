//! Discard a tutorial project that has no session (final whole-branch
//! review I3): `editor_discard_project`.
//!
//! Every other discard runs through a session (`editor_close_session` with
//! `discardProject`), and a session needs the project to OPEN. A project
//! whose `sources.json` or `project.json` no longer parses cannot open, so
//! it could never be discarded — and the capture it pinned could not be
//! discarded either (a pinned capture refuses Discard, R6). This is the way
//! out, offered by the editor window where the user meets it: the "could
//! not be opened" line of a project that is damaged.
//!
//! - Refused while a session holds the project: that discard must go
//!   through the session, which stops its jobs, takes and journal first
//!   (`discard.rs`). Checked under `EditorState::open`, which is held for
//!   the whole discard, so no open can register a session in between.
//! - Every staged capture pinned to the project is unpinned — found by a
//!   scan of the staging sidecars, not through `sources.json` (which may be
//!   exactly what is damaged, and a hand-edited pin elsewhere may name this
//!   project too). A pin naming any other project is never touched.
//! - Then `store_io::remove_project`: ownership proven by `project.json`'s
//!   own `project.id`, owned files only, no-follow, `project.json` last.
//!   Bytes that are not JSON at all prove nothing, so nothing is removed.
//!
//! Like every `editor_*` command it takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (`authz_guard.rs`).

use std::path::Path;

use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::{is_valid_id, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use super::authz::require_editor_window;
use super::prefs_commands::{blocking, local_data};
use super::project_store::{pinned_project, project_dir, unpin_staged};
use super::redact::redact_name;
use super::store_io::remove_project;
use super::EditorState;

fn err(code: EditorErrorCode, message: &str) -> EditorError {
    EditorError::new(code, message)
}

/// Clear every staged capture's pin that names `project_id`.
fn unpin_everywhere(staging_dir: &Path, project_id: &str) -> Result<(), EditorError> {
    let Ok(entries) = std::fs::read_dir(staging_dir) else {
        // No staging directory: nothing can be pinned to anything.
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(base) = name.strip_suffix(".json") else {
            continue;
        };
        if !crate::editor_commands::is_safe_base(base) {
            continue;
        }
        let pinned = staging::read_sidecar(&entry.path()).and_then(|s| pinned_project(&s));
        if pinned.as_deref() != Some(project_id) {
            continue;
        }
        unpin_staged(staging_dir, base, project_id).map_err(|e| {
            log::warn!(
                "editor_discard_project: could not unlink {} from its project: {e}",
                redact_name(base)
            );
            err(
                EditorErrorCode::Internal,
                "A capture linked to this project could not be released, so the project was \
                 kept. See the log for details.",
            )
        })?;
    }
    Ok(())
}

/// The `AppHandle`-free half of `editor_discard_project` — see the module
/// doc.
pub(crate) fn discard_project_in(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    project_id: &str,
) -> Result<(), EditorError> {
    if !is_valid_id(project_id) {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "That is not a project id.",
        ));
    }
    let _open = lock_ignoring_poison(&state.open);
    if lock_ignoring_poison(&state.by_project).contains_key(project_id) {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "This project is open in the editor. Discard it from its project menu.",
        ));
    }
    if !project_dir(root, project_id).is_some_and(|d| d.is_dir()) {
        return Err(err(
            EditorErrorCode::SourceMissing,
            "That project is no longer on disk.",
        ));
    }
    unpin_everywhere(staging_dir, project_id)?;
    remove_project(root, project_id)?;
    log::info!("editor_discard_project: discarded the project {project_id}");
    Ok(())
}

/// ASYNC: a staging scan, sidecar rewrites and a directory removal.
#[tauri::command]
pub async fn editor_discard_project(
    window: WebviewWindow,
    app: AppHandle,
    project_file_id: String,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        let staging_dir = staging::staging_dir(&root);
        discard_project_in(
            &app.state::<EditorState>(),
            &root,
            &staging_dir,
            &project_file_id,
        )
    })
    .await
}

#[cfg(test)]
#[path = "project_discard_tests.rs"]
mod tests;
