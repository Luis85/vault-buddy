//! The media import pipeline behind `editor_import_media` (Task 25, F-02):
//! the files a native dialog granted, one at a time, into the project's own
//! `media\` directory — then ONE `InternalCommand::AddAssets` through the
//! session, so a whole batch is one undo step.
//!
//! Per file, in order: classify by name (`probe::classify_extension`, a
//! hint that only picks the prober) → size cap (`probe::import_plan`) → copy
//! into `media\<assetId>.<ext>` through an owned, exclusive-created
//! `.<assetId>.<ext>.part` renamed with `rename_noreplace` (hashing the
//! bytes on the way through, on this same thread) → probe the COPY (ffprobe
//! for video/audio; images sniffed natively by `probe::sniff_image`) →
//! register the source in `sources.json` → collect the asset.
//!
//! **Never all-or-nothing** (F-02: "valid items remain when another item
//! fails or the batch stops"): a file that fails becomes one `perFile`
//! entry — its DISPLAY name and a message, never a path — and leaves
//! nothing behind: no `.part`, no copy, no `sources.json` entry.
//! Cancellation is checked before each file, so it stops FUTURE files;
//! files already imported still land. Only a failure of the final
//! `AddAssets` (the session closed under the import, say) fails the job, and
//! then the batch's copies and source records are rolled back rather than
//! left as orphans nothing in the graph refers to.
//!
//! `ImportIo` is the seam the tests fake: the ffprobe call, whether ffmpeg
//! is installed at all, and opening a source (so a copy that fails partway
//! is reproducible without a failing disk). Production is `FfprobeImportIo`.

use std::cell::OnceCell;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::commands::payloads::AddAssetsPayload;
use vault_buddy_core::editor::import_io::{copy_hashing, display_name, settle_av_import};
use vault_buddy_core::editor::probe::{
    asset_from_probe, classify_extension, import_plan, sniff_image, ImportKind, ProbeFacts,
};
use vault_buddy_core::editor::{
    limits, new_entity_id, Asset, EditorError, EditorErrorCode, InternalCommand,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::require_session;
use super::media_jobs::{JobPhase, JobReporter, JobTerminal, PerFileError};
use super::media_probe::probe_media;
use super::prefs_commands::project_id_for;
use super::project_store::{project_dir, SourceLocator, SourceMediaKind, SourceRecord};
use super::save_commands::session_save_lock;
use super::store_io::{load_sources, write_sources};
use super::EditorState;
use crate::ffmpeg::{resolve_working_ffmpeg, FfmpegTools};

/// How much of an image is read for `sniff_image` — twice its own JPEG
/// scan cap, so a JPEG whose SOF marker sits right at that cap is still
/// fully in the buffer.
const IMAGE_SNIFF_BYTES: u64 = 512 * 1024;

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn unsupported(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::UnsupportedMedia, message)
}

/// What the pipeline needs from the outside world — faked by the tests.
pub(crate) trait ImportIo {
    /// `Ok` when video/audio can be probed at all; `encoderUnavailable`
    /// naming ffmpeg otherwise. Asked BEFORE a video/audio file is copied,
    /// so a machine with no ffmpeg never copies gigabytes it must delete.
    fn av_ready(&self) -> Result<(), EditorError>;
    /// Probe an already-copied video/audio file.
    fn probe_av(&self, path: &Path, kind: ImportKind) -> Result<ProbeFacts, EditorError>;
    fn open_source(&self, path: &Path) -> io::Result<Box<dyn Read>> {
        Ok(Box::new(File::open(path)?))
    }
}

/// The production seam: the user-installed ffmpeg's ffprobe, resolved once
/// per batch on first need (resolution spawns processes).
#[derive(Default)]
pub(crate) struct FfprobeImportIo {
    tools: OnceCell<Option<FfmpegTools>>,
}

impl FfprobeImportIo {
    fn tools(&self) -> Result<&FfmpegTools, EditorError> {
        self.tools
            .get_or_init(resolve_working_ffmpeg)
            .as_ref()
            .ok_or_else(|| {
                err(
                    EditorErrorCode::EncoderUnavailable,
                    "ffmpeg is not installed, so video and audio cannot be imported. \
                     Install ffmpeg (Buddy settings → Integrations) and import again.",
                )
            })
    }
}

impl ImportIo for FfprobeImportIo {
    fn av_ready(&self) -> Result<(), EditorError> {
        self.tools().map(|_| ())
    }

    fn probe_av(&self, path: &Path, kind: ImportKind) -> Result<ProbeFacts, EditorError> {
        probe_media(self.tools()?, path, kind == ImportKind::Audio).map_err(unsupported)
    }
}

/// Everything one import job runs against.
pub(crate) struct ImportJob<'a> {
    pub state: &'a EditorState,
    pub root: &'a Path,
    pub session_id: &'a str,
    pub io: &'a dyn ImportIo,
    pub cancel: &'a AtomicBool,
}

/// One file that made it: the asset and the file its bytes live in.
struct Imported {
    asset: Asset,
    file: String,
}

/// Run the batch and send its ONE terminal. Never returns an error — every
/// outcome, including the job failing as a whole, is reported through the
/// reporter, because that is the only channel the caller is listening on.
pub(crate) fn run_import(job: &ImportJob, files: &[PathBuf], mut reporter: JobReporter) {
    let project_id = match project_id_for(job.state, job.session_id) {
        Ok(id) => id,
        Err(e) => return fail(reporter, Vec::new(), e),
    };
    let mut imported: Vec<Imported> = Vec::new();
    let mut per_file: Vec<PerFileError> = Vec::new();
    let total = files.len().max(1) as f64;
    for (index, path) in files.iter().enumerate() {
        if job.cancel.load(Ordering::SeqCst) {
            break;
        }
        reporter.progress(JobPhase::Preparing, index as f64 / total);
        match import_one(job, &project_id, path, imported.len()) {
            Ok(one) => imported.push(one),
            Err(e) => per_file.push(PerFileError {
                name: name_of(path),
                error: e.message,
            }),
        }
    }
    let asset_ids: Vec<String> = imported.iter().map(|i| i.asset.id.clone()).collect();
    if !imported.is_empty() {
        if let Err(e) = add_assets(job, &imported) {
            rollback(job, &project_id, &imported);
            return fail(reporter, per_file, e);
        }
    }
    // Checked AFTER the loop, not only inside it: a cancel that arrived
    // while the last file was copying still stopped nothing further, but
    // the user asked, and the terminal says so.
    let phase = if job.cancel.load(Ordering::SeqCst) {
        JobPhase::Cancelled
    } else {
        JobPhase::Complete
    };
    reporter.finish(
        phase,
        JobTerminal {
            asset_ids: Some(asset_ids),
            per_file: Some(per_file),
            ..JobTerminal::default()
        },
    );
}

fn fail(reporter: JobReporter, per_file: Vec<PerFileError>, error: EditorError) {
    log::warn!("editor import failed: {}", error.message);
    reporter.finish(
        JobPhase::Failed,
        JobTerminal {
            asset_ids: Some(Vec::new()),
            per_file: Some(per_file),
            error: Some(error),
            ..JobTerminal::default()
        },
    );
}

/// The file's own name, never its directory — the only part of a path any
/// message or asset may carry.
pub(crate) fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The display name a per-file error and an asset carry.
fn name_of(path: &Path) -> String {
    display_name(&file_name_of(path))
}

fn import_one(
    job: &ImportJob,
    project_id: &str,
    path: &Path,
    imported_so_far: usize,
) -> Result<Imported, EditorError> {
    // Classified (and the extension taken) from the FULL name: the display
    // name is cut at `MAX_NAME_CHARS`, which could cut the extension off.
    let full_name = file_name_of(path);
    let name = display_name(&full_name);
    let kind = classify_extension(&full_name)
        .ok_or_else(|| unsupported("This is not a supported video, audio or image type."))?;
    let size = source_size(path)?;
    if let Some(Err(message)) = import_plan(&[(name.as_str(), size, kind)]).pop() {
        return Err(err(EditorErrorCode::InvalidRequest, message));
    }
    require_capacity(job, imported_so_far)?;
    if kind != ImportKind::Image {
        job.io.av_ready()?;
    }
    let asset_id = new_entity_id("asset");
    let ext = extension_of(&full_name);
    let file = format!("{asset_id}.{ext}");
    let media = media_dir(job.root, project_id)?;
    let (dest, sha256) = copy_owned(job.io, path, &media, &file)?;
    let registered =
        describe(job.io, &dest, kind, size, asset_id.clone(), name).and_then(|(asset, record)| {
            let record = SourceRecord {
                sha256: Some(sha256),
                ..record
            };
            register_source(job, project_id, &asset_id, record, &file).map(|()| asset)
        });
    match registered {
        Ok(asset) => Ok(Imported { asset, file }),
        Err(e) => {
            remove_quietly(&dest);
            Err(e)
        }
    }
}

pub(crate) fn source_size(path: &Path) -> Result<u64, EditorError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        err(
            EditorErrorCode::SourceMissing,
            format!("Cannot read the file: {e}"),
        )
    })?;
    if !meta.is_file() {
        return Err(unsupported("This is not a file."));
    }
    Ok(meta.len())
}

/// Refused per file BEFORE copying: a batch that would overflow
/// `limits::MAX_ASSETS` must not fail its whole `AddAssets` at the end.
fn require_capacity(job: &ImportJob, imported_so_far: usize) -> Result<(), EditorError> {
    let existing = require_session(job.state, job.session_id)?
        .get(job.session_id)
        .map_or(0, |s| s.project().assets.len());
    if existing + imported_so_far >= limits::MAX_ASSETS {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            format!(
                "The project already holds the maximum of {} media items.",
                limits::MAX_ASSETS
            ),
        ));
    }
    Ok(())
}

pub(crate) fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The project's `media\`, created on first import — with `create_dir`, not
/// `create_dir_all`: if the project directory itself is gone (discarded
/// under a running import) this must fail rather than resurrect a partial
/// project directory nothing owns.
pub(crate) fn media_dir(root: &Path, project_id: &str) -> Result<PathBuf, EditorError> {
    let media = project_dir(root, project_id)
        .ok_or_else(|| err(EditorErrorCode::Internal, "The project id is not valid."))?
        .join("media");
    match std::fs::create_dir(&media) {
        Ok(()) => Ok(media),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(media),
        Err(e) => Err(err(
            EditorErrorCode::Internal,
            format!("The project's media folder is unavailable: {e}"),
        )),
    }
}

/// Copy `src` to `media\<file>` via an owned `.part`, returning the final
/// path and the bytes' SHA-256. A failure at ANY step removes the `.part`
/// this call created — and only one it created (`create_new`), never a
/// stranger's file that happened to carry the name.
pub(crate) fn copy_owned(
    io: &dyn ImportIo,
    src: &Path,
    media: &Path,
    file: &str,
) -> Result<(PathBuf, String), EditorError> {
    let part = media.join(format!(".{file}.part"));
    let dest = media.join(file);
    let mut created = false;
    let result = (|| -> io::Result<String> {
        let mut reader = io.open_source(src)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)?;
        created = true;
        let (_, sha256) = copy_hashing(&mut reader, &mut out)?;
        out.sync_all()?;
        drop(out);
        rename_noreplace(&part, &dest)?;
        Ok(sha256)
    })();
    match result {
        Ok(sha256) => Ok((dest, sha256)),
        Err(e) => {
            if created {
                remove_quietly(&part);
            }
            Err(copy_error(e))
        }
    }
}

fn copy_error(e: io::Error) -> EditorError {
    #[cfg(windows)]
    let full = e.kind() == io::ErrorKind::StorageFull || e.raw_os_error() == Some(112);
    #[cfg(not(windows))]
    let full = e.kind() == io::ErrorKind::StorageFull;
    if full {
        err(
            EditorErrorCode::DiskFull,
            format!("Not enough disk space to import the file: {e}"),
        )
    } else {
        err(
            EditorErrorCode::Internal,
            format!("The file could not be copied: {e}"),
        )
    }
}

/// Probe the COPY (never the original — the copy is what the project will
/// play) and build its asset and source record.
pub(crate) fn describe(
    io: &dyn ImportIo,
    dest: &Path,
    declared: ImportKind,
    size: u64,
    asset_id: String,
    name: String,
) -> Result<(Asset, SourceRecord), EditorError> {
    let (kind, facts, dims) = if declared == ImportKind::Image {
        let dims = sniff_copy(dest)?;
        let facts = ProbeFacts {
            duration_ms: 0,
            width: Some(dims.0),
            height: Some(dims.1),
            has_video: true,
            has_audio: false,
        };
        (ImportKind::Image, facts, Some(dims))
    } else {
        let (kind, facts) =
            settle_av_import(declared, io.probe_av(dest, declared)?).map_err(unsupported)?;
        (kind, facts, None)
    };
    let mut asset = asset_from_probe(asset_id, name, kind, facts, dims);
    asset.size = Some(size);
    let record = SourceRecord {
        locator: SourceLocator::Media {
            file: dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        },
        sha256: None,
        size,
        duration_ms: asset.duration_ms,
        width: facts.width,
        height: facts.height,
        has_audio: facts.has_audio,
        has_video: facts.has_video,
        media_kind: match kind {
            ImportKind::Video => SourceMediaKind::Video,
            ImportKind::Audio => SourceMediaKind::Audio,
            ImportKind::Image => SourceMediaKind::Image,
        },
        replaced_from: None,
    };
    Ok((asset, record))
}

fn sniff_copy(dest: &Path) -> Result<(u32, u32), EditorError> {
    let mut head = Vec::new();
    File::open(dest)
        .and_then(|f| f.take(IMAGE_SNIFF_BYTES).read_to_end(&mut head))
        .map_err(|e| {
            err(
                EditorErrorCode::Internal,
                format!("Cannot read the copy: {e}"),
            )
        })?;
    sniff_image(&head)
        .map(|(_, w, h)| (w, h))
        .ok_or_else(|| unsupported("This is not a readable PNG, JPEG or WebP image."))
}

/// Add one record to `sources.json` under the session's save lock (so a
/// concurrent save or discard cannot interleave — `save_commands`'
/// `session_save_lock` doc).
fn register_source(
    job: &ImportJob,
    project_id: &str,
    asset_id: &str,
    record: SourceRecord,
    file: &str,
) -> Result<(), EditorError> {
    let lock = session_save_lock(job.state, job.session_id)?;
    let _guard = lock_ignoring_poison(&lock);
    let mut sources = load_sources(job.root, project_id)
        .map_err(|e| unrecorded(&format!("{file}: {}", e.message)))?;
    sources.insert(asset_id.to_string(), record);
    write_sources(job.root, project_id, &sources).map_err(|e| unrecorded(&format!("{file}: {e}")))
}

/// A `sources.json` failure as the user sees it — path-free (fix round 1:
/// `store_io`'s read error names the file's full path under LOCALAPPDATA,
/// and a per-file error carries a display name and a message, nothing
/// else). The full detail goes to the log.
fn unrecorded(detail: &str) -> EditorError {
    log::warn!("editor import: could not update the project's source registry: {detail}");
    err(
        EditorErrorCode::Internal,
        "The file could not be recorded in the project. See the log for details.",
    )
}

/// The batch's ONE graph edit — so Undo removes the whole import at once.
fn add_assets(job: &ImportJob, imported: &[Imported]) -> Result<(), EditorError> {
    let mut sessions = require_session(job.state, job.session_id)?;
    let session = sessions.get_mut(job.session_id).ok_or_else(|| {
        err(
            EditorErrorCode::Internal,
            "session vanished under its own lock",
        )
    })?;
    let assets = imported.iter().map(|i| i.asset.clone()).collect();
    session.execute_internal(&InternalCommand::AddAssets(AddAssetsPayload { assets }))?;
    drop(sessions);
    // Task 37: an acknowledged edit like any `editor_execute`.
    super::recovery::note_acknowledged(job.state, job.root, job.session_id);
    Ok(())
}

/// Undo a batch whose `AddAssets` was refused: its source records and its
/// copies. Best effort — each failure is logged, and a leftover is only a
/// file nothing in the graph refers to.
fn rollback(job: &ImportJob, project_id: &str, imported: &[Imported]) {
    let removed = (|| -> Result<(), EditorError> {
        // The session is usually GONE here (that is why `AddAssets` failed),
        // and with it its save lock — the revert still runs, unlocked: a
        // closed session has no save left to race.
        let lock = session_save_lock(job.state, job.session_id).ok();
        let _guard = lock.as_deref().map(lock_ignoring_poison);
        let mut sources = load_sources(job.root, project_id)?;
        for one in imported {
            sources.remove(&one.asset.id);
        }
        write_sources(job.root, project_id, &sources)
            .map_err(|e| err(EditorErrorCode::Internal, e.to_string()))
    })();
    if let Err(e) = removed {
        log::warn!(
            "editor import rollback: sources.json not reverted: {}",
            e.message
        );
    }
    if let Some(dir) = project_dir(job.root, project_id) {
        for one in imported {
            remove_quietly(&dir.join("media").join(&one.file));
        }
    }
}

pub(crate) fn remove_quietly(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != io::ErrorKind::NotFound {
            log::warn!("editor import: could not remove {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
#[path = "media_import_tests.rs"]
mod tests;
