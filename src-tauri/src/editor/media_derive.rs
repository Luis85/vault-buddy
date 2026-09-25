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
//! **Nothing lands in a closing project (fix round 1, review Minor 2).**
//! Every write into `cache\` — a thumbnail's commit and prune, a waveform's
//! cache file — goes through `with_live_project`: it holds the session's
//! SAVE lock (the one `editor_close_session{discardProject}` holds across
//! its unpin-then-remove) and requires the session to be live under it, and
//! `ensure_cache_dir` refuses a project directory that is gone. A thumbnail
//! is rendered into the OS temp directory first, never into `cache\`, so an
//! ffmpeg still writing cannot hold a file open inside a project being
//! removed. A discard first STOPS the session's derived work
//! (`stop_session_derivations`: peaks jobs cancelled, thumbnail renders
//! killed through `EditorState::thumbnails`, and -- Task 46 -- editor
//! renders cancelled, which kill their ffmpeg and delete their part) and
//! waits a bounded time for it
//! to end; a closing session cancels both too (`drop_session`).
//!
//! **Lock order:** `PEAKS_GATE`/`THUMBNAIL_GATE` are outermost — never
//! taken while any `EditorState` lock is held; inside them only the
//! `jobs` leaf lock is taken, briefly. A session's save lock is taken only
//! AFTER a gate is released, never inside one. `EditorState::thumbnails` and
//! `RESOLVED_FFMPEG` are leaf locks.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use vault_buddy_core::capture_config;
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::peaks::{expected_samples, fold_s16le, PeakState, MAX_BUCKETS};
use vault_buddy_core::editor::{is_valid_id, new_entity_id, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::require_session;
use super::media_commands::{resolve_asset, ResolvedAsset};
use super::media_jobs::{start_job_in, JobKind, JobPhase, JobReporter, JobTerminal, NoSubscriber};
use super::prefs_commands::project_id_for;
use super::project_store::{project_dir, SourceMediaKind};
use super::redact::redact_path;
use super::save_commands::session_save_lock;
use super::EditorState;
use crate::external_stream::{run_streaming, Streamed};
use crate::external_tool::tool_command;

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
/// A rendered thumbnail larger than this is not a 160 px JPEG.
const MAX_THUMBNAIL_BYTES: u64 = 1024 * 1024;
/// How long a discard waits for the session's derived work to stop. A
/// cancelled child is killed within `external_stream`'s 50 ms poll, so
/// this is only ever reached by a wedged one; the discard then proceeds,
/// still protected by `with_live_project`.
const STOP_WAIT: Duration = Duration::from_secs(5);

const NO_FFMPEG_WAVEFORM: &str = "Install ffmpeg to see waveforms.";
const NO_FFMPEG_THUMBNAIL: &str = "Install ffmpeg to see thumbnails.";

static PEAKS_GATE: Mutex<()> = Mutex::new(());
static THUMBNAIL_GATE: Mutex<()> = Mutex::new(());
/// The resolved ffmpeg, remembered once found (a resolve spawns two
/// probes), KEYED on the ffmpeg path configured when it was resolved: a
/// changed setting (`set_ffmpeg_path`, or a hand edit of config.json)
/// resolves again (fix round 1, review Minor 4). A MISS is never
/// remembered, so installing ffmpeg while the app runs is picked up by the
/// next request.
static RESOLVED_FFMPEG: Mutex<Option<Remembered>> = Mutex::new(None);

struct Remembered {
    configured: Option<String>,
    program: String,
}

/// `EditorState::thumbnails`: the thumbnail renders in flight, by session,
/// each with its own cancel flag. Not `JobRegistry` jobs: no wire `JobKind`
/// names a thumbnail, and a row the frontend cannot decode would break
/// `editor_get_jobs`. Per STATE rather than a process static, so two
/// states (two test cases) can never see — or cancel — each other's.
pub type ThumbnailRenders = Mutex<Vec<(String, Arc<AtomicBool>)>>;

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
    let configured = capture_config::load_config().document_import.ffmpeg_path;
    remembered_ffmpeg(&RESOLVED_FFMPEG, configured, || {
        crate::ffmpeg::resolve_working_ffmpeg().map(|tools| tools.ffmpeg)
    })
}

/// `cell`'s program while it was resolved under `configured`, else a fresh
/// `resolve` (run WITHOUT the lock held: a resolve spawns processes).
fn remembered_ffmpeg(
    cell: &Mutex<Option<Remembered>>,
    configured: Option<String>,
    resolve: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if let Some(known) = lock_ignoring_poison(cell)
        .as_ref()
        .filter(|known| known.configured == configured)
    {
        return Some(known.program.clone());
    }
    let program = resolve();
    *lock_ignoring_poison(cell) = program.clone().map(|program| Remembered {
        configured,
        program,
    });
    program
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

/// Run `write` only while `session_id` is live, holding its SAVE lock —
/// the lock a discard holds across its unpin-then-remove, so a cache write
/// never interleaves with `remove_project` (module doc).
fn with_live_project<T>(
    state: &EditorState,
    session_id: &str,
    write: impl FnOnce() -> Result<T, EditorError>,
) -> Result<T, EditorError> {
    let lock = session_save_lock(state, session_id)?;
    let _no_discard_meanwhile = lock_ignoring_poison(&lock);
    drop(require_session(state, session_id)?);
    write()
}

/// Best-effort: a waveform that could not be cached is still drawn, and
/// one that finished after its session closed is not cached at all.
fn write_cached_peaks(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    project_id: &str,
    name: &str,
    cached: &CachedPeaks,
) {
    let written = with_live_project(state, session_id, || {
        let dir = ensure_cache_dir(root, project_id)?;
        let json = serde_json::to_string(cached)
            .map_err(|e| err(EditorErrorCode::Internal, e.to_string()))?;
        write_atomic_replacing(&dir.join(name), &json)
            .map_err(|e| err(EditorErrorCode::Internal, e.to_string()))
    });
    if let Err(e) = written {
        log::warn!("editor peaks: the waveform was not cached: {}", e.message);
    }
}

/// Decode `src`'s sound into `buckets` peaks — the cancelable part.
pub(crate) fn decode_peaks(
    program: &str,
    src: &Path,
    record_id: &str,
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
        Ok(Streamed::Finished { success: false, .. }) => {
            // stderr is nulled (it names the source's path, and an unread
            // pipe could wedge the child), so say at least which source:
            // its record id, plus the path's handle to match other lines.
            log::warn!(
                "editor peaks: ffmpeg exited with an error decoding source {record_id} ({})",
                redact_path(src)
            );
            Err(err(
                EditorErrorCode::UnsupportedMedia,
                "ffmpeg could not decode this asset's sound.",
            ))
        }
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

/// What one peaks request decodes, and where its answer is cached.
struct PeaksPlan<'a> {
    asset: &'a ResolvedAsset,
    project_id: &'a str,
    cache_name: String,
    cached: PathBuf,
    stamp: SourceStamp,
    buckets: usize,
}

/// The phase and terminal a finished decode is recorded with.
fn peaks_terminal(outcome: &Result<Vec<f32>, EditorError>) -> (JobPhase, JobTerminal) {
    let phase = match outcome {
        Ok(_) => JobPhase::Complete,
        Err(e) if e.code == EditorErrorCode::Cancelled => JobPhase::Cancelled,
        Err(_) => JobPhase::Failed,
    };
    let terminal = JobTerminal {
        error: outcome.as_ref().err().cloned(),
        ..JobTerminal::default()
    };
    (phase, terminal)
}

/// One decode as a registered `peaks` job — see the module doc. Re-reads
/// the cache once the gate is held (a request for the same record that
/// queued behind this one finds it there instead of decoding again — review
/// Minor 6), writes the cache BEFORE the record is forgotten (so a discard
/// waiting for the job also waits for its write), and ends the record with
/// a terminal (review Minor 1).
fn run_peaks_job(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    program: &str,
    plan: &PeaksPlan,
) -> Result<Vec<f32>, EditorError> {
    let (job_id, cancel) = start_job_in(state, session_id, JobKind::Peaks)?;
    let result = {
        let _one_at_a_time = lock_ignoring_poison(&PEAKS_GATE);
        let mut reporter = JobReporter::new(
            &state.jobs,
            &NoSubscriber,
            session_id,
            &job_id,
            JobKind::Peaks,
        );
        reporter.progress(JobPhase::Preparing, 0.0);
        let decoded = read_cached_peaks(&plan.cached, plan.buckets, plan.stamp).map_or_else(
            || {
                let (src, duration) = (&plan.asset.path, plan.asset.record.duration_ms);
                let record_id = plan.asset.record_id.as_str();
                decode_peaks(program, src, record_id, duration, plan.buckets, &cancel)
            },
            Ok,
        );
        let (phase, terminal) = peaks_terminal(&decoded);
        reporter.finish(phase, terminal);
        decoded
    };
    if let Ok(peaks) = &result {
        let record = CachedPeaks {
            stamp: plan.stamp,
            peaks: peaks.clone(),
        };
        write_cached_peaks(
            state,
            root,
            session_id,
            plan.project_id,
            &plan.cache_name,
            &record,
        );
    }
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
    let cache_name = format!("{}.peaks.{buckets}.json", asset.record_id);
    let plan = PeaksPlan {
        cached: cache_path(root, &project_id)?.join(&cache_name),
        stamp: SourceStamp::of(&asset.path)?,
        asset: &asset,
        project_id: &project_id,
        cache_name,
        buckets,
    };
    if let Some(peaks) = read_cached_peaks(&plan.cached, buckets, plan.stamp) {
        return Ok(MediaPeaks { peaks });
    }
    let program = ffmpeg().ok_or_else(|| no_ffmpeg(NO_FFMPEG_WAVEFORM))?;
    let peaks = run_peaks_job(state, root, request.session_id, &program, &plan)?;
    Ok(MediaPeaks { peaks })
}

/// One thumbnail render's entry in `EditorState::thumbnails`, removed when
/// dropped — i.e. after the render AND its commit, whichever way they end.
struct InFlight<'a> {
    renders: &'a ThumbnailRenders,
    cancel: Arc<AtomicBool>,
}

impl<'a> InFlight<'a> {
    fn register(renders: &'a ThumbnailRenders, session_id: &str) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        lock_ignoring_poison(renders).push((session_id.to_string(), Arc::clone(&cancel)));
        Self { renders, cancel }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        lock_ignoring_poison(self.renders).retain(|(_, c)| !Arc::ptr_eq(c, &self.cancel));
    }
}

/// Stop `session_id`'s thumbnail renders (a closing session — `drop_session`).
pub(crate) fn cancel_session_thumbnails(state: &EditorState, session_id: &str) {
    for (_, cancel) in lock_ignoring_poison(&state.thumbnails)
        .iter()
        .filter(|(s, _)| s == session_id)
    {
        cancel.store(true, Ordering::SeqCst);
    }
}

fn derivations_running(state: &EditorState, session_id: &str) -> bool {
    let jobs = lock_ignoring_poison(&state.jobs);
    let running = [JobKind::Peaks, JobKind::Render, JobKind::Publish]
        .into_iter()
        .any(|kind| jobs.is_running(session_id, kind));
    drop(jobs);
    running
        || lock_ignoring_poison(&state.thumbnails)
            .iter()
            .any(|(s, _)| s == session_id)
}

/// Cancel `session_id`'s peaks decodes, thumbnail renders, editor renders
/// (Task 46) AND publishes (Task 48) and wait (at most `STOP_WAIT`) for them to end — a
/// discard's first step, taken BEFORE it holds the session's save lock
/// (their final cache write, and a render's publish, need it). A render is
/// killed and its part and job directory deleted before its terminal
/// lands, so the discard never removes the project under a live ffmpeg.
pub(crate) fn stop_session_derivations(state: &EditorState, session_id: &str) {
    let jobs = lock_ignoring_poison(&state.jobs);
    jobs.cancel_session_kind(session_id, JobKind::Peaks);
    jobs.cancel_session_kind(session_id, JobKind::Render);
    // Task 48: a publish reads the product out of the directory a discard
    // removes, and journals into it; it stops between chunks.
    jobs.cancel_session_kind(session_id, JobKind::Publish);
    drop(jobs);
    cancel_session_thumbnails(state, session_id);
    let started = std::time::Instant::now();
    while derivations_running(state, session_id) {
        if started.elapsed() >= STOP_WAIT {
            log::warn!("editor: derived media of session {session_id} did not stop in time");
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn remove_quietly(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("editor thumbnail: a temp file could not be removed: {e}");
        }
    }
}

/// The bytes of a rendered frame, if ffmpeg really made one.
fn read_frame(tmp: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(tmp).ok()?;
    let plausible = meta.is_file() && meta.len() > 0 && meta.len() <= MAX_THUMBNAIL_BYTES;
    plausible.then(|| std::fs::read(tmp).ok()).flatten()
}

/// Render one frame into an owned temp in the OS temp directory — never
/// inside a project (module doc) — and return its bytes. Cancelable:
/// `cancel` kills ffmpeg. Stdout and stderr are nulled/unused (stderr
/// names the source's path); a frame-less exit is logged.
fn render_thumbnail(
    program: &str,
    src: &Path,
    record_id: &str,
    at_ms: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, EditorError> {
    let tmp =
        std::env::temp_dir().join(format!("vault-buddy-{}.thumbnail.tmp", new_entity_id("t")));
    let mut cmd = tool_command(program);
    cmd.args(thumbnail_args(src, at_ms, &tmp));
    let ignore = |_: &[u8], _: &mut ()| {};
    let outcome = run_streaming(
        cmd,
        "editor-thumbnail",
        cancel,
        THUMBNAIL_TIMEOUT,
        (),
        ignore,
    );
    let result = match outcome {
        Ok(Streamed::Finished { success: true, .. }) => read_frame(&tmp).ok_or_else(|| {
            log::warn!(
                "editor thumbnail: ffmpeg made no frame of source {record_id} ({}) at {at_ms} ms",
                redact_path(src)
            );
            err(
                EditorErrorCode::UnsupportedMedia,
                "ffmpeg could not read a picture at this time.",
            )
        }),
        Ok(Streamed::Finished { success: false, .. }) => {
            log::warn!(
                "editor thumbnail: ffmpeg exited with an error on source {record_id} ({})",
                redact_path(src)
            );
            Err(err(
                EditorErrorCode::UnsupportedMedia,
                "ffmpeg could not read a picture at this time.",
            ))
        }
        Ok(Streamed::Cancelled) => Err(err(
            EditorErrorCode::Cancelled,
            "The thumbnail was cancelled.",
        )),
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

/// Store a rendered frame as `cache\<name>` — owned temp then rename, so a
/// reader never sees half a JPEG — and hold the cache to `MAX_THUMBNAILS`,
/// all under `with_live_project`.
fn commit_thumbnail(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    project_id: &str,
    name: &str,
    bytes: &[u8],
) -> Result<PathBuf, EditorError> {
    with_live_project(state, session_id, || {
        let dir = ensure_cache_dir(root, project_id)?;
        let dest = dir.join(name);
        let tmp = dir.join(format!(".{name}.{}.tmp", new_entity_id("t")));
        let stored = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, &dest));
        if let Err(e) = stored {
            remove_quietly(&tmp);
            return Err(err(
                EditorErrorCode::Internal,
                format!("The thumbnail could not be stored: {e}"),
            ));
        }
        prune_thumbnails(&dir, MAX_THUMBNAILS);
        Ok(dest)
    })
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
    let name = thumbnail_name(&asset.record_id, ms);
    let dest = cache_path(root, &project_id)?.join(&name);
    if is_plain_file(&dest) {
        touch(&dest);
        return Ok(dest);
    }
    let program = ffmpeg().ok_or_else(|| no_ffmpeg(NO_FFMPEG_THUMBNAIL))?;
    // Held until the commit below has finished, so a discard's
    // `stop_session_derivations` waits for the whole thing.
    let in_flight = InFlight::register(&state.thumbnails, request.session_id);
    let bytes = {
        let _one_at_a_time = lock_ignoring_poison(&THUMBNAIL_GATE);
        render_thumbnail(
            &program,
            &asset.path,
            &asset.record_id,
            ms,
            &in_flight.cancel,
        )?
    };
    let session = request.session_id;
    commit_thumbnail(state, root, session, &project_id, &name, &bytes)
}

#[cfg(test)]
#[path = "media_derive_tests.rs"]
mod tests;
