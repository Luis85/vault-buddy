//! Webcam takes (Task 49, F-20; ADR R10): the editor webview records with
//! `getUserMedia` + `MediaRecorder` and streams the chunks HERE, into the
//! project's own `takes\` directory — no frame or sample ever crosses JSON
//! or Pinia.
//!
//! - `editor_webcam_begin` refuses while any capture holds `CaptureGuard`
//!   (F35: R10 only asked for a UI rule; the native refusal backs it), then
//!   refuses `encoderUnavailable` when ffmpeg cannot be found (a take it
//!   could not index would be a dead end), then exclusive-creates
//!   `takes\.<takeId>.webm.part` and answers `{takeId}`.
//! - `editor_webcam_append` takes a RAW invoke body (`tauri::ipc::Request`;
//!   a JSON body is refused) with the chunk's session, take and sequence in
//!   the `x-editor-session` / `x-editor-take` / `x-editor-seq` headers,
//!   validates it through `core::editor::take` and appends it. ANY error
//!   marks the take failed: later chunks are refused (a WebM with a hole is
//!   garbage), but the `.part` is kept — with every accepted chunk and
//!   nothing else (a failed write is truncated back) — for finish/discard.
//! - `editor_webcam_finish` validates the last sequence, remuxes the part
//!   `-c copy` into `takes\<takeId>.webm` (MediaRecorder's WebM carries no
//!   duration or cue index, so the preview could not seek it), probes and
//!   hashes the result, records it in `sources.json` (`Takes{file}`) and
//!   registers the asset through ONE `AddAssets` — the same serial session
//!   the timeline edits go through. Without ffmpeg the raw recording is KEPT
//!   as the `.webm` and registered anyway (A09: a take is never lost), and
//!   the answer is `encoderUnavailable` carrying its asset id.
//! - `editor_webcam_discard` removes an unfinished take's own `.part`; a
//!   FINISHED take's `.webm` is never deleted (its asset — or the undo
//!   history — may still need it), and one a clip plays is refused.
//!
//! Which append errors fail the take: everything decided AFTER the take is
//! identified (a JSON or oversized body, a sequence gap, a size limit, a
//! write). A missing or malformed header is refused before any take is
//! known, so it cannot fail one — the headers are what name it.
//!
//! **A take does not claim `CaptureGuard`** (it holds no native device;
//! GAP-193): a screen capture started DURING a take is not refused.
//!
//! **Locks.** `EditorState::takes` (`TakeRegistry`) is a LEAF lock, taken
//! only to find, add or remove one slot. Each slot's own `entry` lock is
//! held across that take's file I/O (so its chunks, its finish and its
//! discard are serial) and, in finish, across the save lock and then
//! `sessions` — so the order is: take entry, then save lock, then
//! `sessions`. Nothing takes a take entry while holding the save lock:
//! `forget_session` (run by `drop_session` under it) never locks an entry.
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (`authz_guard.rs`).

use std::cell::OnceCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::commands::payloads::AddAssetsPayload;
use vault_buddy_core::editor::import_io::copy_hashing;
use vault_buddy_core::editor::probe::ProbeFacts;
use vault_buddy_core::editor::take::{
    asset_in_use, is_take_mime, next_take_ordinal, parse_seq, take_asset, TakeError, TakeState,
    MAX_TAKE_CHUNK_BYTES, TAKE_ID_PREFIX, TAKE_MIME_TYPES,
};
use vault_buddy_core::editor::{
    is_valid_id, new_entity_id, Asset, EditorError, EditorErrorCode, InternalCommand,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::media_probe::probe_media;
use super::prefs_commands::{blocking, local_data, project_id_for};
use super::project_store::{project_dir, SourceLocator, SourceMediaKind, SourceRecord};
use super::save_commands::session_save_lock;
use super::store_io::{load_sources, write_sources};
use super::EditorState;
use crate::capture_guard::{CaptureGuard, CaptureKind};
use crate::ffmpeg::{resolve_working_ffmpeg, FfmpegTools};

/// The three headers an append carries.
pub(crate) const HEADER_SESSION: &str = "x-editor-session";
pub(crate) const HEADER_TAKE: &str = "x-editor-take";
pub(crate) const HEADER_SEQ: &str = "x-editor-seq";

/// Wall-clock bound on the finish remux. A `-c copy` of even a 4 GiB take
/// is a disk-speed copy; this exists only so a wedged ffmpeg cannot hold
/// the take (and its session's finish) forever.
const REMUX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn invalid(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::InvalidRequest, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

/// `editor_webcam_begin`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeStarted {
    pub take_id: String,
}

/// Contract reference `TakeDto` — `editor_webcam_finish`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeDto {
    pub take_id: String,
    pub asset_id: String,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}

/// One take's mutable state, behind its slot's lock.
pub(crate) struct TakeEntry {
    pub(crate) state: TakeState,
    /// Why the take stopped accepting chunks, once it has.
    pub(crate) failed: Option<String>,
}

/// One live take: who owns it and where its files live (outside the lock,
/// so `forget_session` can clean up without ever waiting on a take).
pub(crate) struct TakeSlot {
    session_id: String,
    project_id: String,
    dir: PathBuf,
    take_id: String,
    pub(crate) entry: Mutex<TakeEntry>,
}

impl TakeSlot {
    fn part(&self) -> PathBuf {
        self.dir.join(format!(".{}.webm.part", self.take_id))
    }

    fn remux_temp(&self) -> PathBuf {
        self.dir.join(format!(".{}.remux.webm", self.take_id))
    }

    fn file_name(&self) -> String {
        format!("{}.webm", self.take_id)
    }

    fn out(&self) -> PathBuf {
        self.dir.join(self.file_name())
    }
}

/// Every take this process began and has not yet discarded, by take id.
#[derive(Default)]
pub struct TakeRegistry(Mutex<HashMap<String, Arc<TakeSlot>>>);

impl TakeRegistry {
    /// The take, if `session_id` owns it — `invalidRequest` otherwise, so a
    /// guessed or another session's take id learns nothing.
    fn get(&self, session_id: &str, take_id: &str) -> Result<Arc<TakeSlot>, EditorError> {
        lock_ignoring_poison(&self.0)
            .get(take_id)
            .filter(|slot| slot.session_id == session_id)
            .cloned()
            .ok_or_else(|| invalid("This webcam take is not part of this editing session."))
    }

    /// The ids of `session_id`'s takes that are still recording — what the
    /// close guard would lose, and what Checks counts as unfinished (Task
    /// 54, `checks_commands`). Never locks an entry for longer than one
    /// read.
    pub(crate) fn open_takes(&self, session_id: &str) -> Vec<String> {
        let slots: Vec<Arc<TakeSlot>> = lock_ignoring_poison(&self.0)
            .values()
            .filter(|s| s.session_id == session_id)
            .cloned()
            .collect();
        let mut ids: Vec<String> = slots
            .iter()
            .filter(|s| {
                matches!(
                    lock_ignoring_poison(&s.entry).state,
                    TakeState::Recording { .. }
                )
            })
            .map(|s| s.take_id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// A closing session's takes: forgotten, and every unfinished take's
    /// `.part` removed (nothing could ever finish it now). Runs under the
    /// session's save lock (`drop_session`), so it NEVER locks an entry —
    /// a finish holds its entry while it takes the save lock. A finished
    /// take has no `.part`, so its `.webm` is untouched.
    pub(crate) fn forget_session(&self, session_id: &str) {
        let gone: Vec<Arc<TakeSlot>> = {
            let mut map = lock_ignoring_poison(&self.0);
            let ids: Vec<String> = map
                .iter()
                .filter(|(_, s)| s.session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter().filter_map(|id| map.remove(id)).collect()
        };
        for slot in gone {
            remove_owned(&slot.part());
        }
    }
}

// ---- begin ----------------------------------------------------------------

/// The refusal `editor_webcam_begin` gives while a capture holds the
/// devices (F35).
fn capture_running(kind: CaptureKind) -> EditorError {
    let what = match kind {
        CaptureKind::Screen => "screen",
        CaptureKind::Audio => "audio",
    };
    err(
        EditorErrorCode::DeviceUnavailable,
        format!("Stop the {what} recording first."),
    )
}

/// The project's `takes\`, created on first use with `create_dir` (never
/// `create_dir_all`: a project discarded under a take must not be
/// resurrected as a bare directory nothing owns).
fn takes_dir(root: &Path, project_id: &str) -> Result<PathBuf, EditorError> {
    let dir = project_dir(root, project_id)
        .ok_or_else(|| internal("The project id is not valid."))?
        .join("takes");
    match std::fs::create_dir(&dir) {
        Ok(()) => Ok(dir),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(dir),
        Err(e) => Err(internal(format!(
            "The project's takes folder is unavailable: {e}"
        ))),
    }
}

/// The `AppHandle`-free half of `editor_webcam_begin`. `capture` is
/// `CaptureGuard::active()`, read by the caller.
pub(crate) fn begin_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    mime_type: &str,
    capture: Option<CaptureKind>,
    io: &dyn TakeIo,
) -> Result<TakeStarted, EditorError> {
    let project_id = project_id_for(state, session_id)?;
    if !is_take_mime(mime_type) {
        return Err(invalid(format!(
            "A webcam take must be recorded as one of {}.",
            TAKE_MIME_TYPES.join(", ")
        )));
    }
    if let Some(kind) = capture {
        return Err(capture_running(kind));
    }
    // Fix round 1 (controller ruling): without ffmpeg a take could only
    // land unindexed, with an unknown length — kept, but never placeable.
    // Refuse before anything is recorded; finish's raw-keep path remains
    // only for ffmpeg vanishing mid-take (GAP-196).
    io.ready()?;
    let dir = takes_dir(root, &project_id)?;
    let take_id = new_entity_id(TAKE_ID_PREFIX);
    let slot = TakeSlot {
        session_id: session_id.to_string(),
        project_id,
        dir,
        take_id: take_id.clone(),
        entry: Mutex::new(TakeEntry {
            state: TakeState::new(),
            failed: None,
        }),
    };
    // Exclusive: a stranger's file wearing this name is never appended to.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(slot.part())
        .map_err(|e| write_error("start the take", &e))?;
    lock_ignoring_poison(&state.takes.0).insert(take_id.clone(), Arc::new(slot));
    Ok(TakeStarted { take_id })
}

/// ASYNC: creates the take's `.part` off the main thread.
#[tauri::command]
pub async fn editor_webcam_begin(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    mime_type: String,
) -> Result<TakeStarted, EditorError> {
    require_editor_window(&window)?;
    let capture = app.state::<CaptureGuard>().active();
    let root = local_data(&app)?;
    blocking(move || {
        begin_in(
            &app.state::<EditorState>(),
            &root,
            &session_id,
            &mime_type,
            capture,
            &FfmpegTakeIo::default(),
        )
    })
    .await
}

// ---- append ---------------------------------------------------------------

/// Which chunk an append carries, from its headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChunkHeaders {
    pub session_id: String,
    pub take_id: String,
    pub seq: u64,
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, EditorError> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| invalid(format!("A webcam chunk must carry the {name} header.")))
}

pub(crate) fn chunk_headers(headers: &HeaderMap) -> Result<ChunkHeaders, EditorError> {
    let session_id = header(headers, HEADER_SESSION)?.to_string();
    let take_id = header(headers, HEADER_TAKE)?;
    if !is_valid_id(take_id) {
        return Err(invalid("The webcam take id is not valid."));
    }
    let seq = parse_seq(header(headers, HEADER_SEQ)?)
        .ok_or_else(|| invalid(format!("{HEADER_SEQ} must be a whole number.")))?;
    Ok(ChunkHeaders {
        session_id,
        take_id: take_id.to_string(),
        seq,
    })
}

/// The chunk's bytes: only a RAW body is a chunk. A JSON body (an array of
/// numbers, say) would put every sample through the JSON codec — exactly
/// what R10 keeps off the wire.
pub(crate) fn raw_body(body: &InvokeBody) -> Result<&[u8], EditorError> {
    match body {
        InvokeBody::Raw(bytes) => Ok(bytes),
        InvokeBody::Json(_) => Err(invalid(
            "A webcam chunk must be sent as raw bytes, not JSON.",
        )),
    }
}

/// The chunk, copied out of the request — but only after its size is
/// checked on the BORROWED body, so an oversized body is never duplicated
/// in memory first. (`accept_chunk` checks the same bound again.)
pub(crate) fn raw_chunk(body: &InvokeBody) -> Result<Vec<u8>, EditorError> {
    let bytes = raw_body(body)?;
    let len = bytes.len() as u64;
    if len > MAX_TAKE_CHUNK_BYTES {
        return Err(TakeError::ChunkTooLarge { len }.into());
    }
    Ok(bytes.to_vec())
}

/// The `AppHandle`-free half of `editor_webcam_append`. `chunk` is the raw
/// body or why there is none — an error either way marks the take failed.
pub(crate) fn append_in(
    state: &EditorState,
    at: &ChunkHeaders,
    chunk: Result<&[u8], EditorError>,
) -> Result<(), EditorError> {
    drop(require_session(state, &at.session_id)?);
    let slot = state.takes.get(&at.session_id, &at.take_id)?;
    let mut entry = lock_ignoring_poison(&slot.entry);
    if let Some(reason) = &entry.failed {
        return Err(invalid(format!(
            "This webcam take stopped recording ({reason}). Finish or discard it."
        )));
    }
    let result = chunk.and_then(|bytes| {
        let mut next = entry.state.clone();
        next.accept_chunk(at.seq, bytes.len() as u64)?;
        let before = entry.state.bytes().unwrap_or(0);
        append_bytes(&slot.part(), before, bytes)?;
        entry.state = next;
        Ok(())
    });
    if let Err(e) = &result {
        log::warn!("webcam take {} failed: {}", at.take_id, e.message);
        entry.failed = Some(e.message.clone());
    }
    result
}

/// Append `bytes` to the part (never following a symlink wearing its
/// name), flushed before returning. A failed write is truncated back to
/// the `before` bytes the take had already accepted, so the part only ever
/// holds whole, accepted chunks.
fn append_bytes(part: &Path, before: u64, bytes: &[u8]) -> Result<(), EditorError> {
    require_owned_file(part)?;
    let file = OpenOptions::new()
        .append(true)
        .open(part)
        .map_err(|e| write_error("save the webcam chunk", &e))?;
    let mut out = BufWriter::new(file);
    let written = out.write_all(bytes).and_then(|()| out.flush());
    if let Err(e) = written {
        let file = out.into_parts().0;
        if let Err(t) = file.set_len(before) {
            log::warn!("webcam take: could not trim a failed chunk: {t}");
        }
        return Err(write_error("save the webcam chunk", &e));
    }
    Ok(())
}

/// ASYNC: one chunk's write, off the main thread. The body is copied out
/// of the request (≤ 1 MiB) before the blocking hop.
#[tauri::command]
pub async fn editor_webcam_append(
    window: WebviewWindow,
    app: AppHandle,
    request: Request<'_>,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    let at = chunk_headers(request.headers())?;
    let chunk = raw_chunk(request.body());
    blocking(move || {
        append_in(
            &app.state::<EditorState>(),
            &at,
            chunk.as_deref().map_err(Clone::clone),
        )
    })
    .await
}

// ---- finish ---------------------------------------------------------------

/// What finish needs from the outside world — faked by the tests.
pub(crate) trait TakeIo {
    /// Remux `part` into `out` (`-c copy`); `encoderUnavailable` when
    /// ffmpeg is not installed.
    fn remux(&self, part: &Path, out: &Path) -> Result<(), EditorError>;
    fn probe(&self, path: &Path) -> Result<ProbeFacts, EditorError>;
    /// `Ok` when ffmpeg can be used at all; `encoderUnavailable` otherwise.
    fn ready(&self) -> Result<(), EditorError>;
}

/// Production: the user-installed ffmpeg, resolved once per finish.
#[derive(Default)]
pub(crate) struct FfmpegTakeIo {
    tools: OnceCell<Option<FfmpegTools>>,
}

impl FfmpegTakeIo {
    fn tools(&self) -> Result<&FfmpegTools, EditorError> {
        self.tools
            .get_or_init(resolve_working_ffmpeg)
            .as_ref()
            .ok_or_else(|| {
                err(
                    EditorErrorCode::EncoderUnavailable,
                    "ffmpeg is not installed. A webcam take needs it to be given a length and \
                     placed on the timeline — install ffmpeg (Buddy settings → Integrations) \
                     and record again.",
                )
            })
    }
}

impl TakeIo for FfmpegTakeIo {
    fn ready(&self) -> Result<(), EditorError> {
        self.tools().map(|_| ())
    }

    fn remux(&self, part: &Path, out: &Path) -> Result<(), EditorError> {
        use crate::external_tool::{run_capturing, tool_command, Capture};
        let mut cmd = tool_command(&self.tools()?.ffmpeg);
        // `-n`: never overwrite — the output name is ours, and a file
        // already wearing it is refused rather than clobbered.
        cmd.args(["-hide_banner", "-nostdin", "-v", "error", "-n", "-i"])
            .arg(part)
            .args(["-map", "0", "-c", "copy", "-f", "webm"])
            .arg(out);
        let (ok, stderr) = run_capturing(cmd, REMUX_TIMEOUT, Capture::Stderr)
            .map_err(|e| internal(format!("Could not run ffmpeg: {e}")))?;
        if ok {
            return Ok(());
        }
        log::warn!("webcam take remux failed: {}", stderr.trim());
        Err(err(
            EditorErrorCode::UnsupportedMedia,
            "ffmpeg could not read the recorded take; it may be damaged.",
        ))
    }

    fn probe(&self, path: &Path) -> Result<ProbeFacts, EditorError> {
        probe_media(self.tools()?, path, false)
            .map_err(|m| err(EditorErrorCode::UnsupportedMedia, m))
    }
}

/// The `AppHandle`-free half of `editor_webcam_finish` — see the module
/// doc. On any failure before the take lands, the `.part` is still there
/// and the take can be finished again or discarded.
pub(crate) fn finish_in(
    state: &EditorState,
    root: &Path,
    io: &dyn TakeIo,
    session_id: &str,
    take_id: &str,
    last_seq: u64,
) -> Result<TakeDto, EditorError> {
    drop(require_session(state, session_id)?);
    let slot = state.takes.get(session_id, take_id)?;
    let mut entry = lock_ignoring_poison(&slot.entry);
    let mut next = entry.state.clone();
    next.finish(last_seq)?;
    let landed = land(state, root, io, &slot)?;
    entry.state = next;
    match landed {
        Landed::Indexed(dto) => Ok(dto),
        Landed::Raw(asset_id) => Err(EditorError {
            retained_asset_ids: Some(vec![asset_id]),
            ..err(
                EditorErrorCode::EncoderUnavailable,
                "ffmpeg could not be found when the take ended, so it was kept exactly as \
                 recorded. It plays from its start in the preview but cannot be seeked, and its \
                 length is unknown, so it cannot be placed on the timeline.",
            )
        }),
    }
}

enum Landed {
    Indexed(TakeDto),
    /// No ffmpeg: the raw recording was kept and registered as this asset.
    Raw(String),
}

fn land(
    state: &EditorState,
    root: &Path,
    io: &dyn TakeIo,
    slot: &TakeSlot,
) -> Result<Landed, EditorError> {
    let temp = slot.remux_temp();
    if std::fs::symlink_metadata(&temp).is_ok() {
        return Err(internal(
            "A leftover file is in the way of this take; discard the take and record again.",
        ));
    }
    match io.remux(&slot.part(), &temp) {
        Ok(()) => land_indexed(state, root, io, slot, &temp).inspect_err(|_| remove_owned(&temp)),
        Err(e) if e.code == EditorErrorCode::EncoderUnavailable => land_raw(state, root, slot),
        Err(e) => {
            remove_owned(&temp);
            Err(e)
        }
    }
}

fn land_indexed(
    state: &EditorState,
    root: &Path,
    io: &dyn TakeIo,
    slot: &TakeSlot,
    temp: &Path,
) -> Result<Landed, EditorError> {
    let facts = io.probe(temp)?;
    let (Some(width), Some(height)) = (facts.width, facts.height) else {
        return Err(err(
            EditorErrorCode::UnsupportedMedia,
            "The recorded take has no picture.",
        ));
    };
    let (size, sha256) = hash_file(temp)?;
    let out = slot.out();
    rename_noreplace(temp, &out).map_err(|e| write_error("keep the take", &e))?;
    let asset =
        register(state, root, slot, facts, size, sha256).inspect_err(|_| remove_owned(&out))?;
    remove_owned(&slot.part());
    Ok(Landed::Indexed(TakeDto {
        take_id: slot.take_id.clone(),
        asset_id: asset.id,
        duration_ms: facts.duration_ms,
        width,
        height,
        has_audio: facts.has_audio,
    }))
}

/// A09: no ffmpeg — the recorded bytes BECOME the take's `.webm`, and are
/// registered with what is known of them (nothing can be probed, so the
/// length is unknown and recorded as 0 — never guessed).
fn land_raw(state: &EditorState, root: &Path, slot: &TakeSlot) -> Result<Landed, EditorError> {
    let part = slot.part();
    let (size, sha256) = hash_file(&part)?;
    let out = slot.out();
    rename_noreplace(&part, &out).map_err(|e| write_error("keep the take", &e))?;
    let facts = ProbeFacts {
        duration_ms: 0,
        width: None,
        height: None,
        has_video: true,
        has_audio: false,
    };
    match register(state, root, slot, facts, size, sha256) {
        Ok(asset) => Ok(Landed::Raw(asset.id)),
        Err(e) => {
            // Back to a `.part`, so the take can still be finished or
            // discarded — the recording is never left nameless.
            if let Err(back) = rename_noreplace(&out, &part) {
                log::warn!("webcam take: could not restore {}: {back}", part.display());
            }
            Err(e)
        }
    }
}

/// Record the take in `sources.json` (under the save lock), then add its
/// asset through the session — one undo step. A refused `AddAssets` takes
/// the record back out.
fn register(
    state: &EditorState,
    root: &Path,
    slot: &TakeSlot,
    facts: ProbeFacts,
    size: u64,
    sha256: String,
) -> Result<Asset, EditorError> {
    let lock = session_save_lock(state, &slot.session_id)?;
    let _guard = lock_ignoring_poison(&lock);
    let asset_id = slot.take_id.clone();
    let record = SourceRecord {
        locator: SourceLocator::Takes {
            file: slot.file_name(),
        },
        sha256: Some(sha256),
        size,
        duration_ms: facts.duration_ms,
        width: facts.width,
        height: facts.height,
        has_audio: facts.has_audio,
        has_video: facts.has_video,
        media_kind: SourceMediaKind::Video,
        replaced_from: None,
    };
    // Path-free, like every other message here: `store_io`'s read error
    // names the file's full path under LOCALAPPDATA (the log keeps it).
    let mut sources = load_sources(root, &slot.project_id).map_err(|e| {
        log::warn!("webcam take: could not read sources.json: {}", e.message);
        internal("The take could not be recorded in the project. See the log for details.")
    })?;
    sources.insert(asset_id.clone(), record);
    store_sources(root, &slot.project_id, &sources)?;
    let added = (|| {
        let mut sessions = require_session(state, &slot.session_id)?;
        let session = sessions
            .get_mut(&slot.session_id)
            .ok_or_else(|| internal("session vanished under its own lock"))?;
        let ordinal = next_take_ordinal(session.project());
        let asset = take_asset(asset_id.clone(), ordinal, facts, size);
        session.execute_internal(&InternalCommand::AddAssets(AddAssetsPayload {
            assets: vec![asset.clone()],
        }))?;
        Ok(asset)
    })();
    match added {
        Ok(asset) => {
            super::recovery::note_acknowledged(state, root, &slot.session_id);
            Ok(asset)
        }
        Err(e) => {
            sources.remove(&asset_id);
            if let Err(w) = store_sources(root, &slot.project_id, &sources) {
                log::warn!("webcam take: sources.json not reverted: {}", w.message);
            }
            Err(e)
        }
    }
}

fn store_sources(
    root: &Path,
    project_id: &str,
    sources: &BTreeMap<String, SourceRecord>,
) -> Result<(), EditorError> {
    write_sources(root, project_id, sources).map_err(|e| {
        log::warn!("webcam take: could not update sources.json: {e}");
        internal("The take could not be recorded in the project. See the log for details.")
    })
}

fn hash_file(path: &Path) -> Result<(u64, String), EditorError> {
    File::open(path)
        .and_then(|mut f| copy_hashing(&mut f, &mut io::sink()))
        .map_err(|e| write_error("read the take", &e))
}

/// ASYNC: a remux (a child process), a probe and a hash, off the main
/// thread.
#[tauri::command]
pub async fn editor_webcam_finish(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    take_id: String,
    last_seq: u64,
) -> Result<TakeDto, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        finish_in(
            &app.state::<EditorState>(),
            &root,
            &FfmpegTakeIo::default(),
            &session_id,
            &take_id,
            last_seq,
        )
    })
    .await
}

// ---- discard --------------------------------------------------------------

/// The `AppHandle`-free half of `editor_webcam_discard`.
///
/// An UNFINISHED take: its owned `.part` is removed. A FINISHED take is
/// already an asset — its `.webm` is NEVER deleted here (fix round 1): the
/// in-use check sees only the current clips, never the undo history, so a
/// clip deleted and then brought back by Undo would otherwise play a file
/// the discard removed. A finished take a clip plays is refused; any other
/// finished take is only forgotten as a pending take, its file and asset
/// kept (no command removes an asset — GAP-195).
pub(crate) fn discard_in(
    state: &EditorState,
    session_id: &str,
    take_id: &str,
) -> Result<(), EditorError> {
    drop(require_session(state, session_id)?);
    let slot = state.takes.get(session_id, take_id)?;
    let mut entry = lock_ignoring_poison(&slot.entry);
    if entry.state == TakeState::Finished {
        let in_use = require_session(state, session_id)?
            .get(session_id)
            .is_some_and(|s| asset_in_use(s.project(), take_id));
        if in_use {
            return Err(invalid(
                "This take is on the timeline. Remove its clips before discarding it.",
            ));
        }
    }
    if entry.state != TakeState::Finished {
        remove_owned(&slot.part());
    }
    entry.state = TakeState::Discarded;
    drop(entry);
    lock_ignoring_poison(&state.takes.0).remove(take_id);
    Ok(())
}

/// ASYNC: at most one unlink, off the main thread.
#[tauri::command]
pub async fn editor_webcam_discard(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    take_id: String,
) -> Result<(), EditorError> {
    require_editor_window(&window)?;
    blocking(move || discard_in(&app.state::<EditorState>(), &session_id, &take_id)).await
}

// ---- owned files ----------------------------------------------------------

/// `Ok` only for a plain file — checked no-follow, so a symlink wearing one
/// of a take's names is never written through.
fn require_owned_file(path: &Path) -> Result<(), EditorError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err(internal(
            "The take's file has been replaced; discard the take.",
        )),
        Err(e) => Err(write_error("reach the take's file", &e)),
    }
}

/// Remove one of a take's own files: a plain file only (a symlink or a
/// directory wearing the name is left alone), a missing one is fine, any
/// other failure is logged.
fn remove_owned(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {
            if let Err(e) = std::fs::remove_file(path) {
                log::warn!("webcam take: could not remove {}: {e}", path.display());
            }
        }
        Ok(_) => log::warn!(
            "webcam take: {} is not a plain file; left in place",
            path.display()
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("webcam take: cannot inspect {}: {e}", path.display()),
    }
}

fn write_error(what: &str, e: &io::Error) -> EditorError {
    #[cfg(windows)]
    let full = e.kind() == io::ErrorKind::StorageFull || e.raw_os_error() == Some(112);
    #[cfg(not(windows))]
    let full = e.kind() == io::ErrorKind::StorageFull;
    if full {
        err(
            EditorErrorCode::DiskFull,
            format!("Not enough disk space to {what}: {e}"),
        )
    } else {
        internal(format!("Could not {what}: {e}"))
    }
}

#[cfg(test)]
#[path = "webcam_commands_tests.rs"]
mod tests;
