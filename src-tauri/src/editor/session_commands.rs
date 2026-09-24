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
//! **Lock order**: `open` (outermost, held across an open's disk I/O by
//! design), then `by_project`, then `sessions`. The two maps are never held
//! across disk I/O: every function here does its disk work first and takes
//! them only for the in-memory register/apply/remove.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::commands::CommandContext;
use vault_buddy_core::editor::{
    migrate, new_entity_id, new_project_id, sanitize, validate_project, EditorCommand, EditorError,
    EditorErrorCode, EditorOpenResult, EditorProjection, EditorSession, ExecuteRequest,
    MissingMedia, Project, WorkspaceEnvelope,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use super::authz::{require_editor_window, require_session};
use super::prefs_commands::project_id_for;
use super::project_store::{
    pin_staged, pinned_project, project_dir, resolve_source, unpin_staged, SourceLocator,
    SourceMediaKind, SourceRecord,
};
use super::recovery;
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
            let opened = load_opened(root, &pid)?;
            return ensure_sources_name(opened, base, &pid);
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
    let webcam = staged_webcam(staging_dir, &sidecar);
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
            webcam: webcam.as_ref().map(|(input, _)| input.clone()),
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
            replaced_from: None,
        },
    );
    if let Some((_, record)) = webcam {
        sources.insert(migrate::WEBCAM_ASSET_ID.to_string(), record);
    }
    create_project(root, &migration.project, &sources)
        .map_err(|e| internal(format!("Could not create the project: {e}")))?;
    pin(staging_dir, base, &project_id)?;
    load_opened(root, &project_id)
}

/// The capture's synchronized webcam track (F-22, F26) as migration input,
/// with the `StagingFile` source record that makes its file resolvable —
/// or `None`: no `webcam` block, a block naming any file but this
/// capture's own `<base>.webcam.mp4` (the sidecar is hand-editable, and a
/// record that could never resolve is worse than no webcam), or no length.
///
/// Its length is the capture's from `offset_ms` on: both streams stamp from
/// the one `CaptureClock` and stop on the one Stop (ADR §4), and nothing on
/// disk records the webcam's own length without a probe.
fn staged_webcam(
    staging_dir: &Path,
    sidecar: &staging::StagedSidecar,
) -> Option<(migrate::WebcamInput, SourceRecord)> {
    let webcam = sidecar.webcam.as_ref()?;
    if webcam.file != staging::webcam_file_name(&sidecar.base) {
        log::warn!(
            "editor_open_staged: {:?}'s webcam block names {:?}, not its own webcam file; ignored",
            sidecar.base,
            webcam.file
        );
        return None;
    }
    let length = i128::from(sidecar.duration_ms) - i128::from(webcam.offset_ms);
    let Some(duration_ms) = u64::try_from(length).ok().filter(|ms| *ms > 0) else {
        log::warn!(
            "editor_open_staged: {:?}'s webcam starts at {} ms, not before the capture ends at {} ms; ignored",
            sidecar.base,
            webcam.offset_ms,
            sidecar.duration_ms
        );
        return None;
    };
    // An unreadable size is recorded as 0 ("unknown", GAP-182's posture) —
    // the record still resolves, and `missing_media` reports a file that is
    // really gone — but never silently.
    let size = match std::fs::metadata(staging_dir.join(&webcam.file)) {
        Ok(meta) => meta.len(),
        Err(e) => {
            log::warn!(
                "editor_open_staged: cannot read {:?}'s webcam file {:?}: {e}",
                sidecar.base,
                webcam.file
            );
            0
        }
    };
    let input = migrate::WebcamInput {
        duration_ms,
        width: webcam.width,
        height: webcam.height,
        file: webcam.file.clone(),
        offset_ms: webcam.offset_ms,
    };
    let record = SourceRecord {
        locator: SourceLocator::StagingFile {
            base: sidecar.base.clone(),
            file: webcam.file.clone(),
        },
        sha256: None,
        size,
        duration_ms,
        width: Some(webcam.width),
        height: Some(webcam.height),
        // The webcam sink is video-only (ADR §4); the capture's sound lives
        // on the screen track.
        has_audio: false,
        has_video: true,
        media_kind: SourceMediaKind::Video,
        replaced_from: None,
    };
    Some((input, record))
}

/// The pin is a claim made by a hand-editable sidecar: refuse a project
/// whose own `sources.json` names a DIFFERENT staged capture (or none), so
/// opening one capture can never open — and later edit or discard — another
/// capture's project.
fn ensure_sources_name(
    opened: OpenedProject,
    base: &str,
    project_id: &str,
) -> Result<OpenedProject, EditorError> {
    let staged: Vec<&str> = opened
        .sources
        .values()
        .filter_map(|r| match &r.locator {
            SourceLocator::Staging { base } => Some(base.as_str()),
            _ => None,
        })
        .collect();
    if staged.contains(&base) {
        return Ok(opened);
    }
    Err(err(
        EditorErrorCode::InvalidProject,
        format!(
            "The capture {base:?} is linked to project {project_id:?}, but that project edits {:?}. It was not opened.",
            staged.join(", ")
        ),
    ))
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
/// saves would race. A freshly minted session RESUMES at `revision` — the
/// on-disk envelope's own `record.revision` the caller just read (Task 12
/// fix round 1, controller ruling): `record.revision` is monotonic across a
/// project's WHOLE life, not just one session's, so reopening a project
/// saved at revision 3 must resume its session AT 3, never reset to 1. A
/// freshly MINTED project's own envelope (via `create_project`) also
/// carries `record.revision: 1`, so callers pass that same on-disk value
/// uniformly for both cases — there is no separate "fresh" path here.
/// `persisted_revision` starts at `Some(revision)`: what is in memory is
/// exactly what was just read off disk.
pub(crate) fn register_session(
    state: &EditorState,
    project: Project,
    revision: u64,
) -> EditorProjection {
    register_session_with(state, project, |id, project| {
        EditorSession::resume(id, project, revision)
    })
    .0
}

/// `register_session`'s find-or-mint, with the session a fresh mint starts
/// as supplied by `make` (Task 37: a recovered open starts DIRTY). The
/// boolean is whether `make` ran — `false` means a live session was reused.
pub(crate) fn register_session_with(
    state: &EditorState,
    project: Project,
    make: impl FnOnce(String, Project) -> EditorSession,
) -> (EditorProjection, bool) {
    let mut by_project = lock_ignoring_poison(&state.by_project);
    let mut sessions = lock_ignoring_poison(&state.sessions);
    if let Some(live) = by_project
        .get(&project.id)
        .and_then(|sid| sessions.get(sid))
    {
        return (EditorProjection::of(live), false);
    }
    let session_id = new_entity_id("ses");
    let project_id = project.id.clone();
    let session = make(session_id.clone(), project);
    let projection = EditorProjection::of(&session);
    sessions.insert(session_id.clone(), session);
    by_project.insert(project_id, session_id);
    (projection, true)
}

/// `open_staged_in` + `register_session` + the open result.
pub(crate) fn open_staged_session(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    base: &str,
) -> Result<EditorOpenResult, EditorError> {
    // Held until the session is registered: the open lock (outermost; see
    // `EditorState`) serializes find-or-mint + pin + register.
    let _open = lock_ignoring_poison(&state.open);
    let opened = open_staged_in(root, staging_dir, base)?;
    let workspace = sanitize(&opened.envelope.workspace);
    let revision = opened.envelope.record.revision;
    let projection = register_session(state, opened.envelope.project, revision);
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

/// The ids of every asset whose `sources.json` record has an audio stream
/// — `CommandContext::assets_with_audio` (Task 27, F15), the ONE place a
/// shell fact crosses into a core call. A staged capture's record says
/// what its sidecar recorded (`hasAudio` = it had audio inputs), an
/// imported file's what ffprobe found, so a detach agrees with the media
/// either way.
///
/// Read only for a command that consults it (today `detachAudio`): every
/// other edit keeps working when `sources.json` is unreadable, and a
/// detach against an unreadable one is refused with the read's own error
/// rather than a misleading "no audio".
fn assets_with_audio(
    root: &Path,
    project_id: &str,
    command: &EditorCommand,
) -> Result<BTreeSet<String>, EditorError> {
    if !matches!(command, EditorCommand::DetachAudio(_)) {
        return Ok(BTreeSet::new());
    }
    Ok(load_sources(root, project_id)?
        .into_iter()
        .filter(|(_, record)| record.has_audio)
        .map(|(id, _)| id)
        .collect())
}

/// The apply behind `editor_execute`: `sources.json` is read (when the
/// command needs it) BEFORE the session mutex is taken, which is then held
/// for the apply and the projection clone, nothing else.
pub(crate) fn execute_in(
    state: &EditorState,
    root: &Path,
    request: &ExecuteRequest,
) -> Result<EditorProjection, EditorError> {
    let project_id = project_id_for(state, &request.session_id)?;
    let with_audio = assets_with_audio(root, &project_id, &request.command)?;
    let ctx = CommandContext {
        assets_with_audio: &with_audio,
    };
    let mut sessions = require_session(state, &request.session_id)?;
    let session = sessions
        .get_mut(&request.session_id)
        .ok_or_else(|| internal("session vanished under its own lock"))?;
    session.execute(request, &ctx)?;
    let projection = EditorProjection::of(session);
    drop(sessions);
    // Task 37: every acknowledged command schedules the recovery journal.
    recovery::note_acknowledged(state, root, &request.session_id);
    Ok(projection)
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
    // Prune the per-session save lock (`EditorState::save_locks`'s own
    // doc) along with the session it belongs to, so the map only grows
    // with sessions currently open rather than every session ever opened
    // in this process's life.
    lock_ignoring_poison(&state.save_locks).remove(session_id);
    // Whatever journal write was still pending has either been flushed
    // (`keep`) or must never happen (`discard*`).
    state.journal.forget(session_id);
    // A running import (Task 25) of a closing session stops before its next
    // file; its results would have no session to land in. Its peaks decodes
    // are jobs too; its thumbnail renders are not (Task 28).
    // Its finished records go too (GAP-174): nothing can ask for them once
    // the session is gone; a render still running keeps its record until
    // its terminal lands (Task 46).
    let mut jobs = lock_ignoring_poison(&state.jobs);
    jobs.cancel_session(session_id);
    jobs.forget_terminal(session_id);
    drop(jobs);
    super::media_derive::cancel_session_thumbnails(state, session_id);
    // Its unfinished webcam takes can never be finished now (Task 49): their
    // `.part` files go; a finished take's `.webm` stays with its asset.
    state.takes.forget_session(session_id);
}

/// Close a session. `discardProject` UNPINS the staged capture first and
/// only then removes the project directory: a failure between the two
/// leaves an unpinned orphan the next open adopts back, whereas the
/// reverse order would leave a pin naming a deleted project, and a pinned
/// capture refuses Discard. The recording itself is never touched (R6).
/// On any failure the session stays open so the user can retry.
///
/// **Every disposition holds the per-session SAVE lock** (Task 12 fix round
/// 2 for `discardProject`; Task 37 for `keep` and `discardRecovery`) — the
/// lock `editor_save_project` holds for its whole read-through-write
/// sequence and every recovery-journal write runs under. Without it a
/// discard could remove the directory under an in-flight save, a `keep`
/// could flush a journal beside a save that is deleting it, and a
/// `discardRecovery` could delete a journal a racing write then recreates.
/// A concurrent close waits behind an in-flight save, never the reverse.
///
/// `drop_session` runs INSIDE that lock (`close_locked`), so a save queued
/// behind this close finds the session gone (`sessionGone`) rather than
/// finding it still registered and failing against a removed directory
/// with a generic write error (Task 12 review, carried to Task 37).
pub(crate) fn close_in(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    session_id: &str,
    disposition: CloseDisposition,
) -> Result<(), EditorError> {
    let project_id = project_id_for(state, session_id)?;
    if disposition == CloseDisposition::DiscardProject {
        // Task 28 fix round 1: derived media (an ffmpeg holding a file of
        // this project open, a late cache write) must be stopped BEFORE
        // the save lock is taken — their final write needs that lock.
        super::media_derive::stop_session_derivations(state, session_id);
    }
    let session_lock = super::save_commands::session_save_lock(state, session_id)?;
    let _save_guard = lock_ignoring_poison(&session_lock);
    close_locked(
        state,
        root,
        staging_dir,
        session_id,
        &project_id,
        disposition,
    )
}

/// The part of `close_in` that runs under the session's save lock — ending
/// with `drop_session`, so nothing queued on that lock ever sees a session
/// whose project this close has already changed underneath it.
pub(crate) fn close_locked(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    session_id: &str,
    project_id: &str,
    disposition: CloseDisposition,
) -> Result<(), EditorError> {
    match disposition {
        // Task 37: the pending journal write lands before the session goes.
        CloseDisposition::Keep => recovery::flush_locked(state, session_id),
        // Task 37: only `recovery.json` goes (owned-file check); the saved
        // project, its sources and the pin are untouched.
        CloseDisposition::DiscardRecovery => {
            state.journal.forget(session_id);
            recovery::remove_journal(root, project_id)
                .map_err(|e| internal(format!("Could not discard the unsaved changes: {e}")))?;
        }
        CloseDisposition::DiscardProject => {
            let sources = load_sources(root, project_id)?;
            for record in sources.values() {
                if let SourceLocator::Staging { base } = &record.locator {
                    unpin_staged(staging_dir, base, project_id).map_err(|e| {
                        internal(format!("Could not unlink the capture {base:?}: {e}"))
                    })?;
                }
            }
            remove_project(root, project_id)?;
        }
    }
    // Task 47: a Review render is disposable -- the session's last one goes
    // with the session (a discard already removed the whole directory).
    // Under the save lock, so a review landing concurrently either lands
    // before this sweep or finds its job cancelled by `drop_session`.
    if disposition != CloseDisposition::DiscardProject {
        super::render_review::sweep_reviews(root, project_id, None);
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
    let root = local_data(&app)?;
    blocking(move || execute_in(&app.state::<EditorState>(), &root, &request)).await
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
