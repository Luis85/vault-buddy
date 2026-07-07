//! Decode our MP3 recording into the exact PCM shape whisper.cpp expects:
//! 16 kHz, mono, f32 in [-1, 1]. Symphonia is pure Rust, so no ffmpeg
//! binary is bundled. Resampling is linear — adequate for 16 kHz speech and
//! fully testable; rubato is a future quality upgrade.

use crate::CancelToken;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const WHISPER_RATE: u32 = 16_000;

pub fn decode_to_16k_mono(path: &Path, cancel: &CancelToken) -> Result<Vec<f32>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe audio: {e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "no audio track in recording".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("init decoder: {e}"))?;

    let mut src_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut mono: Vec<f32> = Vec::new();
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
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("read packet: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                src_rate = spec.rate;
                let channels = spec.channels.count().max(1);
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                for frame in buf.samples().chunks(channels) {
                    let sum: f32 = frame.iter().copied().sum();
                    mono.push(sum / channels as f32);
                }
            }
            // One corrupt frame must not abandon a whole recording.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(format!("decode audio: {e}")),
        }
    }
    Ok(resample_linear(&mono, src_rate, WHISPER_RATE))
}

pub(crate) fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 || from == to {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = input.len() - 1;
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input[idx.min(last)];
        let b = input[(idx + 1).min(last)];
        out.push(a + (b - a) * frac);
    }
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
}
