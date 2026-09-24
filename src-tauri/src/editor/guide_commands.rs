//! The app-wide guided-onboarding progress (Tasks 55 and 57; F-46, F-47;
//! ADR R16, R18) — split out of `prefs_commands.rs` when Task 57 added the
//! portable progress file, because none of it is a project preference.
//!
//! **The stored progress.** `editor-prefs\guide-progress.json` under the
//! app's local data root, read and written by `editor_get_guide_progress`/
//! `editor_save_guide_progress` with no session at all. What the document
//! may hold is `core::editor::guide`'s to decide (a closed schema, the
//! lessons compiled in from the webview's own `steps.json`, 16 KiB); this
//! module only finds the file, reads it bounded and no-follow, and writes it
//! on the same `write_atomic_replacing` rails as `workspace.json`.
//!
//! **The progress FILE (Task 57).** The learning center's Save progress file
//! and Restore progress file: `editor_export_guide_progress` writes the
//! webview's CURRENT progress (which may be session-only, the case the file
//! exists for) to a file the user picks, and `editor_import_guide_progress`
//! reads one back. Both open Rust's OWN native dialog — a path never comes
//! from the webview — on the named `editor-guide-file` thread, joined from
//! the blocking pool (the subtitle/package posture). Both judge the
//! document with the SAME strict gate as a save (`guide::validate_for_save`,
//! and for a file `guide::parse_progress_file`: the raw 16 KiB bound first,
//! JSON, a closed schema, known lesson ids only), so a progress file can
//! only ever hold lesson ids, flags and two presentation preferences — no
//! media, no project detail, no path. The import only READS: it returns the
//! validated progress and writes nothing (the webview installs it, paused,
//! and saves it through `editor_save_guide_progress` like any other
//! change) — it never starts the guide, touches a device or a project.
//!
//! **Which existing file an export may replace.** A new name is written
//! with an owned same-directory temp and `rename_noreplace`. An existing
//! file is replaced only when it is a plain file that is ITSELF valid guide
//! progress AND exactly the path the save dialog confirmed (a name this
//! module normalized onto an existing file was never confirmed); anything
//! else is `writeDenied` and left byte-identical.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST — `authz_guard.rs` fails
//! naming any that does not.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::guide::{self, GuideProgress};
use vault_buddy_core::editor::{new_entity_id, EditorError, EditorErrorCode};

use super::authz::require_editor_window;
use super::prefs_commands::{blocking, local_data};

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

/// The app-wide preference folder under the app's local data root —
/// deliberately OUTSIDE `editor-projects\`: guide progress belongs to the
/// person, not to any project (R16).
pub(crate) const PREFS_DIR: &str = "editor-prefs";
pub(crate) const GUIDE_PROGRESS_FILE: &str = "guide-progress.json";

/// Reads at most one byte past the bound, never following a link: the file
/// and its folder must be plain. `Ok(None)` = nothing saved yet.
fn read_progress_bytes(root: &Path) -> std::io::Result<Option<Vec<u8>>> {
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

// ---- the portable progress file (Task 57) -----------------------------------

/// What the save dialog suggests.
pub(crate) const SUGGESTED_FILE_NAME: &str = "vault-buddy-guide-progress.json";

/// Where the file goes and which file to read — the native dialogs in
/// production, fixed answers in the tests. `None` is a dismissed dialog.
pub(crate) trait GuideFileChooser {
    fn save_target(&self, suggested: &str) -> Option<PathBuf>;
    fn file_to_open(&self) -> Option<PathBuf>;
}

/// `editor_export_guide_progress`' `AppHandle`-free half: the file name
/// written, or `None` for a dismissed dialog. The save gate runs BEFORE the
/// dialog opens, so a document it refuses never creates a file.
pub(crate) fn export_guide_progress_in(
    chooser: &dyn GuideFileChooser,
    progress: Value,
) -> Result<Option<String>, EditorError> {
    let progress = guide::validate_for_save(&progress)?;
    let json = serde_json::to_string_pretty(&progress)
        .map_err(|e| internal(format!("Could not encode guide progress: {e}")))?;
    let Some(chosen) = chooser.save_target(SUGGESTED_FILE_NAME) else {
        return Ok(None);
    };
    let target = with_json_extension(&chosen)?;
    let replace = may_replace(&target, &chosen)?;
    write_progress_file(&target, &json, replace)?;
    Ok(Some(file_name_of(&target)))
}

/// `editor_import_guide_progress`' `AppHandle`-free half: the validated
/// progress, or `None` for a dismissed dialog. Reads only — nothing is
/// stored, started or opened here.
pub(crate) fn import_guide_progress_in(
    chooser: &dyn GuideFileChooser,
) -> Result<Option<GuideProgress>, EditorError> {
    let Some(path) = chooser.file_to_open() else {
        return Ok(None);
    };
    let bytes = read_picked_file(&path)?;
    guide::parse_progress_file(&bytes).map(Some)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `chosen`, ending in `.json` (any case) — appended when it does not.
fn with_json_extension(chosen: &Path) -> Result<PathBuf, EditorError> {
    let name = file_name_of(chosen);
    if name.trim().is_empty() {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "Choose a file name to save to.",
        ));
    }
    let has = Path::new(&name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    Ok(if has {
        chosen.to_path_buf()
    } else {
        chosen.with_file_name(format!("{name}.json"))
    })
}

fn not_progress() -> EditorError {
    err(
        EditorErrorCode::InvalidRequest,
        "That file is not Vault Buddy guide progress.",
    )
}

/// A picked file, no-follow and bounded: a plain file (a link or a folder
/// wearing the name is refused), read at most one byte past 16 KiB so
/// `parse_progress_file`'s size bound can see an oversized one. Errors
/// never name the file (the caption import's privacy posture).
fn read_picked_file(path: &Path) -> Result<Vec<u8>, EditorError> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| {
        log::warn!(
            "editor guide file: cannot inspect the chosen file ({:?})",
            e.kind()
        );
        not_progress()
    })?;
    if !meta.is_file() {
        return Err(not_progress());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(guide::MAX_GUIDE_PROGRESS_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| {
            log::warn!(
                "editor guide file: cannot read the chosen file ({:?})",
                e.kind()
            );
            err(
                EditorErrorCode::InvalidRequest,
                "The chosen file could not be read.",
            )
        })?;
    Ok(bytes)
}

/// `false` for a new name; `true` when `target` is an existing plain file
/// that is itself guide progress AND the path the dialog confirmed;
/// `writeDenied` for anything else that is there.
fn may_replace(target: &Path, chosen: &Path) -> Result<bool, EditorError> {
    match std::fs::symlink_metadata(target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(map_prefs_write_error(e)),
        Ok(_) => {}
    }
    let ours = target == chosen
        && read_picked_file(target)
            .and_then(|bytes| guide::parse_progress_file(&bytes))
            .is_ok();
    if ours {
        return Ok(true);
    }
    Err(err(
        EditorErrorCode::WriteDenied,
        format!(
            "{:?} already exists. Choose a new name; only a guide progress file you pick in the dialog is replaced.",
            file_name_of(target)
        ),
    ))
}

/// An owned `.<name>.<id>.part` beside `target`, fsync'd, then landed:
/// `rename_noreplace` for a new name, a replacing rename only when
/// `may_replace` said so. The temp never outlives a failure.
fn write_progress_file(target: &Path, json: &str, replace: bool) -> Result<(), EditorError> {
    let temp = target.with_file_name(format!(
        ".{}.{}.part",
        file_name_of(target),
        new_entity_id("guide")
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(json.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| {
            if replace {
                std::fs::rename(&temp, target)
            } else {
                rename_noreplace(&temp, target)
            }
        });
    let Err(e) = written else {
        return Ok(());
    };
    if let Err(remove) = std::fs::remove_file(&temp) {
        if remove.kind() != std::io::ErrorKind::NotFound {
            log::warn!("editor guide file: could not remove a temporary file: {remove}");
        }
    }
    if e.kind() == std::io::ErrorKind::AlreadyExists {
        return Err(err(
            EditorErrorCode::WriteDenied,
            format!(
                "{:?} already exists. Choose a new name.",
                file_name_of(target)
            ),
        ));
    }
    Err(map_prefs_write_error(e))
}

/// The production chooser: the dialog plugin, parented to the editor
/// window. Blocking, so it only ever runs on `editor-guide-file`.
struct DialogChooser<'a> {
    app: &'a AppHandle,
    window: &'a WebviewWindow,
}

impl GuideFileChooser for DialogChooser<'_> {
    fn save_target(&self, suggested: &str) -> Option<PathBuf> {
        let picked = self
            .app
            .dialog()
            .file()
            .set_title("Save guide progress")
            .add_filter("Guide progress", &["json"])
            .set_file_name(suggested)
            .set_parent(self.window)
            .blocking_save_file()?;
        local_path(picked)
    }

    fn file_to_open(&self) -> Option<PathBuf> {
        let picked = self
            .app
            .dialog()
            .file()
            .set_title("Restore guide progress")
            .add_filter("Guide progress", &["json"])
            .set_parent(self.window)
            .blocking_pick_file()?;
        local_path(picked)
    }
}

fn local_path(picked: tauri_plugin_dialog::FilePath) -> Option<PathBuf> {
    match picked.into_path() {
        Ok(path) => Some(path),
        Err(_) => {
            log::warn!("editor guide file: the chosen file has no local path");
            None
        }
    }
}

/// Runs `work` with the native dialogs on the named `editor-guide-file`
/// thread, joined from the blocking pool.
async fn on_guide_file_thread<T: Send + 'static>(
    window: WebviewWindow,
    app: AppHandle,
    work: impl FnOnce(&dyn GuideFileChooser) -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError> {
    blocking(move || {
        let handle = std::thread::Builder::new()
            .name("editor-guide-file".into())
            .spawn(move || {
                work(&DialogChooser {
                    app: &app,
                    window: &window,
                })
            })
            .map_err(|e| {
                log::error!("editor guide file: could not start its thread: {e}");
                internal("The guide progress file could not be opened.")
            })?;
        handle.join().map_err(|_| {
            log::error!("editor guide file: its thread panicked");
            internal("The guide progress file stopped unexpectedly.")
        })?
    })
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

/// ASYNC (Task 57): Rust's own save dialog on `editor-guide-file`. Takes
/// the webview's CURRENT progress raw, so the save gate's refusal is this
/// module's `invalidRequest`; returns the file name written, or `null` for
/// a dismissed dialog.
#[tauri::command]
pub async fn editor_export_guide_progress(
    window: WebviewWindow,
    app: AppHandle,
    progress: Value,
) -> Result<Option<String>, EditorError> {
    require_editor_window(&window)?;
    on_guide_file_thread(window, app, move |chooser| {
        export_guide_progress_in(chooser, progress)
    })
    .await
}

/// ASYNC (Task 57): Rust's own open dialog on `editor-guide-file`; the
/// validated progress, or `null` for a dismissed dialog. Writes nothing.
#[tauri::command]
pub async fn editor_import_guide_progress(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Option<GuideProgress>, EditorError> {
    require_editor_window(&window)?;
    on_guide_file_thread(window, app, import_guide_progress_in).await
}

#[cfg(test)]
#[path = "guide_commands_tests.rs"]
mod tests;
