//! `editor_relink_media` (Task 40; F-03; A19; ADR §3.3): reconnect missing
//! originals. Its own module rather than a growth of `media_commands.rs`,
//! which sits near the 800-line cap (pre-flight ruling F31, the
//! `caption_commands.rs` precedent); the pipeline itself is
//! `relink_media.rs`, `AppHandle`-free so it is tested on a tempdir, and
//! the matching is pure in `core::editor::relink` (F32).
//!
//! **The flow:** the request is checked, then the session's one reconnect
//! slot is claimed (a second one while a dialog is open is refused — Rust,
//! not a disabled button, is the authority). On the named `editor-relink`
//! thread, joined from the blocking pool: every requested source is checked
//! BEFORE the dialog opens (`relink_media::targets`, so a present or
//! builtin source never asks the user to pick a file for nothing), then the
//! native open dialog — multi-select for a batch, single for one source,
//! filtered to the import's own allowlist — and then the pipeline. The
//! dialog, never a frontend string, grants the path. The reply is the
//! report, or `null` when the dialog was dismissed.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST — `authz_guard.rs` fails
//! naming any that does not.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::editor::probe::import_extensions;
use vault_buddy_core::editor::relink::RelinkReportDto;
use vault_buddy_core::editor::{EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::media_import::FfprobeImportIo;
use super::prefs_commands::{blocking, local_data};
use super::relink_media::{check_request, relink_in, targets, RelinkJob};
use super::EditorState;

fn err(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::Internal, message)
}

/// Claims the session's one reconnect slot (`caption_imports`' posture).
/// Paired with `release_relink` on every way out.
pub(crate) fn claim_relink(state: &EditorState, session_id: &str) -> Result<(), EditorError> {
    if !lock_ignoring_poison(&state.relinks).insert(session_id.to_string()) {
        return Err(EditorError::new(
            EditorErrorCode::InvalidRequest,
            "A reconnect is already running for this project.",
        ));
    }
    Ok(())
}

pub(crate) fn release_relink(state: &EditorState, session_id: &str) {
    lock_ignoring_poison(&state.relinks).remove(session_id);
}

/// The files the user picked — one for a single source, any number for a
/// batch; empty for a dismissed dialog. Blocking: the `editor-relink`
/// thread calls it, never the main thread.
fn pick_files(app: &AppHandle, window: &WebviewWindow, title: &str, one: bool) -> Vec<PathBuf> {
    let dialog = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter("Video, audio and images", &import_extensions())
        .set_parent(window);
    let picked = if one {
        dialog.blocking_pick_file().into_iter().collect()
    } else {
        dialog.blocking_pick_files().unwrap_or_default()
    };
    picked
        .into_iter()
        .filter_map(|p| match p.into_path() {
            Ok(path) => Some(path),
            Err(_) => {
                log::warn!("editor relink: a picked file has no local path");
                None
            }
        })
        .collect()
}

/// The dialog's title — the asset's display name when there is one source.
fn dialog_title(names: &[String], confirm_replace: bool) -> String {
    match (names, confirm_replace) {
        ([one], true) => format!("Choose a replacement for “{one}”"),
        ([one], false) => format!("Find “{one}”"),
        _ => "Find the missing media".to_string(),
    }
}

/// ASYNC (ADR §3.3): `sessionId, assetIds, confirmReplace` →
/// `RelinkReport | null`. Module doc for the flow.
#[tauri::command]
pub async fn editor_relink_media(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    asset_ids: Vec<String>,
    confirm_replace: bool,
) -> Result<Option<RelinkReportDto>, EditorError> {
    require_editor_window(&window)?;
    check_request(&asset_ids, confirm_replace)?;
    let root = local_data(&app)?;
    drop(require_session(&app.state::<EditorState>(), &session_id)?);
    claim_relink(&app.state::<EditorState>(), &session_id)?;
    let (release_app, release_id) = (app.clone(), session_id.clone());
    let result = blocking(move || {
        let worker = move || -> Result<Option<RelinkReportDto>, EditorError> {
            let state = app.state::<EditorState>();
            let io = FfprobeImportIo::default();
            let job = RelinkJob {
                state: &state,
                root: &root,
                session_id: &session_id,
                io: &io,
            };
            let (_, checked) = targets(&job, &asset_ids)?;
            let names: Vec<String> = checked.into_iter().map(|t| t.name).collect();
            let title = dialog_title(&names, confirm_replace);
            let files = pick_files(&app, &window, &title, asset_ids.len() == 1);
            if files.is_empty() {
                return Ok(None);
            }
            relink_in(&job, &asset_ids, confirm_replace, &files).map(Some)
        };
        let handle = std::thread::Builder::new()
            .name("editor-relink".into())
            .spawn(worker)
            .map_err(|e| {
                log::error!("editor relink: could not start the reconnect thread: {e}");
                err("The reconnect could not start.")
            })?;
        handle.join().map_err(|_| {
            log::error!("editor relink: the reconnect thread panicked");
            err("The reconnect stopped unexpectedly.")
        })?
    })
    .await;
    release_relink(&release_app.state::<EditorState>(), &release_id);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_reconnect_in_the_same_session_is_refused_until_released() {
        let state = EditorState::default();
        claim_relink(&state, "ses-1").unwrap();
        let e = claim_relink(&state, "ses-1").unwrap_err();
        assert_eq!(e.code, EditorErrorCode::InvalidRequest);
        claim_relink(&state, "ses-2").expect("another session is independent");
        release_relink(&state, "ses-1");
        claim_relink(&state, "ses-1").expect("released");
    }

    #[test]
    fn the_dialog_names_the_one_source_it_is_for() {
        let one = vec!["talk.mp4".to_string()];
        assert_eq!(dialog_title(&one, false), "Find “talk.mp4”");
        assert_eq!(
            dialog_title(&one, true),
            "Choose a replacement for “talk.mp4”"
        );
        let two = vec!["a".to_string(), "b".to_string()];
        assert_eq!(dialog_title(&two, false), "Find the missing media");
    }
}
