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
//! - Ownership is proven FIRST (`store_io::prove_ownership`: `project.json`'s
//!   own `project.id`), before any pin is released (GAP-214 item 8). Bytes
//!   that are not JSON at all, or a document naming another project, prove
//!   nothing: the discard is refused in fixed words, every pin untouched.
//! - Every staged capture pinned to the project is unpinned — found by a
//!   scan of the staging sidecars, not through `sources.json` (which may be
//!   exactly what is damaged, and a hand-edited pin elsewhere may name this
//!   project too). A pin naming any other project is never touched.
//! - Then `store_io::remove_project`: owned files only, no-follow,
//!   `project.json` last.
//!
//! Like every `editor_*` command it takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (`authz_guard.rs`).

use std::io;
use std::path::Path;

use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::{is_valid_id, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use super::authz::require_editor_window;
use super::errors::err;
use super::prefs_commands::{blocking, local_data};
use super::project_store::{pinned_project, project_dir, unpin_staged};
use super::redact::redact_name;
use super::store_io::{prove_ownership, remove_project};
use super::EditorState;

/// What a discard says when `project.json` does not prove the folder is this
/// project's (GAP-214 item 8): fixed role words — never the redaction
/// handle or the parser's own text the proof's message carries (that goes
/// to the log).
pub(crate) const NOT_ITS_FILES: &str =
    "This project could not be discarded because its files do not belong to it.";

/// `store_io::prove_ownership`, worded for a discard (through
/// `precheck_discard`). Run BEFORE any pin is released, so a refusal leaves
/// every capture pinned.
fn prove_owned_for_discard(root: &Path, project_id: &str) -> Result<(), EditorError> {
    prove_ownership(root, project_id).map_err(|e| {
        if e.code != EditorErrorCode::InvalidProject {
            return e;
        }
        log::warn!("project discard: refused {project_id}: {}", e.message);
        err(EditorErrorCode::InvalidProject, NOT_ITS_FILES)
    })
}

/// What a discard can learn before it touches anything: ownership
/// (`prove_owned_for_discard`) and that the staging folder can be listed
/// (so the pin scan will not refuse later). Run under `EditorState::open`
/// — pins cannot change while it is held, and a save keeps the project id —
/// FIRST in both discards: a session's `discardProject` runs it before its
/// closing mark, so a refusal here has cancelled no render, publish or
/// derived media (GAP-214 item 5; Task 4 fix round 1).
pub(super) fn precheck_discard(
    root: &Path,
    staging_dir: &Path,
    project_id: &str,
) -> Result<(), EditorError> {
    prove_owned_for_discard(root, project_id)?;
    match std::fs::read_dir(staging_dir) {
        Ok(_) => Ok(()),
        Err(e) => staging_unreadable(&e).map_or(Ok(()), Err),
    }
}

/// A staging folder that cannot be listed means the pins cannot be found,
/// so the discard is refused rather than read as "nothing pinned" — which
/// would remove the project and leave a pin to it. Only a folder that does
/// not EXIST holds no pins.
fn staging_unreadable(e: &io::Error) -> Option<EditorError> {
    if e.kind() == io::ErrorKind::NotFound {
        return None;
    }
    log::warn!("project discard: the staging folder could not be listed: {e}");
    Some(err(
        EditorErrorCode::Internal,
        "The captures linked to this project could not be checked, so the project was kept. \
         Try again in a moment.",
    ))
}

/// Clear every staged capture's pin that names `project_id` — shared with a
/// session's `discardProject` (`session_close::close_locked`, GAP-214 item
/// 7). The caller holds `EditorState::open`, like every pin writer.
///
/// A sidecar that cannot be read or parsed is skipped WITH a log line: it
/// may hold a pin to this project, which then outlives it — harmless since
/// a pin to a project that no longer exists refuses nothing (review finding
/// D-2, `store_io::pin_liveness`).
pub(super) fn unpin_everywhere(staging_dir: &Path, project_id: &str) -> Result<(), EditorError> {
    let entries = match std::fs::read_dir(staging_dir) {
        Ok(entries) => entries,
        Err(e) => return staging_unreadable(&e).map_or(Ok(()), Err),
    };
    for entry in entries {
        // An entry that vanished mid-scan holds no pin; any other failure
        // hides what it might have held.
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => match staging_unreadable(&e) {
                Some(refusal) => return Err(refusal),
                None => continue,
            },
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(base) = name.strip_suffix(".json") else {
            continue;
        };
        if !crate::editor_commands::is_safe_base(base) {
            continue;
        }
        let Some(sidecar) = staging::read_sidecar(&entry.path()) else {
            log::warn!(
                "project discard: the sidecar of {} could not be read; any pin it holds stays",
                redact_name(base)
            );
            continue;
        };
        if pinned_project(&sidecar).as_deref() != Some(project_id) {
            continue;
        }
        unpin_staged(staging_dir, base, project_id).map_err(|e| {
            log::warn!(
                "project discard: could not unlink {} from its project: {e}",
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
    precheck_discard(root, staging_dir, project_id)?;
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
    let discarded = project_file_id.clone();
    blocking(move || {
        let staging_dir = staging::staging_dir(&root);
        discard_project_in(
            &app.state::<EditorState>(),
            &root,
            &staging_dir,
            &project_file_id,
        )
    })
    .await?;
    // GAP-208: a discarded project is not what a reload reopens.
    crate::editor_commands::note_editor_closed(&window, &discarded);
    Ok(())
}

#[cfg(test)]
#[path = "project_discard_tests.rs"]
mod tests;
