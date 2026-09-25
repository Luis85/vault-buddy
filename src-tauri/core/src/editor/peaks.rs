//! Waveform peaks (Task 28; F-26; ADR §3 `peaks.rs`): a STREAMING fold of
//! signed 16-bit little-endian mono PCM into a fixed number of max-abs
//! buckets, each in `0.0..=1.0`.
//!
//! The shell pipes `ffmpeg -ac 1 -ar 8000 -f s16le -` through this fold a
//! pipe-read at a time, so two properties are load-bearing and pinned by
//! the tests below rather than left to the caller:
//! - **Memory is bounded by the bucket count, never by the asset's
//!   length.** The only state is `Vec<f32>` of at most `MAX_BUCKETS`
//!   entries plus a few counters; a two-hour recording folds in the same
//!   footprint as a two-second one.
//! - **The answer does not depend on where the pipe happened to split.** A
//!   read can end in the MIDDLE of a sample (an odd byte count); that byte
//!   is carried into the next chunk instead of being dropped or read as a
//!   sample of its own, which would shift every later sample by one byte
//!   and turn the whole rest of the waveform into noise.
//!
//! Normalisation is against FULL SCALE (`|sample| / 32768`), not against
//! the loudest sample: a quiet recording looks quiet, which is the honest
//! editing guidance (R20) — a per-asset normalisation would draw a whisper
//! and a shout at the same height.

/// The most buckets one waveform may have (the state's memory bound).
pub const MAX_BUCKETS: usize = 4000;

/// The sample rate the shell asks ffmpeg to resample to (`-ar 8000`) —
/// ample for a drawn envelope, and 1/6 of the bytes of 48 kHz.
pub const PEAK_SAMPLE_RATE: u64 = 8000;

/// How many mono samples `duration_ms` of audio yields at
/// `PEAK_SAMPLE_RATE` — what `PeakState::new` spreads over its buckets.
pub fn expected_samples(duration_ms: u64) -> u64 {
    duration_ms.saturating_mul(PEAK_SAMPLE_RATE) / 1000
}

/// The fold's whole state — see the module doc for why it is this small.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakState {
    peaks: Vec<f32>,
    samples_per_bucket: u64,
    next_sample: u64,
    /// The low byte of a sample whose high byte has not arrived yet.
    carry: Option<u8>,
}

impl PeakState {
    /// `buckets` is clamped to `1..=MAX_BUCKETS`; `expected_samples` (the
    /// asset's length in samples) decides how many samples each bucket
    /// covers. A stream that runs LONGER than expected (a container whose
    /// declared duration undershoots) folds its tail into the last bucket
    /// rather than growing the state.
    pub fn new(buckets: usize, expected_samples: u64) -> Self {
        let buckets = buckets.clamp(1, MAX_BUCKETS);
        let per = expected_samples.div_ceil(buckets as u64).max(1);
        Self {
            peaks: vec![0.0; buckets],
            samples_per_bucket: per,
            next_sample: 0,
            carry: None,
        }
    }

    /// How many buckets this state folds into.
    pub fn buckets(&self) -> usize {
        self.peaks.len()
    }

    /// The finished peaks. A byte still carried at the end is half a
    /// sample the stream never completed — dropped, never read as one.
    pub fn finish(self) -> Vec<f32> {
        self.peaks
    }

    fn push(&mut self, sample: i16) {
        let last = self.peaks.len() - 1;
        let index = usize::try_from(self.next_sample / self.samples_per_bucket)
            .unwrap_or(last)
            .min(last);
        let magnitude = f32::from(sample).abs() / 32768.0;
        if magnitude > self.peaks[index] {
            self.peaks[index] = magnitude;
        }
        self.next_sample = self.next_sample.saturating_add(1);
    }
}

/// Fold one chunk of s16le bytes into `state`. Any chunk boundary gives
/// the same result as folding the concatenation in one go.
pub fn fold_s16le(chunk: &[u8], state: &mut PeakState) {
    let mut rest = chunk;
    if let Some(low) = state.carry.take() {
        let Some((&high, tail)) = rest.split_first() else {
            state.carry = Some(low);
            return;
        };
        state.push(i16::from_le_bytes([low, high]));
        rest = tail;
    }
    let mut pairs = rest.chunks_exact(2);
    for pair in &mut pairs {
        state.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    if let [odd] = pairs.remainder() {
        state.carry = Some(*odd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    fn fold_all(chunks: &[&[u8]], buckets: usize, expected: u64) -> Vec<f32> {
        let mut state = PeakState::new(buckets, expected);
        for chunk in chunks {
            fold_s16le(chunk, &mut state);
        }
        state.finish()
    }

    // This task's named RED case (and its mutation check: drop the carry).
    // A pipe read can end mid-sample; the same 18 bytes split at EVERY
    // offset must fold to exactly what one unsplit read gives. The samples
    // are asymmetric (distinct magnitudes, both signs, high AND low bytes
    // nonzero) so a byte slip reads different values instead of the same.
    #[test]
    fn peaks_fold_is_chunk_boundary_independent() {
        let data = bytes(&[1200, -300, 7000, -16384, 250, 9001, -32768, 4097, 12345]);
        assert_eq!(data.len(), 18);
        let whole = fold_all(&[&data], 3, 9);
        assert_eq!(whole.len(), 3);
        for split in 0..=17 {
            let (a, b) = data.split_at(split);
            assert_eq!(fold_all(&[a, b], 3, 9), whole, "split at {split}");
        }
        // Three reads, both cuts odd: two carries in a row.
        assert_eq!(
            fold_all(&[&data[..3], &data[3..9], &data[9..]], 3, 9),
            whole
        );
        // One byte per read: a carry on every other chunk.
        let singles: Vec<&[u8]> = data.chunks(1).collect();
        assert_eq!(fold_all(&singles, 3, 9), whole);
    }

    // Each bucket is the max of |sample| over its span, against full scale:
    // a negative sample counts by magnitude, -32768 is exactly 1.0, and a
    // quiet bucket stays quiet (no per-asset normalisation).
    #[test]
    fn peaks_are_normalised_max_abs() {
        let data = bytes(&[
            1000, -16384, 8192, // bucket 0: max |.| = 16384 -> 0.5
            32767, -32768, 0, // bucket 1: -32768 -> 1.0
            -4096, 2048, -1024, // bucket 2: 4096 -> 0.125
            0, 0, 0, // bucket 3: silence
        ]);
        let peaks = fold_all(&[&data], 4, 12);
        assert_eq!(peaks, vec![0.5, 1.0, 0.125, 0.0]);
        assert!(peaks.iter().all(|p| (0.0..=1.0).contains(p)));
    }

    // The low byte of a split sample waits for its high byte; a byte that
    // never gets one (the stream ended mid-sample) is dropped at `finish`,
    // never read as a phantom sample.
    #[test]
    fn odd_trailing_byte_is_carried() {
        let mut state = PeakState::new(2, 2);
        fold_s16le(&[0x00], &mut state);
        assert_eq!(
            state.clone().finish(),
            vec![0.0, 0.0],
            "half a sample is not one"
        );
        fold_s16le(&[0x40], &mut state); // 0x4000 = 16384 -> 0.5
        assert_eq!(state.clone().finish(), vec![0.5, 0.0]);
        fold_s16le(&[], &mut state); // an empty read changes nothing
        fold_s16le(&[0x00, 0x60, 0x7f], &mut state); // 0x6000 = 24576 -> 0.75, then a dangling 0x7f
        assert_eq!(state.finish(), vec![0.5, 0.75]);
    }

    // The memory bound: the bucket count is clamped to 1..=MAX_BUCKETS
    // whatever the caller asks for, and a stream longer than expected
    // folds into the last bucket rather than growing the Vec.
    #[test]
    fn buckets_are_bounded() {
        assert_eq!(PeakState::new(1_000_000, 10).buckets(), MAX_BUCKETS);
        assert_eq!(PeakState::new(0, 10).buckets(), 1);
        let mut state = PeakState::new(2, 4);
        fold_s16le(&bytes(&[100, 200, 300, 400, 8192, -16384, 1]), &mut state);
        let peaks = state.finish();
        assert_eq!(peaks.len(), 2);
        assert_eq!(
            peaks[1], 0.5,
            "the overflowing tail landed in the last bucket"
        );
        // Fewer samples than buckets never divides by zero.
        let mut tiny = PeakState::new(8, 0);
        fold_s16le(&bytes(&[16384]), &mut tiny);
        assert_eq!(tiny.finish()[0], 0.5);
        assert_eq!(expected_samples(1_500), 12_000);
        assert_eq!(expected_samples(u64::MAX), u64::MAX / 1000);
    }
}
