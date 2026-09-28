//! Separate audio STEMS (Task 53, F-05): each selected audio input kept as
//! its own mono AAC file (`.<base>.stem-<n>.m4a.part` while recording,
//! `<base>.stem-<n>.m4a` once published with the capture) beside the MIXED
//! track the screen file carries.
//!
//! WHY THEY CANNOT DRIFT: a stem is not a second recording of its input. It
//! is TEED off the mixing round itself — the very post-resample slice the
//! mixer sums, stamped with the very `(ts, dur)` the mixed chunk gets from
//! the one `AudioPacer` — so a stem and the mix are two views of one sample
//! stream on one clock, by construction (`Mixdown::round`). Pause, the
//! pause-edge flush and a stalled source are therefore inherited, not
//! re-implemented: whatever the mix writes, every stem writes too, padded
//! with the silence the mixer pads a short source with.
//!
//! OFF BY DEFAULT (`screenAudioStems`). With no `StemParams` the session
//! builds `Mixdown::new(n, false)`, whose rounds carry no stems, constructs
//! no writer and spawns no thread — the mixed track is byte-identical either
//! way (`stems_default_off`).
//!
//! A STEM NEVER FAILS THE CAPTURE. A stem whose file cannot be opened or
//! written, or a writer that falls behind, raises `screen:warning` and is
//! dropped; the mixed track — which holds the same audio — is untouched. A
//! stem is kept only when it is COMPLETE, so every published stem spans the
//! whole mixed track and migration can place it exactly where the screen's
//! clips are.
//!
//! WHAT IS PURE AND WHAT IS NOT — `session/mod.rs`'s rule. Everything here
//! compiles and runs on every platform. `stems_windows.rs` owns the sinks
//! (`screen-stems`, the ONLY thread that touches them) and EXECUTES IN NO
//! AUTOMATED TEST; the tutorial editor's Windows checklist is its gate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use vault_buddy_capture::mixer;

use super::{Warnings, AUDIO_RATE};
use crate::sink::AudioFormat;
use crate::staging;

/// Mono: a stem is one input, already downmixed for the mixer.
pub const STEM_CHANNELS: u16 = 1;
/// Snaps to the AAC encoder's 12 000 B/s (`sink_format`'s legal set).
pub const STEM_BITRATE_BPS: u32 = 96_000;
/// Chunks in flight to the stem writer: ~5 s of 4096-frame rounds. A writer
/// that falls this far behind is abandoned rather than allowed to stall the
/// mixing thread.
pub const STEM_CHANNEL_DEPTH: usize = 64;

/// One stem the session is asked to write: input `index` (1-based, in the
/// order of `ScreenSessionParams::audio`), its device name, and its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemTarget {
    pub index: u32,
    pub input: String,
    /// `.<base>.stem-<index>.m4a.part` — hidden, owned by the recovery
    /// sweep's pattern.
    pub part: PathBuf,
    /// `<base>.stem-<index>.m4a`, where `part` is published.
    pub staged: PathBuf,
}

/// What a caller hands the session to record stems. `None` in
/// `ScreenSessionParams::stems` is today's capture, unchanged.
#[derive(Debug, Clone)]
pub struct StemParams {
    pub targets: Vec<StemTarget>,
}

/// A stem that finished COMPLETE and was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemOutcome {
    pub index: u32,
    pub input: String,
    pub file: PathBuf,
}

/// One target per input, named by `staging`'s stem shape — the ONLY names a
/// stem is ever written under, so the recovery sweep's pattern and
/// `staging_files::capture_file_names` own every file this mints.
pub fn stem_targets(dir: &Path, base: &str, inputs: &[String]) -> Vec<StemTarget> {
    inputs
        .iter()
        .zip(1u32..)
        .map(|(input, index)| StemTarget {
            index,
            input: input.clone(),
            part: dir.join(staging::stem_part_file_name(base, index)),
            staged: dir.join(staging::stem_file_name(base, index)),
        })
        .collect()
}

/// The session's stem request: `None` unless the vault keeps stems
/// (`screenAudioStems`, off by default) AND the capture has an audio input
/// to keep — so a default capture constructs no writer at all.
pub fn stem_params(enabled: bool, dir: &Path, base: &str, inputs: &[String]) -> Option<StemParams> {
    (enabled && !inputs.is_empty()).then(|| StemParams {
        targets: stem_targets(dir, base, inputs),
    })
}

/// The sidecar's `stems` block for the stems that were published: file
/// NAMES (never paths), derived from `base` exactly as they were written.
pub fn stem_sidecar_entries(base: &str, outcomes: &[StemOutcome]) -> Vec<staging::StemSidecar> {
    outcomes
        .iter()
        .map(|o| staging::StemSidecar {
            index: o.index,
            input: o.input.clone(),
            file: staging::stem_file_name(base, o.index),
            extra: serde_json::Map::new(),
        })
        .collect()
}

/// A stem's AAC format: the mixed track's rate, one channel.
pub fn stem_audio_format() -> AudioFormat {
    AudioFormat {
        sample_rate: AUDIO_RATE,
        channels: STEM_CHANNELS,
        bitrate_bps: STEM_BITRATE_BPS,
    }
}

/// One mixing round's stems, on the mixed chunk's own timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemChunk {
    /// One mono buffer per input, each exactly the round's frame count.
    pub pcm: Vec<Vec<i16>>,
    pub ts: Duration,
    pub dur: Duration,
}

/// What one mixing round produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    /// Interleaved stereo for the screen file — `mix_n_to_stereo_i16`.
    pub stereo: Vec<i16>,
    /// `Some` only when stems are on: each input's slice of THIS round.
    pub stems: Option<Vec<Vec<i16>>>,
}

/// The audio thread's per-source buffers and the ONE mixing step, pure so
/// that "the stems receive exactly what the mixer receives" is a property a
/// test can check rather than a hope about the Windows thread.
pub struct Mixdown {
    buffers: Vec<Vec<f32>>,
    stems: bool,
}

impl Mixdown {
    pub fn new(sources: usize, stems: bool) -> Mixdown {
        Mixdown {
            buffers: vec![Vec::new(); sources],
            stems,
        }
    }

    /// Downmix and resample one delivery from source `i` to `AUDIO_RATE`,
    /// then buffer it. The ONLY way samples enter — so the stems, which are
    /// cut from these buffers in `round`, are post-resample by construction.
    pub fn push(&mut self, i: usize, raw: &[f32], channels: u16, rate: u32) {
        let mono = mixer::downmix_to_mono(raw, channels);
        let at_rate = mixer::resample_linear(&mono, rate, AUDIO_RATE);
        if let Some(buffer) = self.buffers.get_mut(i) {
            buffer.extend_from_slice(&at_rate);
        }
    }

    pub fn lens(&self) -> Vec<usize> {
        self.buffers.iter().map(Vec::len).collect()
    }

    /// Mix the first `take` frames of every buffer and drain them. The stems
    /// are the SAME slices the mixer sums, each padded to `take` frames with
    /// the silence the mixer pads a short (stalled) source with.
    pub fn round(&mut self, take: usize) -> Round {
        let slices: Vec<&[f32]> = self
            .buffers
            .iter()
            .map(|b| &b[..take.min(b.len())])
            .collect();
        // No per-source gain normalisation (spec 6.5): dividing by N would
        // change existing two-source meeting levels.
        let stereo = mixer::mix_n_to_stereo_i16(&slices);
        let stems = self
            .stems
            .then(|| slices.iter().map(|s| stem_pcm(s, take)).collect());
        for b in &mut self.buffers {
            b.drain(..take.min(b.len()));
        }
        Round { stereo, stems }
    }

    /// Throw away everything buffered (audio captured DURING a pause).
    pub fn clear(&mut self) {
        for b in &mut self.buffers {
            b.clear();
        }
    }
}

/// A slice as the 16-bit mono PCM the stem's encoder takes, padded with
/// silence to `frames`. The conversion is the mixer's own
/// (`soft_clip(x) * i16::MAX`), so a one-input capture's stem carries the
/// same values as either channel of its mix.
pub fn stem_pcm(slice: &[f32], frames: usize) -> Vec<i16> {
    let mut pcm: Vec<i16> = slice
        .iter()
        .map(|&x| (mixer::soft_clip(x) * i16::MAX as f32) as i16)
        .collect();
    pcm.resize(frames.max(pcm.len()), 0);
    pcm
}

/// The audio thread's end of the stem channel.
///
/// NEVER blocks: a writer that fell behind (the channel is full) is
/// ABANDONED — its sender dropped, `abandoned` set so the writer discards
/// every stem as incomplete, one warning raised — because blocking here
/// would stall the mixed track, which is the recording.
pub struct StemTee {
    tx: Option<SyncSender<StemChunk>>,
    abandoned: Arc<AtomicBool>,
}

impl StemTee {
    pub fn new(tx: SyncSender<StemChunk>, abandoned: Arc<AtomicBool>) -> StemTee {
        StemTee {
            tx: Some(tx),
            abandoned,
        }
    }

    pub fn send(&mut self, chunk: StemChunk, warnings: &Warnings) {
        let Some(tx) = &self.tx else { return };
        match tx.try_send(chunk) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.abandoned.store(true, Ordering::Relaxed);
                self.tx = None;
                warnings.raise(STEMS_FELL_BEHIND.to_string());
            }
            // The writer ended on its own (it warned for itself).
            Err(TrySendError::Disconnected(_)) => self.tx = None,
        }
    }
}

pub const STEMS_FELL_BEHIND: &str = "The separate audio tracks could not keep up and were \
                                     stopped. The mixed recording is unaffected.";

/// The warning for one stem that could not be kept. It names the input,
/// never a path, and says what the user still has.
pub fn stem_failed_warning(input: &str) -> String {
    format!(
        "The separate track for \"{input}\" could not be saved. The mixed recording \
         still contains it."
    )
}

#[cfg(test)]
#[path = "stems_tests.rs"]
mod tests;
