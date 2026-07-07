//! Decode our MP3 recording into the exact PCM shape whisper.cpp expects:
//! 16 kHz, mono, f32 in [-1, 1]. Symphonia is pure Rust, so no ffmpeg
//! binary is bundled. Resampling is linear — adequate for 16 kHz speech and
//! fully testable; rubato is a future quality upgrade.

use crate::CancelToken;
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const WHISPER_RATE: u32 = 16_000;

pub fn decode_to_16k_mono(path: &Path, cancel: &CancelToken) -> Result<Vec<f32>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("probe audio: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "no audio track in recording".to_string())?;
    let track_id = track.id;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| "no audio codec parameters in recording".to_string())?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .map_err(|e| format!("init decoder: {e}"))?;

    // Resample each packet straight into the 16 kHz output as it decodes, so we
    // never hold the full source-rate PCM at once: hours of 48 kHz audio is
    // gigabytes of source samples, but the 16 kHz result plus one packet's
    // scratch is bounded and small. The resampler is built lazily from the
    // FIRST packet's rate — our captures use one constant rate for the file.
    let mut out: Vec<f32> = Vec::new();
    let mut resampler: Option<StreamingLinearResampler> = None;
    let mut interleaved: Vec<f32> = Vec::new(); // per-packet decode scratch, reused
    let mut mono: Vec<f32> = Vec::new(); // per-packet downmix scratch, reused
    let mut warned_rate_change = false; // latch: warn once, never per packet
    loop {
        // Decoding a long recording is many seconds of uninterruptible work
        // if we don't look here — the whisper abort callback only covers
        // inference, not this pre-inference decode. Check once per packet
        // (thousands of them for a real recording) so a cancel lands
        // promptly instead of waiting out the whole file. The caller
        // disambiguates this Err from a real decode failure via the token.
        if cancel.is_cancelled() {
            return Err("cancelled".to_string());
        }
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("read packet: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                let channels = spec.channels().count().max(1);
                // A mid-stream rate change would need its own resampler to be
                // correct; it must not happen for our recordings. If it does we
                // keep feeding the original-rate resampler (no worse than the
                // old code, which resampled the whole file at the LAST packet's
                // rate) and leave an honest breadcrumb rather than silently
                // emitting wrong-pitch audio.
                if let Some(r) = resampler.as_ref() {
                    // Once, not per packet: a genuinely variable-rate stream
                    // would otherwise flood the log with a warning per frame.
                    if spec.rate() != r.from && !warned_rate_change {
                        warned_rate_change = true;
                        log::warn!(
                            "unexpected sample-rate change decoding {}: {} -> {}; keeping {}",
                            path.display(),
                            r.from,
                            spec.rate(),
                            r.from
                        );
                    }
                }
                let rate = spec.rate();
                let r = resampler
                    .get_or_insert_with(|| StreamingLinearResampler::new(rate, WHISPER_RATE));
                decoded.copy_to_vec_interleaved(&mut interleaved);
                mono.clear();
                for frame in interleaved.chunks(channels) {
                    let sum: f32 = frame.iter().copied().sum();
                    mono.push(sum / channels as f32);
                }
                r.push(&mono, &mut out);
            }
            // One corrupt frame must not abandon a whole recording.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(format!("decode audio: {e}")),
        }
    }
    // Flush the resampler's trailing sample so the length tracks input×ratio. No
    // packets decoded (empty or all-corrupt file) leaves `out` empty, as before.
    if let Some(mut r) = resampler {
        r.finish(&mut out);
    }
    Ok(out)
}

/// Linear resampler that runs incrementally — one `push` of decoded samples at
/// a time — so a caller never has to materialise the whole source-rate signal.
/// Its defining property: the output for a given input stream is identical no
/// matter how that stream is split across `push` calls. The single carried
/// sample (`prev`) plus counting outputs against the running input total is
/// what makes chunk boundaries seamless.
pub(crate) struct StreamingLinearResampler {
    from: u32,
    to: u32,
    in_count: u64,  // total input samples seen across all pushes
    out_count: u64, // total output samples emitted so far
    // last input sample seen; the LEFT neighbour carried into the next chunk so
    // a boundary output can still interpolate across the seam
    prev: f32,
}

impl StreamingLinearResampler {
    pub(crate) fn new(from: u32, to: u32) -> Self {
        Self {
            from,
            to,
            in_count: 0,
            out_count: 0,
            prev: 0.0,
        }
    }

    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        // Matched rate (or an unusable 0) is a bit-exact pass-through: no
        // resample math, and keeping the counters coherent makes finish() a
        // no-op.
        if self.from == self.to || self.from == 0 {
            out.extend_from_slice(input);
            self.in_count += input.len() as u64;
            self.out_count = self.in_count;
            self.prev = *input.last().unwrap();
            return;
        }
        let base = self.in_count; // global index of input[0] in this chunk
        self.in_count += input.len() as u64;
        let (from, to) = (self.from as u64, self.to as u64);
        // Output i samples the source at pos = i*from/to; its RIGHT neighbour is
        // input[floor(pos)+1], which only exists once pos < in_count-1, i.e.
        // i*from < (in_count-1)*to. So we emit up to that bound and carry the
        // chunk's last sample as the LEFT neighbour of the next chunk's first
        // output. The emitted set depends only on the running total (never on
        // where a chunk edge fell) and each value depends only on fixed input
        // positions, so the concatenated output is chunk-size invariant.
        let prev = self.prev;
        let limit = (self.in_count - 1) * to;
        while self.out_count * from < limit {
            let pos = self.out_count as f64 * self.from as f64 / self.to as f64;
            let left = pos.floor() as u64; // >= base-1 by construction
            let frac = (pos - left as f64) as f32;
            // left is either base-1 (the carried sample) or an index within
            // this chunk; left+1 is always within this chunk given the bound.
            let a = if left >= base {
                input[(left - base) as usize]
            } else {
                prev
            };
            let b = input[(left + 1 - base) as usize];
            out.push(a + (b - a) * frac);
            self.out_count += 1;
        }
        self.prev = *input.last().unwrap();
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<f32>) {
        // Pass-through already emitted everything; nothing decoded => nothing.
        if self.from == self.to || self.from == 0 || self.in_count == 0 {
            return;
        }
        // The tail outputs land at or past the last input sample, where linear
        // interpolation clamps to it, so top up with that final sample. Target
        // AT LEAST round(input×ratio): when downsampling, `push` may already
        // have emitted one more tail sample than that (it legitimately
        // interpolates the last output from two real samples), and we keep it —
        // a ±1-sample delta from a one-shot batch resample, inaudible and fully
        // chunk-invariant (the loop below is then simply a no-op).
        let ratio = self.to as f64 / self.from as f64;
        let out_len = (self.in_count as f64 * ratio).round() as u64;
        while self.out_count < out_len {
            out.push(self.prev);
            self.out_count += 1;
        }
    }
}

/// Batch linear resample — a thin wrapper over the streaming resampler. The
/// decode path resamples incrementally, so this exists only as a test seam that
/// exercises the exact same algorithm one-shot (`#[cfg(test)]` keeps it out of
/// production builds where nothing calls it).
#[cfg(test)]
pub(crate) fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let mut out = Vec::new();
    let mut r = StreamingLinearResampler::new(from, to);
    r.push(input, &mut out);
    r.finish(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm};

    fn make_mp3(rate: u32, secs: f32) -> Vec<u8> {
        let frames = (rate as f32 * secs) as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16;
            pcm.push(s);
            pcm.push(s);
        }
        let mut b = Builder::new().unwrap();
        b.set_num_channels(2).unwrap();
        b.set_sample_rate(rate).unwrap();
        b.set_brate(Bitrate::Kbps128).unwrap();
        b.set_quality(mp3lame_encoder::Quality::Good).unwrap();
        let mut enc = b.build().unwrap();
        // encode_to_vec/flush_to_vec only ever write into the Vec's existing
        // *spare* capacity — they never grow it themselves. An unreserved
        // `Vec::new()` hands LAME a zero-length buffer and it writes out of
        // bounds anyway (SIGSEGV), so reserve first, exactly as
        // `capture::encoder::Mp3Encoder` already does for this same crate.
        let mut out = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(frames));
        enc.encode_to_vec(InterleavedPcm(&pcm[..]), &mut out)
            .unwrap();
        out.reserve(7200);
        enc.flush_to_vec::<FlushNoGap>(&mut out).unwrap();
        out
    }

    #[test]
    fn decodes_mp3_to_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, make_mp3(44_100, 1.0)).unwrap();
        let pcm = decode_to_16k_mono(&path, &crate::CancelToken::new()).unwrap();
        let secs = pcm.len() as f32 / WHISPER_RATE as f32;
        assert!(
            (secs - 1.0).abs() < 0.25,
            "expected ~1s, got {secs}s ({} samples)",
            pcm.len()
        );
        assert!(
            pcm.iter().any(|&s| s.abs() > 0.01),
            "decoded audio is not silent"
        );
    }

    #[test]
    fn precancelled_decode_bails_instead_of_running_to_completion() {
        // Regression: cancelling a transcription used to wait out the entire
        // MP3 decode (uninterruptible) before the single post-decode check
        // noticed. A pre-cancelled token must abort inside the packet loop —
        // the same valid MP3 that `decodes_mp3_to_16k_mono` decodes in full
        // returns Err here purely because the token is set.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, make_mp3(44_100, 1.0)).unwrap();
        let cancel = crate::CancelToken::new();
        cancel.cancel();
        assert!(
            decode_to_16k_mono(&path, &cancel).is_err(),
            "a pre-cancelled decode must not run the whole file to completion"
        );
    }

    #[test]
    fn resample_preserves_duration_ratio() {
        let input: Vec<f32> = (0..44_100).map(|i| (i as f32 / 100.0).sin()).collect();
        let out = resample_linear(&input, 44_100, 16_000);
        let ratio = out.len() as f32 / input.len() as f32;
        assert!((ratio - 16_000.0 / 44_100.0).abs() < 0.01, "ratio {ratio}");
    }

    #[test]
    fn resample_is_identity_when_rates_match() {
        let input = vec![0.1f32, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn resample_handles_empty_input() {
        assert!(resample_linear(&[], 44_100, 16_000).is_empty());
    }

    fn feed_in_chunks(input: &[f32], from: u32, to: u32, chunk: usize) -> Vec<f32> {
        let mut r = StreamingLinearResampler::new(from, to);
        let mut out = Vec::new();
        if chunk == 0 {
            r.push(input, &mut out);
        } else {
            for c in input.chunks(chunk) {
                r.push(c, &mut out);
            }
        }
        r.finish(&mut out);
        out
    }

    #[test]
    fn streaming_resample_is_chunk_size_invariant() {
        // The whole reason the resampler is streaming: the output must not
        // depend on how the input is split across push() calls. A wrong
        // boundary carry makes some chunkings drift in value or length. Use a
        // signal with both curvature (sine) and slope (ramp) at the real
        // 44.1k -> 16k downsample ratio, and compare every chunking against the
        // all-at-once reference byte-for-byte (tiny float epsilon).
        let input: Vec<f32> = (0..500)
            .map(|i| (i as f32 * 0.11).sin() * 0.7 + i as f32 * 0.001)
            .collect();
        let reference = feed_in_chunks(&input, 44_100, 16_000, 0);
        // all-at-once, 1-at-a-time, 3, 128, and uneven remainders (7, 333)
        for chunk in [1usize, 3, 7, 128, 333] {
            let got = feed_in_chunks(&input, 44_100, 16_000, chunk);
            assert_eq!(
                got.len(),
                reference.len(),
                "chunk size {chunk} changed output length"
            );
            for (k, (a, b)) in reference.iter().zip(&got).enumerate() {
                assert!(
                    (a - b).abs() < 1e-6,
                    "chunk size {chunk} diverged at sample {k}: {a} vs {b}"
                );
            }
        }
    }

    #[test]
    fn streaming_resample_of_ramp_matches_analytic_interpolation() {
        // A linear ramp must resample to the exact analytic line: output i sits
        // at pos = i*from/to and equals slope*pos + intercept. This pins the
        // interpolation to true linear — a wrong frac or carry shows up here.
        let (from, to) = (8_000u32, 16_000u32);
        let (slope, intercept) = (0.25f32, -3.0f32);
        let input: Vec<f32> = (0..200).map(|i| slope * i as f32 + intercept).collect();
        let out = resample_linear(&input, from, to);
        let last = (input.len() - 1) as f64;
        for (i, &v) in out.iter().enumerate() {
            // The tail clamps to the last input sample (no data past the end).
            let pos = (i as f64 * from as f64 / to as f64).min(last);
            let expected = slope * pos as f32 + intercept;
            assert!(
                (v - expected).abs() < 1e-4,
                "ramp mismatch at {i}: got {v}, want {expected}"
            );
        }
    }

    #[test]
    fn streaming_resample_passthrough_and_empty() {
        // Matched rates are a bit-exact identity (no resample math at all);
        // empty input yields nothing through the streaming API.
        let input = vec![0.1f32, -0.2, 0.3, 0.9];
        let mut out = Vec::new();
        let mut r = StreamingLinearResampler::new(22_050, 22_050);
        r.push(&input, &mut out);
        r.finish(&mut out);
        assert_eq!(out, input);

        let mut empty = Vec::new();
        let mut r2 = StreamingLinearResampler::new(44_100, 16_000);
        r2.push(&[], &mut empty);
        r2.finish(&mut empty);
        assert!(empty.is_empty());
    }
}
