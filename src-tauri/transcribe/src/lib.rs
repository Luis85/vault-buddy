//! Local speech-to-text: decode our MP3 to 16 kHz mono PCM and run
//! whisper.cpp (via whisper-rs, behind the `whisper` feature) behind a
//! `Transcriber` trait so orchestration is testable without a real model.

pub mod decode;
pub mod model;

use vault_buddy_core::transcript::Segment;

/// A speech-to-text backend. `samples` are 16 kHz mono f32 in [-1, 1];
/// `language` is an ISO code (e.g. "es") or None to auto-detect.
pub trait Transcriber {
    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<Vec<Segment>, String>;
}

pub struct TranscribeOptions {
    pub language: Option<String>,
    pub timestamps: bool,
    pub model_label: String,
}

use std::path::{Path, PathBuf};
use vault_buddy_core::transcript::{self, TranscriptMeta};

/// Decode → transcribe → atomically replace the sidecar with the finished
/// transcript. `generated_at` (RFC3339) is passed in so this stays
/// clock-free and testable. On any error the sidecar is left as-is (the
/// caller writes a retryable `failed` note); a `complete` transcript is only
/// ever written on success.
pub fn transcribe_recording(
    mp3: &Path,
    transcriber: &dyn Transcriber,
    opts: &TranscribeOptions,
    generated_at: &str,
) -> Result<PathBuf, String> {
    let samples = decode::decode_to_16k_mono(mp3)?;
    let duration_secs = samples.len() as u64 / decode::WHISPER_RATE as u64;
    let segments = transcriber.transcribe(&samples, opts.language.as_deref())?;
    let mp3_file_name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = TranscriptMeta {
        mp3_file_name,
        model_label: opts.model_label.clone(),
        language: opts.language.clone(),
        duration_secs,
        generated_at: generated_at.to_string(),
        timestamps: opts.timestamps,
    };
    let content = transcript::render_transcript(&meta, &segments);
    let path = transcript::transcript_path(mp3);
    transcript::replace_if_ours(&path, &content).map_err(|e| format!("write transcript: {e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_core::transcript::{transcript_path, Segment};

    struct FakeOk;
    impl Transcriber for FakeOk {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>) -> Result<Vec<Segment>, String> {
            Ok(vec![Segment {
                start_ms: 0,
                end_ms: 1000,
                text: "hello world".into(),
            }])
        }
    }
    struct FakeErr;
    impl Transcriber for FakeErr {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>) -> Result<Vec<Segment>, String> {
            Err("engine exploded".into())
        }
    }

    fn write_mp3(path: &std::path::Path) {
        use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm};
        let rate = 44_100u32;
        let frames = rate as usize / 2;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin()
                * 0.4
                * i16::MAX as f32) as i16;
            pcm.push(s);
            pcm.push(s);
        }
        let mut b = Builder::new().unwrap();
        b.set_num_channels(2).unwrap();
        b.set_sample_rate(rate).unwrap();
        b.set_brate(Bitrate::Kbps128).unwrap();
        b.set_quality(mp3lame_encoder::Quality::Good).unwrap();
        let mut enc = b.build().unwrap();
        // encode_to_vec/flush_to_vec only write into the Vec's existing
        // *spare* capacity — they never grow it. An unreserved `Vec::new()`
        // hands LAME a zero-length buffer and it writes out of bounds anyway
        // (SIGSEGV, hit in Task 7's decode.rs fixture). Reserve first, exactly
        // as `capture::encoder::Mp3Encoder` and decode.rs's `make_mp3` do.
        let mut out = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(frames));
        enc.encode_to_vec(InterleavedPcm(&pcm[..]), &mut out)
            .unwrap();
        out.reserve(7200);
        enc.flush_to_vec::<FlushNoGap>(&mut out).unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn opts() -> TranscribeOptions {
        TranscribeOptions {
            language: Some("en".into()),
            timestamps: true,
            model_label: "whisper-small".into(),
        }
    }

    #[test]
    fn transcribe_writes_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        write_mp3(&mp3);
        let path =
            transcribe_recording(&mp3, &FakeOk, &opts(), "2026-07-04T15:00:00+00:00").unwrap();
        assert_eq!(path, transcript_path(&mp3));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("vault-buddy-transcript: complete"));
        assert!(text.contains("[00:00:00] hello world"));
    }

    #[test]
    fn engine_error_leaves_no_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        write_mp3(&mp3);
        let err = transcribe_recording(&mp3, &FakeErr, &opts(), "t").unwrap_err();
        assert!(err.contains("engine exploded"));
        assert!(!transcript_path(&mp3).exists());
    }
}
