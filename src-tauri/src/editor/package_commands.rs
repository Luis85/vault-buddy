//! Portable and lightweight project files (Task 39; F-40; ADR R5, R9;
//! SCREENS 08; A17, A22): `editor_export_package` writes the session's
//! frozen revision to a file the user picks, and `editor_import_package`
//! installs a picked file into the store as a project (`package_import`).
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (R8, `authz_guard.rs`).
//! The native dialogs sit behind `PathChooser`, so everything else runs in
//! the `AppHandle`-free `export_package_in`/`import_package_in` on a
//! tempdir; the commands run both on the named `editor-package` thread,
//! joined from the blocking pool (the `editor-captions` pattern) — a
//! dialog must never block the main thread.
//!
//! An export is a FILE the user owns, never a store commit: it does not
//! touch `project.json`, the session's persisted revision or any staged
//! capture's pin. It lands as an owned same-directory temp renamed into
//! place — `rename_noreplace` for a new name (a first write never
//! replaces), a replacing rename only onto this project's own earlier file
//! of the same format (the dialog's overwrite confirmation); anything else
//! at the chosen name is refused and left byte-identical.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::import_io::{copy_hashing, hashing_reader};
use vault_buddy_core::editor::package::{
    inspect_archive, write_package, PackageManifest, PackageMedia, WORKSPACE_NAME,
};
use vault_buddy_core::editor::package_plan::{
    asset_id_problem, assets_needing_media, attach_source_facts, file_backed_asset_ids,
    media_extension, package_file_name, suggested_file_name, FactsMediaKind, PackageFormat,
    SourceFacts,
};
use vault_buddy_core::editor::{
    self, limits, new_entity_id, EditorError, EditorErrorCode, EditorOpenResult, Map, Record,
    WorkspaceEnvelope, PACKAGE_SCHEMA,
};

use super::authz::{require_editor_window, require_session};
pub(crate) use super::package_import::import_package_in;
use super::prefs_commands::{blocking, local_data, read_workspace};
use super::project_store::{resolve_source, SourceMediaKind, SourceRecord};
use super::save_commands::map_write_error;
use super::store_io::{load_project, load_sources, read_bounded};
use super::EditorState;

/// The refusal a portable export past `MAX_PACKAGE_MEDIA_BYTES` gets (ADR
/// R9, residual R-P1: the lightweight file is the way forward).
const TOO_LARGE: &str = "This project is too large for a portable file (limit 200 MiB). Save a lightweight copy instead.";
/// The refusal for a save target that exists and is not this project's own
/// earlier file of the same format.
const NOT_THIS_PROJECT: &str = "Choose a new name — that file is not this project";

/// Where a package is saved to or opened from — the native dialogs in
/// production (`DialogChooser`), a fixed answer in the tests. `None` is a
/// dismissed dialog.
pub(crate) trait PathChooser {
    fn save_target(&self, format: PackageFormat, suggested: &str) -> Option<PathBuf>;
    fn package_to_open(&self) -> Option<PathBuf>;
}

/// `editor_export_package`'s reply (Contract reference `PackageReceipt`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageReceipt {
    pub session_id: String,
    pub saved_revision: u64,
    pub file_name: String,
    pub format: PackageFormat,
}

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

/// The envelope a package carries: the session's project at
/// `expected_revision` (`revisionConflict` otherwise).
///
/// "Freeze revision": the project is cloned under the sessions lock at the
/// revision the caller saw, so an edit landing while the dialog is open
/// never changes what is written. `record.createdAt` comes from the last
/// saved envelope when there is one; `record.products` is the product
/// LEDGER (`products.json`, Task 46), exactly as `editor_save_project`
/// assembles it.
pub(crate) fn export_envelope(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    expected_revision: u64,
) -> Result<WorkspaceEnvelope, EditorError> {
    let (project, revision) = {
        let sessions = require_session(state, session_id)?;
        let session = sessions.get(session_id).ok_or_else(|| {
            err(
                EditorErrorCode::Internal,
                "session vanished under its own lock",
            )
        })?;
        let revision = session.snapshot().revision;
        if revision != expected_revision {
            return Err(err(
                EditorErrorCode::RevisionConflict,
                format!("expected revision {expected_revision} but the session is at {revision}"),
            ));
        }
        (session.project().clone(), revision)
    };
    let now = chrono::Local::now().to_rfc3339();
    let created_at = load_project(root, &project.id)
        .map(|(envelope, _)| envelope.record.created_at)
        .unwrap_or_else(|_| now.clone());
    let workspace = read_workspace(root, &project.id).unwrap_or_else(|e| {
        log::warn!(
            "editor package: could not read the workspace preferences, exporting none: {}",
            e.message
        );
        serde_json::json!({})
    });
    Ok(WorkspaceEnvelope {
        schema: editor::WORKSPACE_SCHEMA.to_string(),
        record: Record {
            id: project.id.clone(),
            revision,
            created_at,
            updated_at: now.clone(),
            products: super::render_jobs::read_ledger(root, &project.id)?,
            extra: Map::new(),
        },
        project,
        workspace,
        saved_at: now,
        extra: Map::new(),
    })
}

/// One original a portable package carries.
struct MediaFile {
    asset_id: String,
    entry: String,
    path: PathBuf,
}

/// The AVAILABLE originals the package carries (`assets_needing_media`,
/// which includes a retained snapshot's sources, A17, and the `screen`
/// builtin): a source with no record, no file, or a non-file at its path (a
/// symlink, a directory) is left out and reported missing by the import —
/// reported, never silently pruned. A staging locator packages the staged
/// capture's own `.mp4`, whose base exists only on this machine. Refuses
/// the portable format past `MAX_PACKAGE_MEDIA_BYTES` before any dialog.
fn collect_media(
    root: &Path,
    env: &WorkspaceEnvelope,
    sources: &BTreeMap<String, SourceRecord>,
) -> Result<Vec<MediaFile>, EditorError> {
    let mut total: u64 = 0;
    let mut out = Vec::new();
    for asset_id in assets_needing_media(env) {
        let Some(path) = sources
            .get(&asset_id)
            .and_then(|record| resolve_source(root, &env.project.id, record))
        else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        let ext = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(media_extension)
            .unwrap_or_else(|| "bin".to_string());
        total = total.saturating_add(meta.len());
        out.push(MediaFile {
            entry: format!("media/{asset_id}.{ext}"),
            asset_id,
            path,
        });
    }
    if total > limits::MAX_PACKAGE_MEDIA_BYTES {
        return Err(err(EditorErrorCode::InvalidRequest, TOO_LARGE));
    }
    Ok(out)
}

/// Each file-backed source's facts as this machine's `sources.json` records
/// them, carried in the envelope (`package_plan::SOURCE_FACTS_KEY`) so the
/// importing machine does not have to guess them (fix round 1, GAP-182).
fn source_facts(
    env: &WorkspaceEnvelope,
    sources: &BTreeMap<String, SourceRecord>,
) -> BTreeMap<String, SourceFacts> {
    file_backed_asset_ids(env)
        .into_iter()
        .filter_map(|id| {
            let r = sources.get(&id)?;
            let facts = SourceFacts {
                has_audio: r.has_audio,
                has_video: r.has_video,
                width: r.width,
                height: r.height,
                media_kind: match r.media_kind {
                    SourceMediaKind::Video => FactsMediaKind::Video,
                    SourceMediaKind::Audio => FactsMediaKind::Audio,
                    SourceMediaKind::Image => FactsMediaKind::Image,
                },
                size: r.size,
                duration_ms: r.duration_ms,
            };
            Some((id, facts))
        })
        .collect()
}

/// Whether the chosen target is new, or this project's own earlier file of
/// the same format.
enum Placement {
    Create,
    Replace,
}

fn placement(
    target: &Path,
    project_id: &str,
    format: PackageFormat,
) -> Result<Placement, EditorError> {
    let meta = match std::fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Placement::Create),
        Err(e) => return Err(map_write_error(e)),
    };
    let ours = meta.file_type().is_file()
        && existing_project_id(target, format).as_deref() == Some(project_id);
    if ours {
        Ok(Placement::Replace)
    } else {
        Err(err(EditorErrorCode::WriteDenied, NOT_THIS_PROJECT))
    }
}

/// The project id an existing file of `format` belongs to, if it is one.
fn existing_project_id(path: &Path, format: PackageFormat) -> Option<String> {
    match format {
        PackageFormat::Portable => File::open(path)
            .ok()
            .and_then(|file| inspect_archive(&file).ok())
            .map(|index| index.manifest.project_id),
        PackageFormat::Lightweight => read_bounded(path, limits::MAX_PROJECT_JSON_BYTES)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<WorkspaceEnvelope>(&bytes).ok())
            .map(|envelope| envelope.project.id),
    }
}

/// `<chosen>.part-<rand>` in the target's own directory, exclusively
/// created, and removed again unless `keep` is set — on every failure
/// path, including a panic unwinding through the export.
struct OwnedTemp {
    path: PathBuf,
    keep: bool,
}

impl Drop for OwnedTemp {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Err(e) = std::fs::remove_file(&self.path) {
            if e.kind() != io::ErrorKind::NotFound {
                log::warn!("editor package: could not remove a temporary file: {e}");
            }
        }
    }
}

fn create_temp(target: &Path) -> Result<(OwnedTemp, File), EditorError> {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = target.with_file_name(format!("{name}.{}", new_entity_id("part")));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(map_write_error)?;
    Ok((OwnedTemp { path, keep: false }, file))
}

/// The manifest's digests are computed BEFORE the archive is written (the
/// manifest is its first entry), so each file is read twice. The second
/// read, the bytes actually written, is hashed too (fix round 1) and
/// compared with the manifest: a source that changed in between, even at
/// the same size, fails the export instead of yielding a receipt for a file
/// the import would refuse. `write_package` itself refuses a size change.
fn write_portable(
    file: File,
    env: &WorkspaceEnvelope,
    json: &[u8],
    media: &[MediaFile],
) -> Result<(), EditorError> {
    let mut entries = Vec::with_capacity(media.len());
    let mut readers = Vec::with_capacity(media.len());
    let mut digests = Vec::with_capacity(media.len());
    for m in media {
        let (size, sha256) = File::open(&m.path)
            .and_then(|mut f| copy_hashing(&mut f, &mut io::sink()))
            .map_err(map_write_error)?;
        entries.push(PackageMedia {
            asset_id: m.asset_id.clone(),
            path: m.entry.clone(),
            size,
            sha256,
        });
        let (reader, digest) = hashing_reader(File::open(&m.path).map_err(map_write_error)?);
        readers.push((m.entry.clone(), reader));
        digests.push(digest);
    }
    let manifest = PackageManifest {
        schema: PACKAGE_SCHEMA.to_string(),
        project_id: env.project.id.clone(),
        saved_at: env.saved_at.clone(),
        workspace: WORKSPACE_NAME.to_string(),
        media: entries,
        products: Vec::new(),
    };
    #[cfg(test)]
    before_archive_write::run();
    let writer = write_package(BufWriter::new(file), &manifest, json, readers)?;
    for (entry, digest) in manifest.media.iter().zip(&digests) {
        if digest.finish() != (entry.size, entry.sha256.clone()) {
            return Err(err(
                EditorErrorCode::Internal,
                format!(
                    "The original for {:?} changed while the project file was being written. \
                     Nothing was saved; try again.",
                    entry.asset_id
                ),
            ));
        }
    }
    let file = writer
        .into_inner()
        .map_err(|e| map_write_error(e.into_error()))?;
    file.sync_all().map_err(map_write_error)
}

fn write_lightweight(mut file: File, json: &[u8]) -> Result<(), EditorError> {
    file.write_all(json)
        .and_then(|()| file.sync_all())
        .map_err(map_write_error)
}

/// The chosen path with its name normalized to the format's double
/// extension (`package_file_name`).
fn normalized_target(chosen: &Path, format: PackageFormat) -> Result<PathBuf, EditorError> {
    let name = chosen
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| {
            err(
                EditorErrorCode::InvalidRequest,
                "Choose a file name to save to.",
            )
        })?;
    Ok(chosen.with_file_name(package_file_name(&name, format)))
}

/// The `AppHandle`-free half of `editor_export_package`. Every refusal the
/// project itself earns (a stale revision, clashing asset ids, a project
/// too large for the format) comes BEFORE the save dialog; the target check
/// comes before a byte is written.
pub(crate) fn export_package_in(
    state: &EditorState,
    root: &Path,
    chooser: &dyn PathChooser,
    session_id: &str,
    expected_revision: u64,
    format: PackageFormat,
) -> Result<Option<PackageReceipt>, EditorError> {
    let mut envelope = export_envelope(state, root, session_id, expected_revision)?;
    if let Some(problem) = asset_id_problem(&envelope) {
        return Err(err(EditorErrorCode::InvalidRequest, problem));
    }
    let sources = load_sources(root, &envelope.project.id)?;
    let facts = source_facts(&envelope, &sources);
    attach_source_facts(&mut envelope, &facts);
    let json = serde_json::to_vec_pretty(&envelope).map_err(|e| {
        err(
            EditorErrorCode::Internal,
            format!("Could not encode the project: {e}"),
        )
    })?;
    if json.len() as u64 > limits::MAX_PROJECT_JSON_BYTES {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "This project is too large to save as a project file.",
        ));
    }
    let media = match format {
        PackageFormat::Portable => collect_media(root, &envelope, &sources)?,
        PackageFormat::Lightweight => Vec::new(),
    };
    let suggested = suggested_file_name(&envelope.project.title, format);
    let Some(chosen) = chooser.save_target(format, &suggested) else {
        return Ok(None);
    };
    let target = normalized_target(&chosen, format)?;
    let placement = placement(&target, &envelope.project.id, format)?;
    // The native dialog confirmed an overwrite of what the user PICKED
    // (fix round 1): a normalized name that lands on an existing file, even
    // this project's own, is never replaced without that confirmation.
    if matches!(placement, Placement::Replace) && target != chosen {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(err(
            EditorErrorCode::WriteDenied,
            format!(
                "{name:?} already exists. Choose it in the save dialog to replace it, \
                 or pick another name."
            ),
        ));
    }
    let (mut temp, file) = create_temp(&target)?;
    match format {
        PackageFormat::Portable => write_portable(file, &envelope, &json, &media)?,
        PackageFormat::Lightweight => write_lightweight(file, &json)?,
    }
    let placed = match placement {
        Placement::Create => rename_noreplace(&temp.path, &target),
        Placement::Replace => std::fs::rename(&temp.path, &target),
    };
    placed.map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            err(EditorErrorCode::WriteDenied, NOT_THIS_PROJECT)
        } else {
            map_write_error(e)
        }
    })?;
    temp.keep = true;
    Ok(Some(PackageReceipt {
        session_id: session_id.to_string(),
        saved_revision: envelope.record.revision,
        file_name: target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        format,
    }))
}

/// The production `PathChooser`: the dialog plugin, parented to the
/// editor window. Blocking, so it only ever runs on `editor-package`.
struct DialogChooser<'a> {
    app: &'a AppHandle,
    window: &'a WebviewWindow,
}

impl PathChooser for DialogChooser<'_> {
    fn save_target(&self, format: PackageFormat, suggested: &str) -> Option<PathBuf> {
        let (title, filter, extension) = match format {
            PackageFormat::Portable => ("Save a portable copy", "Portable project", "zip"),
            PackageFormat::Lightweight => {
                ("Save a lightweight copy", "Lightweight project", "json")
            }
        };
        let picked = self
            .app
            .dialog()
            .file()
            .set_title(title)
            .add_filter(filter, &[extension])
            .set_file_name(suggested)
            .set_parent(self.window)
            .blocking_save_file()?;
        local_path(picked)
    }

    fn package_to_open(&self) -> Option<PathBuf> {
        let picked = self
            .app
            .dialog()
            .file()
            .set_title("Open a project file")
            .add_filter("Vault Buddy project", &["zip", "json"])
            .set_parent(self.window)
            .blocking_pick_file()?;
        local_path(picked)
    }
}

fn local_path(picked: tauri_plugin_dialog::FilePath) -> Option<PathBuf> {
    match picked.into_path() {
        Ok(path) => Some(path),
        Err(_) => {
            log::warn!("editor package: the chosen file has no local path");
            None
        }
    }
}

/// Runs `work` on the named `editor-package` thread and joins it.
fn on_package_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError> {
    let handle = std::thread::Builder::new()
        .name("editor-package".into())
        .spawn(work)
        .map_err(|e| {
            log::error!("editor package: could not start the package thread: {e}");
            err(
                EditorErrorCode::Internal,
                "The project file could not be prepared.",
            )
        })?;
    handle.join().map_err(|_| {
        log::error!("editor package: the package thread panicked");
        err(
            EditorErrorCode::Internal,
            "Preparing the project file stopped unexpectedly.",
        )
    })?
}

/// ASYNC (ADR §3.3): `null` when the save dialog was dismissed.
#[tauri::command]
pub async fn editor_export_package(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    expected_revision: u64,
    format: PackageFormat,
) -> Result<Option<PackageReceipt>, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        on_package_thread(move || {
            let chooser = DialogChooser {
                app: &app,
                window: &window,
            };
            export_package_in(
                &app.state::<EditorState>(),
                &root,
                &chooser,
                &session_id,
                expected_revision,
                format,
            )
        })
    })
    .await
}

/// ASYNC (ADR §3.3): `null` when the open dialog was dismissed.
#[tauri::command]
pub async fn editor_import_package(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Option<EditorOpenResult>, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        on_package_thread(move || {
            let chooser = DialogChooser {
                app: &app,
                window: &window,
            };
            import_package_in(&app.state::<EditorState>(), &root, &chooser)
        })
    })
    .await
}

/// Test-only seam (fix round 1): runs once, on the exporting thread, after
/// the manifest's digests are computed and before the archive is written,
/// the window a same-size change of a source can land in.
#[cfg(test)]
pub(crate) mod before_archive_write {
    use std::cell::RefCell;

    thread_local! {
        static HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    }

    pub(crate) fn set(hook: impl FnOnce() + 'static) {
        HOOK.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
    }

    pub(super) fn run() {
        if let Some(hook) = HOOK.with(|h| h.borrow_mut().take()) {
            hook();
        }
    }
}

#[cfg(test)]
#[path = "package_commands_tests.rs"]
mod tests;
