//! Subtitle export (Task 48; F-35; ADR §3.3): `editor_export_subtitles`
//! writes the timeline's captions to an SRT or WebVTT file the user picks.
//! Its own module beside `caption_commands` (the import), because
//! `media_commands.rs` is at its line cap.
//!
//! **Output time, the whole timeline.** The cues are
//! `render_plan::subtitle_cues` -- the burn-in's own rows, every caption
//! the Captions list shows mapped to where it PLAYS -- then
//! `captions_io::export_srt`/`export_vtt`. An export is its own, explicit
//! request, so it does not depend on whether captions are shown or burned
//! in.
//!
//! **A file the user owns, never a store or vault write.** It lands as an
//! owned same-directory temp renamed into place with `rename_noreplace`: a
//! first write never replaces, so an existing file at the chosen name is
//! refused and left byte-identical (the package export's rule). Nothing in
//! the project or the session changes.
//!
//! The `#[tauri::command]` takes `window` and calls `require_editor_window`
//! first (R8, `authz_guard.rs`); the native dialog runs on the named
//! `editor-subtitles` thread, joined from the blocking pool.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::captions_io::{export_srt, export_vtt};
use vault_buddy_core::editor::render_plan::subtitle_cues;
use vault_buddy_core::editor::{new_entity_id, EditorError, EditorErrorCode, Project};
use vault_buddy_screen::staging_title::sanitize_title;

use super::authz::{require_editor_window, require_session};
use super::prefs_commands::blocking;
use super::save_commands::map_write_error;
use super::EditorState;

/// `editor_export_subtitles`' `format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleFormat {
    Srt,
    Vtt,
}

impl SubtitleFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
        }
    }
}

/// Where the file goes -- the native save dialog in production, a fixed
/// answer in the tests. `None` is a dismissed dialog.
pub(crate) trait SubtitleTarget {
    fn save_target(&self, format: SubtitleFormat, suggested: &str) -> Option<PathBuf>;
}

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

/// The session's project right now, cloned (the export reads what is on
/// screen; an edit landing while the dialog is open changes nothing).
fn live_project(state: &EditorState, session_id: &str) -> Result<Project, EditorError> {
    let sessions = require_session(state, session_id)?;
    sessions
        .get(session_id)
        .map(|s| s.project().clone())
        .ok_or_else(|| {
            err(
                EditorErrorCode::Internal,
                "session vanished under its own lock",
            )
        })
}

/// `chosen` with the format's extension unless it already ends in it (any
/// case -- `captions.VTT` stays as the user typed it).
fn with_extension(chosen: &Path, format: SubtitleFormat) -> Result<PathBuf, EditorError> {
    let name = chosen
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| {
            err(
                EditorErrorCode::InvalidRequest,
                "Choose a file name to save to.",
            )
        })?;
    let extension = format.extension();
    let has = Path::new(&name)
        .extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension));
    Ok(if has {
        chosen.to_path_buf()
    } else {
        chosen.with_file_name(format!("{name}.{extension}"))
    })
}

/// Write `text` to an owned `.<name>.<id>.part` beside `target`, fsync it,
/// and land it with `rename_noreplace`; the temp never outlives a failure.
/// Shared with the diagnostics export (Task 58), the other new file a user
/// picks the name of.
pub(crate) fn write_new_file(target: &Path, text: &str) -> Result<(), EditorError> {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(".{name}.{}.part", new_entity_id("sub")));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| rename_noreplace(&temp, target));
    let Err(e) = written else {
        return Ok(());
    };
    if let Err(remove) = std::fs::remove_file(&temp) {
        if remove.kind() != std::io::ErrorKind::NotFound {
            log::warn!("editor export: could not remove a temporary file: {remove}");
        }
    }
    if e.kind() == std::io::ErrorKind::AlreadyExists {
        return Err(err(
            EditorErrorCode::WriteDenied,
            format!(
                "{name:?} already exists. Choose a new name; an existing file is never replaced."
            ),
        ));
    }
    Err(map_write_error(e))
}

/// The `AppHandle`-free half: the file name written, or `None` for a
/// dismissed dialog. "Nothing to export" is said before the dialog opens.
pub(crate) fn export_subtitles_in(
    state: &EditorState,
    chooser: &dyn SubtitleTarget,
    session_id: &str,
    format: SubtitleFormat,
) -> Result<Option<String>, EditorError> {
    let project = live_project(state, session_id)?;
    let cues = subtitle_cues(&project);
    if cues.is_empty() {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "There are no captions on the timeline to export yet.",
        ));
    }
    let text = match format {
        SubtitleFormat::Srt => export_srt(&cues),
        SubtitleFormat::Vtt => export_vtt(&cues),
    };
    let suggested = format!("{}.{}", sanitize_title(&project.title), format.extension());
    let Some(chosen) = chooser.save_target(format, &suggested) else {
        return Ok(None);
    };
    let target = with_extension(&chosen, format)?;
    write_new_file(&target, &text)?;
    Ok(target.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// The production `SubtitleTarget`: the dialog plugin, parented to the
/// editor window. Blocking, so it only ever runs on `editor-subtitles`.
struct DialogTarget<'a> {
    app: &'a AppHandle,
    window: &'a WebviewWindow,
}

impl SubtitleTarget for DialogTarget<'_> {
    fn save_target(&self, format: SubtitleFormat, suggested: &str) -> Option<PathBuf> {
        let (title, filter) = match format {
            SubtitleFormat::Srt => ("Export subtitles (SRT)", "SubRip subtitles"),
            SubtitleFormat::Vtt => ("Export subtitles (WebVTT)", "WebVTT subtitles"),
        };
        let picked = self
            .app
            .dialog()
            .file()
            .set_title(title)
            .add_filter(filter, &[format.extension()])
            .set_file_name(suggested)
            .set_parent(self.window)
            .blocking_save_file()?;
        match picked.into_path() {
            Ok(path) => Some(path),
            Err(_) => {
                log::warn!("editor subtitles: the chosen file has no local path");
                None
            }
        }
    }
}

/// ASYNC (ADR §3.3): `null` when the save dialog was dismissed.
#[tauri::command]
pub async fn editor_export_subtitles(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    format: SubtitleFormat,
) -> Result<Option<String>, EditorError> {
    require_editor_window(&window)?;
    blocking(move || {
        let handle = std::thread::Builder::new()
            .name("editor-subtitles".into())
            .spawn(move || {
                let target = DialogTarget {
                    app: &app,
                    window: &window,
                };
                export_subtitles_in(&app.state::<EditorState>(), &target, &session_id, format)
            })
            .map_err(|e| {
                log::error!("editor subtitles: could not start the export thread: {e}");
                err(
                    EditorErrorCode::Internal,
                    "The subtitles could not be exported.",
                )
            })?;
        handle.join().map_err(|_| {
            log::error!("editor subtitles: the export thread panicked");
            err(
                EditorErrorCode::Internal,
                "Exporting the subtitles stopped unexpectedly.",
            )
        })?
    })
    .await
}

#[cfg(test)]
#[path = "subtitle_commands_tests.rs"]
mod tests;
