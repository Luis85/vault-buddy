//! A webcam take's finish half, split out of `webcam_commands.rs` (hardening
//! Task 9) when crash recovery (GAP-197, `webcam_recover.rs`) needed the
//! same land + register path: the remux `-c copy` into `takes\<takeId>.webm`,
//! the probe and hash, the `sources.json` record and ONE `AddAssets` — or,
//! without ffmpeg, the raw recording kept as-is (A09).
//!
//! **Locks** — take entry, then save lock, then `sessions`
//! (`webcam_registry.rs`).

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::Path;

use serde::Serialize;
use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::commands::payloads::AddAssetsPayload;
use vault_buddy_core::editor::import_io::copy_hashing;
use vault_buddy_core::editor::probe::ProbeFacts;
use vault_buddy_core::editor::take::{next_take_ordinal, recovered_take_asset, take_asset};
use vault_buddy_core::editor::{Asset, EditorError, EditorErrorCode, InternalCommand};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::require_session;
use super::media_probe::probe_media;
use super::project_store::{SourceLocator, SourceMediaKind, SourceRecord};
use super::redact::redact_path;
use super::save_commands::session_save_lock;
use super::store_io::{load_sources, write_sources};
use super::webcam_commands::{err, internal, write_error};
use super::webcam_registry::{remove_owned, TakeSlot};
use super::EditorState;
use crate::ffmpeg::{resolve_working_ffmpeg, FfmpegTools};

/// Wall-clock bound on the finish remux. A `-c copy` of even a 4 GiB take
/// is a disk-speed copy; this exists only so a wedged ffmpeg cannot hold
/// the take (and its session's finish) forever.
const REMUX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

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
        // ffmpeg's stderr names both files (final review M8).
        let detail = super::redact::redact_paths_in(stderr.trim(), &[part, out]);
        log::warn!("webcam take remux failed: {detail}");
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
    super::discard::refuse_if_closing(state, session_id)?;
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

/// How a take landed.
pub(crate) enum Landed {
    Indexed(TakeDto),
    /// No ffmpeg: the raw recording was kept and registered as this asset.
    Raw(String),
}

/// Land `slot`'s `.part` as `takes\<takeId>.webm` and register it — the
/// remux-probe-hash-rename, or without ffmpeg the raw keep (A09). Shared by
/// a finish and by crash recovery (`webcam_recover`, whose slot is marked
/// `recovered` so `register` names it "Webcam take N (recovered)"). The
/// caller holds the slot's entry lock.
pub(crate) fn land(
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
                log::warn!(
                    "webcam take: could not restore {}: {back}",
                    redact_path(&part)
                );
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
    // names the file by its role and a `redact_path` handle (Task 58), and
    // the log line keeps that handle, never the path itself.
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
        let asset = if slot.recovered {
            recovered_take_asset(asset_id.clone(), ordinal, facts, size)
        } else {
            take_asset(asset_id.clone(), ordinal, facts, size)
        };
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
