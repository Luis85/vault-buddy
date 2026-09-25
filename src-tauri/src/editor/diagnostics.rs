//! Export diagnostics (Task 58; F-50; PRODUCT-SPEC NFR "support without
//! content"): `editor_export_diagnostics` writes a small JSON file a user
//! can attach to a bug report, through Rust's OWN native save dialog.
//!
//! **Counts, capabilities and codes — nothing else.** The document is
//! `Diagnostics`, below: the app version, the OS and architecture, whether
//! an ffmpeg was found (its major.minor version and which of the render's
//! feature filters it has), how many editing sessions are open, how many
//! projects and products are on disk, every job's kind, phase and error
//! CODE, and the WebView2 version. It is built from typed fields only, so
//! no project title, file or asset name, path, caption, cue text, frame or
//! sample can reach it: there is no field to carry one. An error's MESSAGE
//! can name a file (`media_import`'s per-file errors do), which is why only
//! its code is copied. The ffmpeg banner line is left out for the same
//! reason (a custom build's configure line can carry a build path).
//!
//! **A new file the user names, never a replacement.** The subtitle
//! export's writer (`subtitle_commands::write_new_file`): an owned
//! same-directory temp, fsync'd, landed with `rename_noreplace`, so an
//! existing file at the chosen name is refused `writeDenied` and left
//! byte-identical. The dialog runs on the named `editor-diagnostics` thread,
//! joined from the blocking pool; the command takes `window` and calls
//! `require_editor_window` first (R8, `authz_guard.rs`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::editor::{EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::render::run::FEATURE_FILTERS;

use super::authz::require_editor_window;
use super::media_jobs::{JobKind, JobPhase};
use super::prefs_commands::{blocking, local_data};
use super::render_jobs::read_ledger;
use super::store_io::list_projects;
use super::subtitle_commands::write_new_file;
use super::EditorState;

use crate::ffmpeg::{probe_capabilities, resolve_working_ffmpeg};

/// `Diagnostics.ffmpeg`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegDiagnostics {
    pub found: bool,
    /// `"<major>.<minor>"`, or `null` when none was found.
    pub version: Option<String>,
    /// The render's optional filters this build has
    /// (`screen::render::run::FEATURE_FILTERS`, the one list).
    pub filters: Vec<String>,
}

/// One row of `Diagnostics.jobs`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDiagnostics {
    pub kind: JobKind,
    pub phase: JobPhase,
    pub error_code: Option<EditorErrorCode>,
}

/// The exported document (see the module doc for what it may never hold).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub app_version: String,
    pub os: String,
    pub ffmpeg: FfmpegDiagnostics,
    pub sessions: usize,
    pub projects: usize,
    pub products: usize,
    pub jobs: Vec<JobDiagnostics>,
    pub webview2_version: Option<String>,
}

/// What the process knows about its environment, gathered by the command
/// (the tests hand in their own).
pub(crate) struct Environment {
    pub app_version: String,
    pub os: String,
    pub ffmpeg: FfmpegDiagnostics,
    pub webview2_version: Option<String>,
}

/// The document for `state` and the project store under `root`.
pub(crate) fn collect_in(state: &EditorState, root: &Path, env: Environment) -> Diagnostics {
    let sessions = lock_ignoring_poison(&state.sessions).len();
    let projects = list_projects(root);
    let products = projects
        .iter()
        .map(|p| match read_ledger(root, &p.project_file_id) {
            Ok(ledger) => ledger.len(),
            Err(e) => {
                log::warn!(
                    "editor diagnostics: a product ledger was unreadable ({:?})",
                    e.code
                );
                0
            }
        })
        .sum();
    let jobs = lock_ignoring_poison(&state.jobs)
        .summaries()
        .into_iter()
        .map(|(kind, phase, error_code)| JobDiagnostics {
            kind,
            phase,
            error_code,
        })
        .collect();
    Diagnostics {
        app_version: env.app_version,
        os: env.os,
        ffmpeg: env.ffmpeg,
        sessions,
        projects: projects.len(),
        products,
        jobs,
        webview2_version: env.webview2_version,
    }
}

/// The resolved ffmpeg's version and feature filters (spawns ffmpeg: only
/// ever on the `editor-diagnostics` thread).
fn ffmpeg_diagnostics() -> FfmpegDiagnostics {
    let Some(tools) = resolve_working_ffmpeg() else {
        return FfmpegDiagnostics::default();
    };
    let caps = probe_capabilities(&tools);
    FfmpegDiagnostics {
        found: true,
        version: Some(format!("{}.{}", tools.version.0, tools.version.1)),
        filters: FEATURE_FILTERS
            .iter()
            .filter(|f| caps.has_filter(f))
            .map(|f| f.to_string())
            .collect(),
    }
}

/// Where the file goes -- the native save dialog in production, a fixed
/// answer in the tests. `None` is a dismissed dialog.
pub(crate) trait DiagnosticsTarget {
    fn save_target(&self, suggested: &str) -> Option<PathBuf>;
}

const SUGGESTED: &str = "vault-buddy-diagnostics.json";

/// `diagnostics` written to the file the user picks (`.json` appended
/// unless the name has it): the file name written, or `None`.
pub(crate) fn export_in(
    target: &dyn DiagnosticsTarget,
    diagnostics: &Diagnostics,
) -> Result<Option<String>, EditorError> {
    let Some(chosen) = target.save_target(SUGGESTED) else {
        return Ok(None);
    };
    let name = chosen
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| {
            EditorError::new(
                EditorErrorCode::InvalidRequest,
                "Choose a file name to save to.",
            )
        })?;
    let has_json = Path::new(&name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let path = if has_json {
        chosen
    } else {
        chosen.with_file_name(format!("{name}.json"))
    };
    let text = serde_json::to_string_pretty(diagnostics).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("The diagnostics could not be written: {e}"),
        )
    })?;
    write_new_file(&path, &text)?;
    Ok(path.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// The production `DiagnosticsTarget`: the dialog plugin, parented to the
/// editor window. Blocking, so it only ever runs on `editor-diagnostics`.
struct DialogTarget<'a> {
    app: &'a AppHandle,
    window: &'a WebviewWindow,
}

impl DiagnosticsTarget for DialogTarget<'_> {
    fn save_target(&self, suggested: &str) -> Option<PathBuf> {
        let picked = self
            .app
            .dialog()
            .file()
            .set_title("Export diagnostics")
            .add_filter("Diagnostics", &["json"])
            .set_file_name(suggested)
            .set_parent(self.window)
            .blocking_save_file()?;
        match picked.into_path() {
            Ok(path) => Some(path),
            Err(_) => {
                log::warn!("editor diagnostics: the chosen file has no local path");
                None
            }
        }
    }
}

fn thread_error(what: &str) -> EditorError {
    EditorError::new(
        EditorErrorCode::Internal,
        format!("The diagnostics export {what}."),
    )
}

/// ASYNC (ADR §3.3): the file name written, or `null` for a dismissed
/// dialog. Reading the WebView2 version asks the installed runtime
/// (`GetAvailableCoreWebView2BrowserVersionString`); a failure is `null`.
#[tauri::command]
pub async fn editor_export_diagnostics(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Option<String>, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    let app_version = app.package_info().version.to_string();
    blocking(move || {
        let handle = std::thread::Builder::new()
            .name("editor-diagnostics".into())
            .spawn(move || {
                let env = Environment {
                    app_version,
                    os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
                    ffmpeg: ffmpeg_diagnostics(),
                    webview2_version: tauri::webview_version().ok(),
                };
                let diagnostics = collect_in(&app.state::<EditorState>(), &root, env);
                let target = DialogTarget {
                    app: &app,
                    window: &window,
                };
                export_in(&target, &diagnostics)
            })
            .map_err(|e| {
                log::error!("editor diagnostics: could not start its thread: {e}");
                thread_error("could not start")
            })?;
        handle.join().map_err(|_| {
            log::error!("editor diagnostics: its thread panicked");
            thread_error("stopped unexpectedly")
        })?
    })
    .await
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
