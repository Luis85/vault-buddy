//! Derived media (Task 28; F-26; ADR §3.3's `editor_media_peaks` /
//! `editor_media_thumbnail`): waveform peaks and timeline thumbnails,
//! computed by the user-installed ffmpeg and cached in the project's own
//! `cache\` directory (R5: "derived, deletable").
//!
//! **Peaks** — `ffmpeg -v error -i <src> -ac 1 -ar 8000 -f s16le -`, its
//! stdout folded a read at a time by `core::editor::peaks` through
//! `external_stream::run_streaming` on the named `editor-peaks` thread.
//! Memory is the fold's bucket vector (≤ `MAX_BUCKETS`), never the audio.
//! Each decode is registered as a `peaks` job, so `editor_cancel_job` and a
//! closing session (`JobRegistry::cancel_session`) kill the child; the
//! record is FORGOTTEN once the command replies (the reply is the result —
//! there is no Channel message a reconcile would need to recover), so these
//! jobs do not add to GAP-174's never-pruned registry. `PEAKS_GATE` runs
//! one decode at a time: the timeline asks for every visible asset at once,
//! and N parallel full-file decodes would starve the preview. The cache,
//! `cache\<assetId>.peaks.<buckets>.json`, carries the source file's size
//! and mtime, so a file that changed under the same asset id is decoded
//! again rather than drawn stale.
//!
//! **Thumbnails** — `-ss <t> -frames:v 1 -vf scale=160:-2` into
//! `cache\<assetId>-<ms>.jpg`. The time is QUANTIZED to `THUMBNAIL_STEP_MS`
//! (at most one thumbnail per 250 ms of source), which is what bounds the
//! number of distinct files a zoomed-in timeline can ask for; the directory
//! is then held to `MAX_THUMBNAILS` by least-recently-used eviction (a hit
//! refreshes the file's mtime). A thumbnail is NOT fingerprinted — a later
//! relink of an asset id must purge its thumbnails (docs/Gaps.md GAP-176).
//!
//! `<assetId>` in both names is the id of the `sources.json` record the
//! bytes live under (`media_commands::resolve_asset`): a detached audio
//! asset shares its video's waveform instead of decoding the same file
//! twice. Both refuse politely without ffmpeg — `encoderUnavailable`, which
//! the timeline turns into "Install ffmpeg to see waveforms" — but a cache
//! HIT never needs ffmpeg at all.
//!
//! **Lock order:** `PEAKS_GATE`/`THUMBNAIL_GATE` are outermost — never
//! taken while any `EditorState` lock is held; inside them only the
//! `jobs` leaf lock is taken, briefly.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::peaks::{expected_samples, fold_s16le, PeakState, MAX_BUCKETS};
use vault_buddy_core::editor::{is_valid_id, new_entity_id, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::media_commands::{resolve_asset, ResolvedAsset};
use super::media_jobs::{start_job_in, JobKind, JobPhase, JobReporter, NoSubscriber};
use super::prefs_commands::project_id_for;
use super::project_store::{project_dir, SourceMediaKind};
use super::EditorState;
use crate::external_stream::{run_streaming, Streamed};
use crate::external_tool::{run_capturing, tool_command, Capture};

/// Thumbnails are one per this many ms of source time, at most.
pub(crate) const THUMBNAIL_STEP_MS: u64 = 250;
/// The most thumbnails one project's `cache\` keeps (LRU).
pub(crate) const MAX_THUMBNAILS: usize = 200;
/// A decode longer than this is killed: 8 kHz mono decodes a two-hour
/// recording in well under a minute on any machine this app targets.
const PEAKS_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// One input-seeked frame; generous for a slow network share.
const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(30);
/// A cached peaks file larger than this is not ours (4000 floats is ~50 KB).
const MAX_PEAKS_CACHE_BYTES: u64 = 1024 * 1024;

const NO_FFMPEG_WAVEFORM: &str = "Install ffmpeg to see waveforms.";
const NO_FFMPEG_THUMBNAIL: &str = "Install ffmpeg to see thumbnails.";

static PEAKS_GATE: Mutex<()> = Mutex::new(());
static THUMBNAIL_GATE: Mutex<()> = Mutex::new(());
/// The resolved ffmpeg, remembered once found (a resolve spawns two
/// probes). A MISS is never remembered, so installing ffmpeg while the app
/// runs is picked up by the next request.
static RESOLVED_FFMPEG: Mutex<Option<String>> = Mutex::new(None);

/// `editor_media_peaks`' reply: `{ peaks: number[] }`, each in `0..=1`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPeaks {
    pub peaks: Vec<f32>,
}

/// Which asset of which session a derived-media request names.
pub(crate) struct MediaRequest<'a> {
    pub session_id: &'a str,
    pub asset_id: &'a str,
}

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn no_ffmpeg(message: &str) -> EditorError {
    err(EditorErrorCode::EncoderUnavailable, message)
}

/// The ffmpeg the editor's derived media runs — see `RESOLVED_FFMPEG`.
pub(crate) fn editor_ffmpeg() -> Option<String> {
    if let Some(program) = lock_ignoring_poison(&RESOLVED_FFMPEG).clone() {
        return Some(program);
    }
    // Resolved WITHOUT the lock held: a resolve spawns processes.
    let program = crate::ffmpeg::resolve_working_ffmpeg()?.ffmpeg;
    *lock_ignoring_poison(&RESOLVED_FFMPEG) = Some(program.clone());
    Some(program)
}

/// A remembered ffmpeg that no longer spawns (uninstalled, moved) is
/// dropped, so the next request resolves again.
fn forget_editor_ffmpeg() {
    *lock_ignoring_poison(&RESOLVED_FFMPEG) = None;
}

/// The brief's argv, verbatim: mono, 8 kHz, raw s16le on stdout.
pub(crate) fn peaks_args(src: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-v", "error", "-i"].map(OsString::from).to_vec();
    args.push(src.as_os_str().to_owned());
    args.extend(["-ac", "1", "-ar", "8000", "-f", "s16le", "-"].map(OsString::from));
    args
}

/// `ms` as ffmpeg's `seconds.millis` time syntax.
fn seconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// One frame, input-seeked (`-ss` BEFORE `-i`, so ffmpeg seeks the demuxer
/// instead of decoding from zero), 160 px wide with an even height, written
/// as a single JPEG (`-f mjpeg`, because `out` is a temp name whose
/// extension ffmpeg cannot guess a format from).
pub(crate) fn thumbnail_args(src: &Path, at_ms: u64, out: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-v", "error", "-ss"].map(OsString::from).to_vec();
    args.push(seconds(at_ms).into());
    args.push("-i".into());
    args.push(src.as_os_str().to_owned());
    args.extend(["-frames:v", "1", "-vf", "scale=160:-2", "-f", "mjpeg", "-y"].map(OsString::from));
    args.push(out.as_os_str().to_owned());
    args
}

/// Where a thumbnail request lands: clamped inside the source (a seek to
/// the very end yields no frame) and floored to `THUMBNAIL_STEP_MS`. A
/// still image has one picture, at 0.
pub(crate) fn thumbnail_ms(at_ms: u64, duration_ms: u64, still: bool) -> u64 {
    if still {
        return 0;
    }
    let clamped = at_ms.min(duration_ms.saturating_sub(1));
    clamped - clamped % THUMBNAIL_STEP_MS
}

fn thumbnail_name(record_id: &str, ms: u64) -> String {
    format!("{record_id}-{ms}.jpg")
}

/// Is `name` one of OUR thumbnails (`<valid id>-<digits>.jpg`)? The LRU
/// sweep touches nothing else in `cache\`.
fn is_thumbnail_name(name: &str) -> bool {
    let Some((id, ms)) = name.strip_suffix(".jpg").and_then(|s| s.rsplit_once('-')) else {
        return false;
    };
    is_valid_id(id) && !ms.is_empty() && ms.bytes().all(|b| b.is_ascii_digit())
}

/// A real directory, never followed through a symlink/junction.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn is_plain_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

fn cache_path(root: &Path, project_id: &str) -> Result<PathBuf, EditorError> {
    project_dir(root, project_id)
        .map(|dir| dir.join("cache"))
        .ok_or_else(|| err(EditorErrorCode::Internal, "The project id is not valid."))
}

/// The project's `cache\`, created if missing — `create_dir`, never
/// `create_dir_all`: a project directory removed under a running request
/// must stay removed, not be resurrected by a cache write.
fn ensure_cache_dir(root: &Path, project_id: &str) -> Result<PathBuf, EditorError> {
    let cache = cache_path(root, project_id)?;
    let project = cache.parent().map(Path::to_path_buf).unwrap_or_default();
    let refused = || {
        err(
            EditorErrorCode::SourceMissing,
            "The project is no longer on disk.",
        )
    };
    if !is_real_dir(&project) {
        return Err(refused());
    }
    match std::fs::create_dir(&cache) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(err(
                EditorErrorCode::Internal,
                format!("The project's cache could not be created: {e}"),
            ))
        }
    }
    if is_real_dir(&cache) {
        Ok(cache)
    } else {
        Err(refused())
    }
}

/// What a cached waveform was computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceStamp {
    source_size: u64,
    source_modified_ms: u64,
}

impl SourceStamp {
    fn of(path: &Path) -> Result<Self, EditorError> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| {
            err(
                EditorErrorCode::SourceMissing,
                format!("The media file cannot be read: {e}"),
            )
        })?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        Ok(Self {
            source_size: meta.len(),
            source_modified_ms: modified,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedPeaks {
    #[serde(flatten)]
    stamp: SourceStamp,
    peaks: Vec<f32>,
}

/// The cached peaks, only when they are exactly what a fresh decode of the
/// SAME file at the SAME bucket count would give the frontend.
fn read_cached_peaks(path: &Path, buckets: usize, stamp: SourceStamp) -> Option<Vec<f32>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PEAKS_CACHE_BYTES {
        return None;
    }
    let cached: CachedPeaks = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let valid = cached.stamp == stamp
        && cached.peaks.len() == buckets
        && cached.peaks.iter().all(|p| (0.0..=1.0).contains(p));
    valid.then_some(cached.peaks)
}

/// Best-effort: a waveform that could not be cached is still drawn.
fn write_cached_peaks(root: &Path, project_id: &str, name: &str, cached: &CachedPeaks) {
    let written = ensure_cache_dir(root, project_id)
        .map_err(|e| e.message)
        .and_then(|dir| {
            let json = serde_json::to_string(cached).map_err(|e| e.to_string())?;
            write_atomic_replacing(&dir.join(name), &json).map_err(|e| e.to_string())
        });
    if let Err(e) = written {
        log::warn!("editor peaks: the waveform could not be cached: {e}");
    }
}

/// Decode `src`'s sound into `buckets` peaks — the cancelable part.
pub(crate) fn decode_peaks(
    program: &str,
    src: &Path,
    duration_ms: u64,
    buckets: usize,
    cancel: &AtomicBool,
) -> Result<Vec<f32>, EditorError> {
    let mut cmd = tool_command(program);
    cmd.args(peaks_args(src));
    let state = PeakState::new(buckets, expected_samples(duration_ms));
    match run_streaming(
        cmd,
        "editor-peaks",
        cancel,
        PEAKS_TIMEOUT,
        state,
        fold_s16le,
    ) {
        Ok(Streamed::Finished {
            success: true,
            state,
        }) => Ok(state.finish()),
        Ok(Streamed::Finished { success: false, .. }) => Err(err(
            EditorErrorCode::UnsupportedMedia,
            "ffmpeg could not decode this asset's sound.",
        )),
        Ok(Streamed::Cancelled) => Err(err(
            EditorErrorCode::Cancelled,
            "The waveform was cancelled.",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            forget_editor_ffmpeg();
            Err(no_ffmpeg(NO_FFMPEG_WAVEFORM))
        }
        Err(e) => {
            log::warn!("editor peaks: the decode failed: {e}");
            Err(err(
                EditorErrorCode::Internal,
                format!("The waveform could not be computed: {e}"),
            ))
        }
    }
}

/// One decode as a registered `peaks` job — see the module doc.
fn run_peaks_job(
    state: &EditorState,
    session_id: &str,
    program: &str,
    asset: &ResolvedAsset,
    buckets: usize,
) -> Result<Vec<f32>, EditorError> {
    let (job_id, cancel) = start_job_in(state, session_id, JobKind::Peaks)?;
    let result = {
        let _one_at_a_time = lock_ignoring_poison(&PEAKS_GATE);
        JobReporter::new(
            &state.jobs,
            &NoSubscriber,
            session_id,
            &job_id,
            JobKind::Peaks,
        )
        .progress(JobPhase::Preparing, 0.0);
        let duration = asset.record.duration_ms;
        decode_peaks(program, &asset.path, duration, buckets, &cancel)
    };
    lock_ignoring_poison(&state.jobs).forget(&job_id);
    result
}

/// `editor_media_peaks`' body.
pub(crate) fn peaks_in(
    state: &EditorState,
    root: &Path,
    request: &MediaRequest,
    buckets: usize,
    ffmpeg: &dyn Fn() -> Option<String>,
) -> Result<MediaPeaks, EditorError> {
    if !(1..=MAX_BUCKETS).contains(&buckets) {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            format!("A waveform has 1 to {MAX_BUCKETS} buckets."),
        ));
    }
    let project_id = project_id_for(state, request.session_id)?;
    let asset = resolve_asset(
        state,
        root,
        request.session_id,
        &project_id,
        request.asset_id,
    )?;
    if !asset.record.has_audio {
        return Err(err(
            EditorErrorCode::UnsupportedMedia,
            "This asset has no sound to draw.",
        ));
    }
    let stamp = SourceStamp::of(&asset.path)?;
    let name = format!("{}.peaks.{buckets}.json", asset.record_id);
    let cached = cache_path(root, &project_id)?.join(&name);
    if let Some(peaks) = read_cached_peaks(&cached, buckets, stamp) {
        return Ok(MediaPeaks { peaks });
    }
    let program = ffmpeg().ok_or_else(|| no_ffmpeg(NO_FFMPEG_WAVEFORM))?;
    let peaks = run_peaks_job(state, request.session_id, &program, &asset, buckets)?;
    let record = CachedPeaks { stamp, peaks };
    write_cached_peaks(root, &project_id, &name, &record);
    Ok(MediaPeaks {
        peaks: record.peaks,
    })
}

fn remove_quietly(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("editor thumbnail: a temp file could not be removed: {e}");
        }
    }
}

/// Render one thumbnail into an owned temp beside `dest`, then rename it
/// into place — a reader never sees half a JPEG. ffmpeg's stderr is
/// logged, not returned: it names the source file's path.
fn render_thumbnail(program: &str, src: &Path, at_ms: u64, dest: &Path) -> Result<(), EditorError> {
    let file = dest.file_name().map(|n| n.to_string_lossy().into_owned());
    let tmp = dest.with_file_name(format!(
        ".{}.{}.tmp",
        file.unwrap_or_default(),
        new_entity_id("t")
    ));
    let mut cmd = tool_command(program);
    cmd.args(thumbnail_args(src, at_ms, &tmp));
    let result = match run_capturing(cmd, THUMBNAIL_TIMEOUT, Capture::Stderr) {
        Ok((true, _)) if std::fs::metadata(&tmp).is_ok_and(|m| m.len() > 0) => {
            std::fs::rename(&tmp, dest).map_err(|e| {
                err(
                    EditorErrorCode::Internal,
                    format!("The thumbnail could not be stored: {e}"),
                )
            })
        }
        Ok((ok, stderr)) => {
            log::warn!("editor thumbnail: ffmpeg made no frame (ok={ok}): {stderr}");
            Err(err(
                EditorErrorCode::UnsupportedMedia,
                "ffmpeg could not read a picture at this time.",
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            forget_editor_ffmpeg();
            Err(no_ffmpeg(NO_FFMPEG_THUMBNAIL))
        }
        Err(e) => Err(err(
            EditorErrorCode::Internal,
            format!("ffmpeg could not be run: {e}"),
        )),
    };
    remove_quietly(&tmp);
    result
}

/// Mark a thumbnail as just used (the LRU key is its mtime).
fn touch(path: &Path) {
    let touched = std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now()));
    if let Err(e) = touched {
        log::warn!("editor thumbnail: could not refresh a cached thumbnail: {e}");
    }
}

/// Hold `dir`'s thumbnails to `keep`, evicting the least recently used.
/// Only OUR plain files are counted or removed (`is_thumbnail_name`, no
/// symlink): peaks caches and anything else in `cache\` are left alone.
pub(crate) fn prune_thumbnails(dir: &Path, keep: usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("editor thumbnail: the cache could not be listed: {e}");
            return;
        }
    };
    let mut thumbs: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| is_thumbnail_name(&entry.file_name().to_string_lossy()))
        .filter_map(|entry| {
            let meta = std::fs::symlink_metadata(entry.path()).ok()?;
            let used = meta.modified().ok()?;
            meta.is_file().then(|| (used, entry.path()))
        })
        .collect();
    if thumbs.len() <= keep {
        return;
    }
    thumbs.sort_by_key(|(used, _)| *used);
    let evict = thumbs.len() - keep;
    for (_, path) in thumbs.into_iter().take(evict) {
        remove_quietly(&path);
    }
}

/// `editor_media_thumbnail`'s body: the thumbnail's absolute path.
pub(crate) fn thumbnail_in(
    state: &EditorState,
    root: &Path,
    request: &MediaRequest,
    at_ms: u64,
    ffmpeg: &dyn Fn() -> Option<String>,
) -> Result<PathBuf, EditorError> {
    let project_id = project_id_for(state, request.session_id)?;
    let asset = resolve_asset(
        state,
        root,
        request.session_id,
        &project_id,
        request.asset_id,
    )?;
    if !asset.record.has_video {
        return Err(err(
            EditorErrorCode::UnsupportedMedia,
            "This asset has no picture.",
        ));
    }
    let still = asset.record.media_kind == SourceMediaKind::Image;
    let ms = thumbnail_ms(at_ms, asset.record.duration_ms, still);
    let dir = ensure_cache_dir(root, &project_id)?;
    let dest = dir.join(thumbnail_name(&asset.record_id, ms));
    if is_plain_file(&dest) {
        touch(&dest);
        return Ok(dest);
    }
    let program = ffmpeg().ok_or_else(|| no_ffmpeg(NO_FFMPEG_THUMBNAIL))?;
    {
        let _one_at_a_time = lock_ignoring_poison(&THUMBNAIL_GATE);
        render_thumbnail(&program, &asset.path, ms, &dest)?;
    }
    prune_thumbnails(&dir, MAX_THUMBNAILS);
    Ok(dest)
}

#[cfg(test)]
#[path = "media_derive_tests.rs"]
mod tests;
