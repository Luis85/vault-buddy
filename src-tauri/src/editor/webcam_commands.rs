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
//!   the answer is `encoderUnavailable` carrying its asset id. That half —
//!   `finish_in`, the remux/probe seam `TakeIo` and the land + register it
//!   shares with crash recovery — lives in `webcam_finish.rs`.
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
//! **Locks** — the registry and its lock order live in `webcam_registry.rs`
//! (take entry, then save lock, then `sessions`).
//!
//! Every `#[tauri::command]` here takes `window: WebviewWindow` and calls
//! `authz::require_editor_window(&window)?` FIRST (`authz_guard.rs`).

use std::fs::OpenOptions;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::editor::take::{
    asset_in_use, is_take_mime, parse_seq, TakeError, TakeState, MAX_TAKE_CHUNK_BYTES,
    TAKE_ID_PREFIX, TAKE_MIME_TYPES,
};
use vault_buddy_core::editor::{is_valid_id, new_entity_id, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::{require_editor_window, require_session};
use super::prefs_commands::{blocking, local_data, project_id_for};
use super::project_store::project_dir;
use super::webcam_finish::{finish_in, FfmpegTakeIo, TakeDto, TakeIo};
use super::webcam_registry::{remove_owned, TakeEntry, TakeSlot};
use super::EditorState;
use crate::capture_guard::{CaptureGuard, CaptureKind};

/// The three headers an append carries.
pub(crate) const HEADER_SESSION: &str = "x-editor-session";
pub(crate) const HEADER_TAKE: &str = "x-editor-take";
pub(crate) const HEADER_SEQ: &str = "x-editor-seq";

pub(super) fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn invalid(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::InvalidRequest, message)
}

pub(super) fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

/// `editor_webcam_begin`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeStarted {
    pub take_id: String,
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
    let take_id = new_entity_id(TAKE_ID_PREFIX);
    let slot = Arc::new(TakeSlot {
        session_id: session_id.to_string(),
        dir: project_dir(root, &project_id)
            .ok_or_else(|| internal("The project id is not valid."))?
            .join("takes"),
        project_id,
        take_id: take_id.clone(),
        recovered: false,
        entry: Mutex::new(TakeEntry {
            state: TakeState::new(),
            failed: None,
        }),
    });
    // Registered (entry held) BEFORE anything is created, and the closing
    // mark checked AFTER — so a discard's quiesce either waits for this
    // begin or this begin refuses (final review C2, `discard.rs`).
    let entry = lock_ignoring_poison(&slot.entry);
    lock_ignoring_poison(&state.takes.0).insert(take_id.clone(), Arc::clone(&slot));
    let created = super::discard::refuse_if_closing(state, session_id).and_then(|()| {
        takes_dir(root, &slot.project_id)?;
        // Exclusive: a stranger's file wearing this name is never appended to.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(slot.part())
            .map(drop)
            .map_err(|e| write_error("start the take", &e))
    });
    drop(entry);
    if let Err(e) = created {
        lock_ignoring_poison(&state.takes.0).remove(&take_id);
        return Err(e);
    }
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
/// body or why there is none — an error in the chunk or its write marks the
/// take failed; a refusal because the session is closing does NOT (GAP-214
/// item 6), since that discard may yet be refused and the chunk retried.
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
    // Under the entry lock (final review C2): a discard's quiesce waits for
    // a write already here; a later one refuses — BEFORE the error path
    // below, so the refusal never fails the take (GAP-214 item 6): the
    // discard may yet be refused, and the same chunk is then retried.
    super::discard::refuse_if_closing(state, &at.session_id)?;
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

pub(super) fn write_error(what: &str, e: &io::Error) -> EditorError {
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
