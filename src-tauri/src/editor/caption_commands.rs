//! `editor_import_captions` (Task 36; F-34; ADR §3.3): subtitle import onto
//! one clip. Its own module rather than a growth of `media_commands.rs`,
//! which sits at the 800-line cap (pre-flight ruling F31); the seam is the
//! same one Task 25 drew for media import -- a native dialog on a named
//! thread, the dialog (never a frontend string) granting access to a path.
//!
//! **The flow, and where each rule lives:** the `editor-captions` thread
//! opens the native open dialog (`srt`, `vtt`, `txt`); the picked file is
//! read BOUNDED (`read_caption_file`: at most `MAX_CAPTION_FILE_BYTES` + 1
//! bytes are ever read, UTF-8 or refused); `core::editor::captions_io`
//! parses it (every error names its line); `commands::captions::
//! plan_caption_import` maps the file's clip-relative OUTPUT times to the
//! clip's SOURCE time and counts what fell outside; then ONE
//! `InternalCommand::ImportCaptions` edit installs it, so Undo removes the
//! whole import. The reply is `CaptionImportResult { projection, imported,
//! skipped }`, or `null` when the dialog was cancelled.
//!
//! **Nothing about the file leaves in an error or a log line** (bundle
//! § Privacy: logs omit captions, file names and paths): a failed open or
//! read logs only the `io::ErrorKind`, and every user-facing message is
//! fixed text or the parser's own line-numbered complaint.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST — `authz_guard.rs` fails
//! naming any that does not.

use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::editor::captions_io::parse_subtitles;
use vault_buddy_core::editor::commands::captions::plan_caption_import;
use vault_buddy_core::editor::commands::payloads::ImportCaptionsPayload;
use vault_buddy_core::editor::{
    limits, CaptionImportResult, EditorError, EditorErrorCode, EditorProjection, InternalCommand,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::prefs_commands::blocking;
use super::EditorState;

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

/// The picked file's text: at most `MAX_CAPTION_FILE_BYTES` read (one byte
/// more is read only to tell "exactly at the limit" from "over it"), and
/// UTF-8 or refused -- a Latin-1 file decoded lossily would import
/// replacement characters the user never wrote.
pub(crate) fn read_caption_file(path: &Path) -> Result<String, EditorError> {
    let file = std::fs::File::open(path).map_err(|e| {
        log::warn!(
            "editor captions: the picked file could not be opened ({:?})",
            e.kind()
        );
        err(
            EditorErrorCode::InvalidRequest,
            "The subtitle file could not be opened.",
        )
    })?;
    let mut bytes = Vec::new();
    file.take(limits::MAX_CAPTION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            log::warn!(
                "editor captions: the picked file could not be read ({:?})",
                e.kind()
            );
            err(
                EditorErrorCode::InvalidRequest,
                "The subtitle file could not be read.",
            )
        })?;
    if bytes.len() as u64 > limits::MAX_CAPTION_FILE_BYTES {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "Subtitle files are limited to 2 MiB.",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        err(
            EditorErrorCode::InvalidRequest,
            "The subtitle file is not UTF-8 text. Save it as UTF-8 and import it again.",
        )
    })
}

/// Parse, plan and apply against the live session -- the `AppHandle`-free
/// half. The parse runs before the session lock is taken (it is the only
/// part that can take a while on a 2 MiB file); the plan and the edit run
/// under it, against the project as it stands at that moment, so an edit
/// made while the dialog was open is never lost or raced.
pub(crate) fn import_captions_in(
    state: &EditorState,
    session_id: &str,
    clip_id: &str,
    replace: bool,
    text: &str,
) -> Result<CaptionImportResult, EditorError> {
    let parsed =
        parse_subtitles(text).map_err(|e| err(EditorErrorCode::InvalidRequest, e.to_string()))?;
    let mut sessions = require_session(state, session_id)?;
    let session = sessions.get_mut(session_id).ok_or_else(|| {
        err(
            EditorErrorCode::Internal,
            "session vanished under its own lock",
        )
    })?;
    let plan = plan_caption_import(session.project(), clip_id, &parsed)?;
    let imported = plan.cues.len();
    session.execute_internal(&InternalCommand::ImportCaptions(ImportCaptionsPayload {
        clip_id: clip_id.to_string(),
        cues: plan.cues,
        replace,
    }))?;
    Ok(CaptionImportResult {
        projection: EditorProjection::of(session),
        imported,
        skipped: plan.skipped,
    })
}

/// Claims the session's one caption-import slot, refusing a second import
/// while one is still running there (the `editor_import_media` posture:
/// Rust, not the UI's disabled button, is the authority). Paired with
/// `release_caption_import`, which `editor_import_captions` calls on every
/// way out.
pub(crate) fn claim_caption_import(
    state: &EditorState,
    session_id: &str,
) -> Result<(), EditorError> {
    if !lock_ignoring_poison(&state.caption_imports).insert(session_id.to_string()) {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "A caption import is already running for this project.",
        ));
    }
    // Final review C2: claimed first, checked second (`discard.rs`).
    super::discard::refuse_if_closing(state, session_id)
        .inspect_err(|_| release_caption_import(state, session_id))
}

pub(crate) fn release_caption_import(state: &EditorState, session_id: &str) {
    lock_ignoring_poison(&state.caption_imports).remove(session_id);
}

/// The one file the user picked, or `None` for a dismissed dialog.
/// Blocking, so it runs on the `editor-captions` thread, never the main
/// thread.
fn pick_caption_file(app: &AppHandle, window: &WebviewWindow) -> Option<PathBuf> {
    let picked = app
        .dialog()
        .file()
        .set_title("Import captions")
        .add_filter("Subtitles (SRT, WebVTT)", &["srt", "vtt", "txt"])
        .set_parent(window)
        .blocking_pick_file()?;
    match picked.into_path() {
        Ok(path) => Some(path),
        Err(_) => {
            log::warn!("editor captions: the picked file has no local path");
            None
        }
    }
}

/// ASYNC (ADR §3.3). The session is checked BEFORE the dialog opens, so a
/// closed session never asks the user to pick a file for nothing, and a
/// second import while one runs is refused (`claim_caption_import`); the
/// dialog, the read and the edit then run on the named `editor-captions`
/// thread, joined from the blocking pool.
#[tauri::command]
pub async fn editor_import_captions(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    clip_id: String,
    replace: bool,
) -> Result<Option<CaptionImportResult>, EditorError> {
    require_editor_window(&window)?;
    drop(require_session(&app.state::<EditorState>(), &session_id)?);
    claim_caption_import(&app.state::<EditorState>(), &session_id)?;
    let (release_app, release_id) = (app.clone(), session_id.clone());
    let result = blocking(move || {
        let worker = move || -> Result<Option<CaptionImportResult>, EditorError> {
            let Some(path) = pick_caption_file(&app, &window) else {
                return Ok(None);
            };
            let text = read_caption_file(&path)?;
            let state = app.state::<EditorState>();
            let imported = import_captions_in(&state, &session_id, &clip_id, replace, &text)?;
            // Task 37: an acknowledged edit like any `editor_execute`.
            match app.path().app_local_data_dir() {
                Ok(root) => super::recovery::note_acknowledged(&state, &root, &session_id),
                Err(e) => log::warn!("editor captions: no data directory to journal into: {e}"),
            }
            Ok(Some(imported))
        };
        let handle = std::thread::Builder::new()
            .name("editor-captions".into())
            .spawn(worker)
            .map_err(|e| {
                log::error!("editor captions: could not start the import thread: {e}");
                err(
                    EditorErrorCode::Internal,
                    "The caption import could not start.",
                )
            })?;
        handle.join().map_err(|_| {
            log::error!("editor captions: the import thread panicked");
            err(
                EditorErrorCode::Internal,
                "The caption import stopped unexpectedly.",
            )
        })?
    })
    .await;
    release_caption_import(&release_app.state::<EditorState>(), &release_id);
    result
}

#[cfg(test)]
#[path = "caption_commands_tests.rs"]
mod tests;
