//! The editor's media-URL surface (Task 22, R7; ADR §3.3's
//! `editor_media_url`): the ONE way the editor webview learns where a
//! registered asset's or product's bytes live on disk.
//!
//! **The frontend never constructs a path.** It names an entity it already
//! holds an id for (`{ assetId }` or `{ productId }`), and this command
//! answers with an absolute path only when that id is REGISTERED to the
//! caller's own session's project:
//! - an asset must be in the live session's project graph AND have an entry
//!   in the project's `sources.json` (a detached audio asset: its
//!   `linked_asset` root's entry, Task 27), resolved through
//!   `project_store::resolve_source` (so a hand-edited `file` that tries to
//!   escape its directory resolves to nothing);
//! - a product must be in the project's product LEDGER (`products.json`,
//!   Task 46), whose every record is named exactly `<productId>.mp4` under
//!   `products\` (`render_jobs::read_ledger` refuses any other);
//! - a Review render (Task 47) is only ever the project's own
//!   `cache\review-<jobId>.mp4`, named by `render_review` from a valid id.
//!
//! Anything else — an unknown id, a builtin asset with no file, an escaping
//! name, a symlink wearing one of our names — is `unauthorizedSource`; a
//! registered entity whose file is not there is `sourceMissing`.
//!
//! This is NOT the authorization boundary for the asset protocol, and the
//! path it returns is not a capability: `convertFileSrc` is URL conversion
//! (bundle § Access control). The boundary is `tauri.conf.json`'s
//! `assetProtocol.scope` (R7's enumerated list, pinned by `tray.rs`), which
//! Tauri enforces on every request regardless of how the URL was built.
//! This command's job is narrower: keep the webview from ever having to
//! guess a path, so no editor surface can drift into building one.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST — `authz_guard.rs` fails
//! naming any that does not.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use vault_buddy_core::editor::probe::import_extensions;
use vault_buddy_core::editor::{is_valid_id, EditorError, EditorErrorCode};

use super::authz::{require_editor_window, require_session};
use super::media_derive::{editor_ffmpeg, peaks_in, thumbnail_in, MediaPeaks, MediaRequest};
use super::media_import::{run_import, FfprobeImportIo, ImportJob};
use super::media_jobs::{
    cancel_job_in, jobs_in, start_job_in, JobKind, JobPhase, JobProgressDto, JobRecordDto,
    JobReporter, JobStarted, JobTerminal,
};
use super::prefs_commands::{blocking, local_data, project_id_for};
use super::project_store::{join_contained, project_dir, resolve_source, SourceRecord};
use super::render_jobs::read_ledger;
use super::render_review::review_path;
use super::store_io::load_sources;
use super::EditorState;

/// Which registered entity the caller wants a path for. The wire shape is
/// exactly one of `{"assetId": "<id>"}`, `{"productId": "<id>"}` or (Task
/// 47) `{"reviewJobId": "<id>"}` — parsed
/// by hand (`parse_media_ref`) rather than through an untagged serde enum,
/// so a malformed reference is an `invalidRequest` `EditorError` the
/// frontend can branch on, not Tauri's own opaque argument-decode string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaRef {
    Asset(String),
    Product(String),
    Review(String),
}

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn unregistered(what: &str, id: &str) -> EditorError {
    err(
        EditorErrorCode::UnauthorizedSource,
        format!("{what} {id:?} is not a registered source of this project."),
    )
}

/// `{ "assetId": id }` or `{ "productId": id }` — exactly one key, a valid
/// entity id, nothing else. Everything else is `invalidRequest`.
pub(crate) fn parse_media_ref(value: &Value) -> Result<MediaRef, EditorError> {
    let invalid = |why: &str| {
        err(
            EditorErrorCode::InvalidRequest,
            format!(
                "A media reference must be {{assetId}}, {{productId}} or {{reviewJobId}}: {why}"
            ),
        )
    };
    let obj = value.as_object().ok_or_else(|| invalid("not an object"))?;
    if obj.len() != 1 {
        return Err(invalid("exactly one key is required"));
    }
    let (key, id) = obj.iter().next().expect("len checked above");
    let id = id
        .as_str()
        .filter(|id| is_valid_id(id))
        .ok_or_else(|| invalid("the id is not a valid entity id"))?
        .to_string();
    match key.as_str() {
        "assetId" => Ok(MediaRef::Asset(id)),
        "productId" => Ok(MediaRef::Product(id)),
        "reviewJobId" => Ok(MediaRef::Review(id)),
        other => Err(invalid(&format!("unknown key {other:?}"))),
    }
}

/// The file must be a plain file we can hand out — checked no-follow
/// (`symlink_metadata`), so a symlink wearing one of our names is refused
/// rather than followed out of the project directory. Tauri's own scope
/// check resolves symlinks too; this refusal is ours, one step earlier.
fn require_plain_file(path: PathBuf, id: &str) -> Result<PathBuf, EditorError> {
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(unregistered("source", id)),
        Ok(meta) if meta.is_file() => Ok(path),
        Ok(_) => Err(missing(id)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(missing(id)),
        Err(e) => Err(err(
            EditorErrorCode::Internal,
            format!("Cannot inspect the media file for {id:?}: {e}"),
        )),
    }
}

fn missing(id: &str) -> EditorError {
    err(
        EditorErrorCode::SourceMissing,
        format!("The media file for {id:?} is no longer on disk."),
    )
}

/// A registered asset's source, resolved: the id of the `sources.json`
/// record its bytes live under (the asset itself, or its `linked_asset`
/// root), that record, and the plain file it names.
pub(crate) struct ResolvedAsset {
    pub record_id: String,
    pub record: SourceRecord,
    pub path: PathBuf,
}

/// Every refusal `editor_media_url` makes for an asset, shared with the
/// waveform/thumbnail commands (Task 28) so they can never answer for an
/// asset the preview would be refused.
pub(crate) fn resolve_asset(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    project_id: &str,
    asset_id: &str,
) -> Result<ResolvedAsset, EditorError> {
    // In the live graph first: a stale `sources.json` entry for an asset
    // the project no longer holds is not something the preview can ask for.
    // A detached audio asset (Task 27) has no record of its own -- its
    // bytes ARE its video's -- so its lookup key is the `linked_asset` root
    // (`validate_media` guarantees the root is itself unlinked and in the
    // graph, so one hop is the whole chain).
    let record_id = require_session(state, session_id)?
        .get(session_id)
        .and_then(|s| {
            let asset = s.project().assets.iter().find(|a| a.id == asset_id)?;
            Some(
                asset
                    .linked_asset
                    .clone()
                    .unwrap_or_else(|| asset.id.clone()),
            )
        })
        .ok_or_else(|| unregistered("asset", asset_id))?;
    let mut sources = load_sources(root, project_id)?;
    let record = sources
        .remove(&record_id)
        .ok_or_else(|| unregistered("asset", asset_id))?;
    let path =
        resolve_source(root, project_id, &record).ok_or_else(|| unregistered("asset", asset_id))?;
    Ok(ResolvedAsset {
        path: require_plain_file(path, asset_id)?,
        record_id,
        record,
    })
}

/// A product's file, from the LEDGER (Task 46: `products.json` is committed
/// with the render, so an unsaved project's products resolve too). The
/// ledger read refuses any record not named exactly `<productId>.mp4`.
fn product_path(root: &Path, project_id: &str, product_id: &str) -> Result<PathBuf, EditorError> {
    let product = read_ledger(root, project_id)?
        .into_iter()
        .find(|p| p.id == product_id)
        .ok_or_else(|| unregistered("product", product_id))?;
    let dir = project_dir(root, project_id)
        .ok_or_else(|| unregistered("product", product_id))?
        .join("products");
    let path = join_contained(&dir, &product.filename)
        .ok_or_else(|| unregistered("product", product_id))?;
    require_plain_file(path, product_id)
}

/// The absolute path of a registered asset or product of `session_id`'s
/// project — see the module doc for every refusal.
pub(crate) fn media_path_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    media: &MediaRef,
) -> Result<PathBuf, EditorError> {
    let project_id = project_id_for(state, session_id)?;
    match media {
        MediaRef::Asset(id) => {
            resolve_asset(state, root, session_id, &project_id, id).map(|asset| asset.path)
        }
        MediaRef::Product(id) => product_path(root, &project_id, id),
        MediaRef::Review(id) => require_plain_file(review_path(root, &project_id, id)?, id),
    }
}

/// ASYNC: reads `sources.json` (or `products.json`) and stats one file, off
/// the main thread. `ref` is the ADR's own parameter name (`r#ref` —
/// Tauri's command macro unraws it, so the IPC key is `ref`).
#[tauri::command]
pub async fn editor_media_url(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    r#ref: Value,
) -> Result<String, EditorError> {
    require_editor_window(&window)?;
    let media = parse_media_ref(&r#ref)?;
    let root = local_data(&app)?;
    let path =
        blocking(move || media_path_in(&app.state::<EditorState>(), &root, &session_id, &media))
            .await?;
    path_string(&path)
}

fn path_string(path: &Path) -> Result<String, EditorError> {
    path.to_str().map(str::to_string).ok_or_else(|| {
        err(
            EditorErrorCode::Internal,
            "The media path is not valid UTF-8.",
        )
    })
}

// ---- waveforms and thumbnails (Task 28) --------------------------------------

/// ASYNC (ADR §3.3): a cache hit reads one small file; a miss decodes the
/// asset's whole sound through ffmpeg on the named `editor-peaks` thread —
/// minutes of work for a long recording, registered as a cancelable
/// `peaks` job (`media_derive`'s doc).
#[tauri::command]
pub async fn editor_media_peaks(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    asset_id: String,
    buckets: u32,
) -> Result<MediaPeaks, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    let buckets = usize::try_from(buckets).unwrap_or(usize::MAX);
    blocking(move || {
        let request = MediaRequest {
            session_id: &session_id,
            asset_id: &asset_id,
        };
        peaks_in(
            &app.state::<EditorState>(),
            &root,
            &request,
            buckets,
            &editor_ffmpeg,
        )
    })
    .await
}

/// ASYNC (ADR §3.3): one ffmpeg single-frame seek on a miss. Answers the
/// thumbnail's absolute path under the project's `cache\` (inside R7's
/// asset scope), for `convertFileSrc` — the frontend never builds it.
#[tauri::command]
pub async fn editor_media_thumbnail(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    asset_id: String,
    at_ms: u64,
) -> Result<String, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    let path = blocking(move || {
        let request = MediaRequest {
            session_id: &session_id,
            asset_id: &asset_id,
        };
        thumbnail_in(
            &app.state::<EditorState>(),
            &root,
            &request,
            at_ms,
            &editor_ffmpeg,
        )
    })
    .await?;
    path_string(&path)
}

// ---- the import job (Task 25) ------------------------------------------------

/// The files the user picked in the native multi-file dialog — the ONLY way
/// a path reaches the import: the dialog, not a frontend string, is what
/// grants access (ADR §3.3). The filter is the classification allowlist
/// itself (`probe::import_extensions`). Blocking, so it must never run on
/// the main thread; the `editor-import` thread is where it is called.
fn pick_files(app: &AppHandle, window: &WebviewWindow) -> Vec<PathBuf> {
    app.dialog()
        .file()
        .set_title("Import media")
        .add_filter("Video, audio and images", &import_extensions())
        .set_parent(window)
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|picked| match picked.into_path() {
            Ok(path) => Some(path),
            Err(e) => {
                log::warn!("editor import: a picked file has no local path: {e}");
                None
            }
        })
        .collect()
}

/// The `editor-import` thread: `queued` → the dialog → the pipeline, which
/// sends the job's ONE terminal. A dismissed dialog is a `cancelled` job
/// with nothing imported — the user called it off before it began.
fn import_worker(
    app: AppHandle,
    window: WebviewWindow,
    root: PathBuf,
    session_id: String,
    job_id: String,
    cancel: Arc<AtomicBool>,
    channel: Channel<JobProgressDto>,
) {
    let state = app.state::<EditorState>();
    let mut reporter =
        JobReporter::new(&state.jobs, &channel, &session_id, &job_id, JobKind::Import);
    reporter.progress(JobPhase::Queued, 0.0);
    let files = pick_files(&app, &window);
    if files.is_empty() {
        return reporter.finish(
            JobPhase::Cancelled,
            JobTerminal {
                asset_ids: Some(Vec::new()),
                per_file: Some(Vec::new()),
                ..JobTerminal::default()
            },
        );
    }
    let io = FfprobeImportIo::default();
    let job = ImportJob {
        state: &state,
        root: &root,
        session_id: &session_id,
        io: &io,
        cancel: &cancel,
    };
    run_import(&job, &files, reporter);
}

/// ASYNC (ADR §3.3): answers `{jobId}` at once; the dialog, the copies, the
/// probes and the hashing all run on the named `editor-import` thread, and
/// every result travels on `on_progress` — never an app-wide event.
#[tauri::command]
pub async fn editor_import_media(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    on_progress: Channel<JobProgressDto>,
) -> Result<JobStarted, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    let (job_id, cancel) = start_job_in(&app.state::<EditorState>(), &session_id, JobKind::Import)?;
    // Kept so a failed spawn can still end the job it registered: the
    // closure (and the channel inside it) is gone once `spawn` refuses.
    let fallback = on_progress.clone();
    let worker = {
        let (app, session_id, job_id) = (app.clone(), session_id.clone(), job_id.clone());
        move || import_worker(app, window, root, session_id, job_id, cancel, on_progress)
    };
    if let Err(e) = std::thread::Builder::new()
        .name("editor-import".into())
        .spawn(worker)
    {
        log::error!("editor import: could not start the import thread: {e}");
        let error = err(
            EditorErrorCode::Internal,
            format!("The import could not start: {e}"),
        );
        let state = app.state::<EditorState>();
        JobReporter::new(
            &state.jobs,
            &fallback,
            &session_id,
            &job_id,
            JobKind::Import,
        )
        .finish(
            JobPhase::Failed,
            JobTerminal {
                error: Some(error.clone()),
                ..JobTerminal::default()
            },
        );
        return Err(error);
    }
    Ok(JobStarted { job_id })
}

/// SYNC (ADR §3.3): one leaf-lock read and an atomic store — no I/O. Stops
/// FUTURE work only; what a job already finished stays.
#[tauri::command]
pub fn editor_cancel_job(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    job_id: String,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    cancel_job_in(&app.state::<EditorState>(), &session_id, &job_id)
}

/// The authoritative job states after a reload or a missed message
/// (IPC-CONTRACTS.md "reconciliation").
#[tauri::command]
pub async fn editor_get_jobs(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
) -> Result<Vec<JobRecordDto>, EditorError> {
    require_editor_window(&window)?;
    jobs_in(&app.state::<EditorState>(), &session_id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use vault_buddy_core::editor::{Asset, AssetKind, EditorSession, Map, Product};

    use super::*;
    use crate::editor::project_store::{
        minimal_project, SourceLocator, SourceMediaKind, SourceRecord,
    };
    use crate::editor::render_jobs::write_ledger;
    use crate::editor::store_io::create_project;

    fn asset(id: &str) -> Asset {
        Asset {
            id: id.to_string(),
            kind: AssetKind::Video,
            name: format!("{id}.mp4"),
            duration_ms: 4_000,
            width: None,
            height: None,
            size: None,
            builtin: None,
            media_type: None,
            linked_asset: None,
            original_name: None,
            extra: Map::new(),
        }
    }

    fn record(locator: SourceLocator) -> SourceRecord {
        SourceRecord {
            locator,
            sha256: None,
            size: 10,
            duration_ms: 4_000,
            width: Some(1280),
            height: Some(720),
            has_audio: true,
            has_video: true,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        }
    }

    /// A live session over a project whose graph holds `in_graph` and whose
    /// `sources.json` holds `sources`.
    fn opened(
        root: &Path,
        state: &EditorState,
        in_graph: &[&str],
        sources: BTreeMap<String, SourceRecord>,
    ) -> String {
        let mut project = minimal_project("proj1");
        project.assets = in_graph.iter().map(|id| asset(id)).collect();
        create_project(root, &project, &sources).unwrap();
        let session_id = "ses-proj1".to_string();
        let session = EditorSession::resume(session_id.clone(), project, 1);
        state
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), session);
        session_id
    }

    fn write_media(root: &Path, name: &str) -> PathBuf {
        let dir = project_dir(root, "proj1").unwrap().join("media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"media").unwrap();
        path
    }

    // This task's named RED case. The preview must never be handed a path
    // for something the project did not register: an id absent from the
    // graph, an id in the graph with no `sources.json` entry, a builtin
    // (no file at all) and an escaping file name are all
    // `unauthorizedSource`; a registered asset whose file is gone is the
    // DIFFERENT `sourceMissing`, and a registered one that is present
    // resolves under the project's own `media\`.
    #[test]
    fn media_url_refuses_unregistered_assets() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let mut sources = BTreeMap::new();
        sources.insert(
            "real".to_string(),
            record(SourceLocator::Media {
                file: "real.mp4".into(),
            }),
        );
        sources.insert(
            "gone".to_string(),
            record(SourceLocator::Media {
                file: "gone.mp4".into(),
            }),
        );
        sources.insert("synth".to_string(), record(SourceLocator::Builtin));
        sources.insert(
            "escape".to_string(),
            record(SourceLocator::Media {
                file: "..\\..\\secret.mp4".into(),
            }),
        );
        let session = opened(
            root.path(),
            &state,
            &["real", "gone", "synth", "escape", "graph-only"],
            sources,
        );
        let expected = write_media(root.path(), "real.mp4");

        let path = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Asset("real".into()),
        )
        .expect("a registered, present asset resolves");
        assert_eq!(path, expected);

        for id in ["nope", "synth", "escape", "graph-only"] {
            let e = media_path_in(&state, root.path(), &session, &MediaRef::Asset(id.into()))
                .expect_err(id);
            assert_eq!(e.code, EditorErrorCode::UnauthorizedSource, "{id}: {e:?}");
        }
        let e = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Asset("gone".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::SourceMissing);
    }

    // A `sources.json` entry the live graph no longer holds (a deleted asset
    // whose record was not pruned) is not the preview's to ask for.
    #[test]
    fn a_source_record_outside_the_live_graph_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let mut sources = BTreeMap::new();
        sources.insert(
            "stale".to_string(),
            record(SourceLocator::Media {
                file: "stale.mp4".into(),
            }),
        );
        let session = opened(root.path(), &state, &[], sources);
        write_media(root.path(), "stale.mp4");

        let e = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Asset("stale".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::UnauthorizedSource);
    }

    // Task 27: a detached audio asset has no `sources.json` record of its
    // own -- its bytes ARE its video's -- so the preview's lookup follows
    // `linked_asset` to the root's record. Without this the detached clip
    // would play silently while claiming to be the video's sound (R20).
    #[test]
    fn a_detached_audio_asset_resolves_to_its_videos_file() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let mut sources = BTreeMap::new();
        sources.insert(
            "real".to_string(),
            record(SourceLocator::Media {
                file: "real.mp4".into(),
            }),
        );
        let mut project = minimal_project("proj1");
        let mut linked = asset("real-audio");
        linked.kind = AssetKind::Audio;
        linked.linked_asset = Some("real".into());
        let mut orphan = asset("ghost-audio");
        orphan.kind = AssetKind::Audio;
        orphan.linked_asset = Some("ghost".into());
        project.assets = vec![asset("real"), linked, asset("ghost"), orphan];
        create_project(root.path(), &project, &sources).unwrap();
        let session = "ses-proj1".to_string();
        state.sessions.lock().unwrap().insert(
            session.clone(),
            EditorSession::resume(session.clone(), project, 1),
        );
        let expected = write_media(root.path(), "real.mp4");

        let path = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Asset("real-audio".into()),
        )
        .expect("a linked asset resolves through its root");
        assert_eq!(path, expected);

        // A root with no record of its own is still unregistered.
        let e = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Asset("ghost-audio".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::UnauthorizedSource);
    }

    #[test]
    fn a_staged_asset_resolves_into_the_staging_directory() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let mut sources = BTreeMap::new();
        sources.insert(
            "cap".to_string(),
            record(SourceLocator::Staging {
                base: "2026-09-21 1430 Demo".into(),
            }),
        );
        let session = opened(root.path(), &state, &["cap"], sources);
        let staging = vault_buddy_screen::staging::staging_dir(root.path());
        std::fs::create_dir_all(&staging).unwrap();
        let file = staging.join("2026-09-21 1430 Demo.mp4");
        std::fs::write(&file, b"mp4").unwrap();

        let path = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Asset("cap".into()),
        )
        .unwrap();
        assert_eq!(path, file);
    }

    // Task 46: a product is resolved from the LEDGER (`products.json`,
    // committed with the render), never from `project.json`'s record -- an
    // unsaved project's products must play -- and only as
    // `products\<productId>.mp4`: a ledger naming any other file is refused
    // whole, even when that file exists.
    #[test]
    fn media_url_resolves_a_registered_product_and_refuses_an_unknown_one() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let session = opened(root.path(), &state, &[], BTreeMap::new());
        let product = Product {
            id: "prod1".into(),
            project_id: "proj1".into(),
            name: "Review".into(),
            filename: "prod1.mp4".into(),
            mime: "video/mp4".into(),
            revision: 1,
            duration_ms: 4_000,
            created_at: "2026-09-21T14:30:00+02:00".into(),
            edit_fingerprint: "sha256:abc".into(),
            snapshot: None,
            render_range: None,
            extra: Map::new(),
        };
        write_ledger(root.path(), "proj1", std::slice::from_ref(&product)).unwrap();

        let e = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Product("prod1".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::SourceMissing, "not rendered yet");

        let dir = project_dir(root.path(), "proj1").unwrap().join("products");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prod1.mp4"), b"mp4").unwrap();
        let path = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Product("prod1".into()),
        )
        .unwrap();
        assert_eq!(path, dir.join("prod1.mp4"));

        let e = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Product("other".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::UnauthorizedSource);

        std::fs::write(dir.join("evil.mp4"), b"mp4").unwrap();
        let foreign = Product {
            filename: "evil.mp4".into(),
            ..product
        };
        write_ledger(root.path(), "proj1", &[foreign]).unwrap();
        let e = media_path_in(
            &state,
            root.path(),
            &session,
            &MediaRef::Product("prod1".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::InvalidProject, "not <id>.mp4");
    }

    #[test]
    fn media_url_refuses_an_unknown_session() {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let e = media_path_in(
            &state,
            root.path(),
            "ses-nope",
            &MediaRef::Asset("a".into()),
        )
        .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::SessionGone);
    }

    // The wire shape, pinned as LITERAL JSON (never a struct re-serialized
    // against itself): exactly `{assetId}` or `{productId}`.
    #[test]
    fn media_ref_wire_shape_is_pinned() {
        assert_eq!(
            parse_media_ref(&json!({ "assetId": "a1" })).unwrap(),
            MediaRef::Asset("a1".into())
        );
        assert_eq!(
            parse_media_ref(&json!({ "productId": "p1" })).unwrap(),
            MediaRef::Product("p1".into())
        );
        for bad in [
            json!({}),
            json!({ "assetId": "a1", "productId": "p1" }),
            json!({ "asset_id": "a1" }),
            json!({ "path": "C:\\x.mp4" }),
            json!({ "assetId": "../x" }),
            json!({ "assetId": 7 }),
            json!("a1"),
        ] {
            let e = parse_media_ref(&bad).expect_err(&bad.to_string());
            assert_eq!(e.code, EditorErrorCode::InvalidRequest, "{bad}");
        }
    }
}
