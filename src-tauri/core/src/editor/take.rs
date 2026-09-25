//! A webcam take's pure rules (Task 49, F-20; ADR R10): the chunk-sequence
//! state machine `editor_webcam_append`/`editor_webcam_finish` enforce, the
//! `MediaRecorder` MIME types a take may declare, and the asset a finished
//! take becomes.
//!
//! The webview's `MediaRecorder` hands the shell one chunk per timeslice as
//! a RAW invoke body, numbered from 0. The file those chunks build is only a
//! valid WebM if every chunk lands exactly once, in order — a lost or
//! repeated chunk corrupts every cluster after it with no error anywhere —
//! so `TakeState::accept_chunk` admits ONLY the next sequence number, never
//! "anything at or after it", and `finish` admits only the sequence of the
//! last chunk actually accepted. Sizes are bounded per chunk (1 MiB — the
//! ADR's chunk ceiling) and per take (4 GiB), before a byte is written.
//!
//! A take is an INDEPENDENT asset (A08): `take_asset` builds a new video
//! asset; nothing here touches the staged capture's own asset or clips.

use super::model::{Asset, Project};
use super::probe::{asset_from_probe, ImportKind, ProbeFacts};
use super::{EditorError, EditorErrorCode};

/// The `MediaRecorder` MIME types a take may be recorded in — WebM only,
/// because the finish step remuxes WebM to WebM (`-c copy`).
pub const TAKE_MIME_TYPES: [&str; 3] = [
    "video/webm;codecs=vp8,opus",
    "video/webm;codecs=vp9,opus",
    "video/webm",
];

/// The largest chunk one append may carry.
pub const MAX_TAKE_CHUNK_BYTES: u64 = 1024 * 1024;

/// The largest a whole take may grow.
pub const MAX_TAKE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// The prefix of every take id (and of the asset id a take registers as).
pub const TAKE_ID_PREFIX: &str = "take";

/// Is `mime` one of `TAKE_MIME_TYPES`, exactly?
pub fn is_take_mime(mime: &str) -> bool {
    TAKE_MIME_TYPES.contains(&mime)
}

/// Where a take is in its life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeState {
    /// Chunks `0..next_seq` have been accepted, `bytes` in total.
    Recording {
        next_seq: u64,
        bytes: u64,
    },
    Finished,
    Discarded,
}

/// Why a chunk or a finish was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeError {
    /// A chunk that is not the next one — a gap, a repeat, or a reorder.
    OutOfSequence {
        expected: u64,
        got: u64,
    },
    ChunkTooLarge {
        len: u64,
    },
    TakeTooLarge {
        total: u64,
    },
    /// A finish that names a chunk other than the last one accepted
    /// (`expected` is `None` when no chunk was ever accepted).
    WrongLastSequence {
        expected: Option<u64>,
        got: u64,
    },
    /// The take is already finished or discarded.
    NotRecording,
}

impl TakeError {
    pub fn message(&self) -> String {
        match self {
            TakeError::OutOfSequence { expected, got } => {
                format!("Webcam chunk {got} arrived out of order; chunk {expected} was expected.")
            }
            TakeError::ChunkTooLarge { len } => format!(
                "A webcam chunk of {len} bytes is larger than the {MAX_TAKE_CHUNK_BYTES}-byte limit."
            ),
            TakeError::TakeTooLarge { total } => format!(
                "The take would grow to {total} bytes, past the {MAX_TAKE_BYTES}-byte limit."
            ),
            TakeError::WrongLastSequence {
                expected: Some(expected),
                got,
            } => format!(
                "The take ends at chunk {expected}, not {got}; it cannot be finished there."
            ),
            TakeError::WrongLastSequence {
                expected: None,
                got,
            } => format!("The take has no chunks yet, so it cannot end at chunk {got}."),
            TakeError::NotRecording => "This take is no longer recording.".to_string(),
        }
    }
}

impl From<TakeError> for EditorError {
    fn from(e: TakeError) -> Self {
        EditorError::new(EditorErrorCode::InvalidRequest, e.message())
    }
}

impl Default for TakeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TakeState {
    /// A take with nothing accepted yet.
    pub fn new() -> Self {
        TakeState::Recording {
            next_seq: 0,
            bytes: 0,
        }
    }

    /// Admit chunk `seq` of `len` bytes: it must be exactly the next one,
    /// at most `MAX_TAKE_CHUNK_BYTES`, and keep the take within
    /// `MAX_TAKE_BYTES`. A refusal changes nothing.
    pub fn accept_chunk(&mut self, seq: u64, len: u64) -> Result<(), TakeError> {
        let TakeState::Recording { next_seq, bytes } = self else {
            return Err(TakeError::NotRecording);
        };
        if seq != *next_seq {
            return Err(TakeError::OutOfSequence {
                expected: *next_seq,
                got: seq,
            });
        }
        if len > MAX_TAKE_CHUNK_BYTES {
            return Err(TakeError::ChunkTooLarge { len });
        }
        let total = bytes.saturating_add(len);
        if total > MAX_TAKE_BYTES {
            return Err(TakeError::TakeTooLarge { total });
        }
        *next_seq += 1;
        *bytes = total;
        Ok(())
    }

    /// End the take at `last_seq`, which must be the last chunk accepted —
    /// so a take whose final chunk never arrived is refused rather than
    /// finished short.
    pub fn finish(&mut self, last_seq: u64) -> Result<(), TakeError> {
        let TakeState::Recording { next_seq, .. } = self else {
            return Err(TakeError::NotRecording);
        };
        if last_seq.checked_add(1) != Some(*next_seq) {
            return Err(TakeError::WrongLastSequence {
                expected: next_seq.checked_sub(1),
                got: last_seq,
            });
        }
        *self = TakeState::Finished;
        Ok(())
    }

    /// The bytes accepted so far — `None` once the take is not recording.
    pub fn bytes(&self) -> Option<u64> {
        match self {
            TakeState::Recording { bytes, .. } => Some(*bytes),
            _ => None,
        }
    }
}

/// An `x-editor-seq` header value: a canonical decimal `u64` — digits only,
/// no sign, no whitespace, no leading zero — so two spellings can never
/// name one chunk.
pub fn parse_seq(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

/// The ordinal the next take's name carries: one past the takes the
/// project already holds.
pub fn next_take_ordinal(project: &Project) -> usize {
    let prefix = format!("{TAKE_ID_PREFIX}-");
    1 + project
        .assets
        .iter()
        .filter(|a| a.id.starts_with(&prefix))
        .count()
}

/// The asset a finished take registers as: a NEW video asset named
/// "Webcam take N" (A08 — a take is never folded into the capture).
pub fn take_asset(asset_id: String, ordinal: usize, facts: ProbeFacts, size: u64) -> Asset {
    let mut asset = asset_from_probe(
        asset_id,
        format!("Webcam take {ordinal}"),
        ImportKind::Video,
        facts,
        None,
    );
    asset.size = Some(size);
    asset
}

/// Does any clip play `asset_id`? A take in use cannot be discarded.
pub fn asset_in_use(project: &Project, asset_id: &str) -> bool {
    project.clips.iter().any(|c| c.asset_id == asset_id)
}

#[cfg(test)]
#[path = "take_tests.rs"]
mod tests;
