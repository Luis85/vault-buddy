//! Local speech-to-text: decode our MP3 to 16 kHz mono PCM and run
//! whisper.cpp (via whisper-rs, behind the `whisper` feature) behind a
//! `Transcriber` trait so orchestration is testable without a real model.

pub mod decode;
#[cfg(feature = "whisper")]
pub mod engine;
pub mod model;
pub mod vad;

use vault_buddy_core::transcript::Segment;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A shared abort flag polled by whisper's abort callback and checked between
/// stages. Cloning shares the flag (Arc), so the shell holds one and the
/// engine another.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Cancel and failure are different outcomes: cancel writes a `cancelled`
/// sidecar (no scary toast, no auto-retry); failure writes a retryable `failed`.
#[derive(Debug)]
pub enum TranscribeError {
    Cancelled,
    Failed(String),
}

/// Per-job knobs threaded into the engine — a borrowed view over
/// `TranscribeOptions`. A struct rather than positional parameters so the
/// next knob doesn't ripple through every `Transcriber` implementor again.
pub struct EngineOptions<'a> {
    /// ISO language code (e.g. "es"), or None to auto-detect.
    pub language: Option<&'a str>,
    /// Vocabulary/context priming; None = no prompt (whisper's default).
    pub initial_prompt: Option<&'a str>,
    /// Some(path to the Silero ggml) enables VAD with that model; None = off
    /// (either the setting is off, or the model wasn't available and the job
    /// degraded — see the shell's ensure_vad_model).
    pub vad_model: Option<&'a Path>,
}

/// A speech-to-text backend. `samples` are 16 kHz mono f32 in [-1, 1];
/// `opts` carries the language plus the prompt/VAD knobs (see
/// `EngineOptions`). `cancel` is polled by the engine's abort callback (an
/// aborted run returns `Err`, which `transcribe_recording` disambiguates via
/// the token); `on_progress` is the engine's 0-100 percent callback,
/// forwarded as-is.
pub trait Transcriber {
    /// `Ok` carries the recognized segments plus `vad_engaged`: whether
    /// this run actually filtered non-speech via VAD (an all-silence
    /// buffer that short-circuits before whisper runs at all still counts
    /// as engaged). This is the EFFECTIVE state, not the request — a
    /// VAD-enabled job whose detection failed degrades to `false` (an
    /// unfiltered run) even though `opts.vad_model` was `Some`.
    /// `transcribe_recording` records it as `TranscriptMeta.vad`, so the
    /// stats footer reports what actually happened.
    fn transcribe(
        &self,
        samples: &[f32],
        opts: &EngineOptions,
        cancel: &CancelToken,
        on_progress: Box<dyn FnMut(i32) + Send>,
    ) -> Result<(Vec<Segment>, bool), String>;
}

pub struct TranscribeOptions {
    pub language: Option<String>,
    pub timestamps: bool,
    pub model_label: String,
    /// Composed title+vocabulary priming (see `compose_initial_prompt`).
    pub initial_prompt: Option<String>,
    /// Resolved Silero model path when this job runs with VAD.
    pub vad_model: Option<PathBuf>,
}

use std::path::{Path, PathBuf};
use vault_buddy_core::transcript::{self, TranscriptMeta};

/// The non-error result of a transcription attempt. Decoding and inference
/// both succeeding is not the same as the sidecar actually changing: a
/// finished (`complete`) or hand-edited transcript is deliberately left
/// alone (see `transcript::replace_if_ours`). `SkippedForeign` lets the
/// caller tell that apart from a real write instead of reporting a lying
/// blanket "success" for both.
#[derive(Debug)]
pub enum TranscribeOutcome {
    Written(PathBuf),
    SkippedForeign(PathBuf),
}

/// Turn a raw whisper.cpp inference failure into guidance a user can act on.
/// `code` is whisper's own `whisper_full` return code (carried by
/// `whisper_rs::WhisperError::GenericError`); `raw` is whisper-rs's Display
/// text, kept for the codes we have no specific advice for.
///
/// whisper.cpp returns -6..-9 ("failed to encode/decode") when it aborts in
/// the encode/decode loop. That is almost never a bug the user can see — in
/// practice it's a too-short, near-silent, or non-speech clip, or the machine
/// running low on memory — so those get plain-language, actionable guidance
/// instead of the opaque "Generic whisper error. ... Error code: -9". The
/// numeric code is still appended so the logs and any support request keep the
/// exact failure. Lives outside the `whisper` feature gate so it's unit-tested
/// on Linux; `engine.rs` (Windows-only) extracts the code and calls it.
pub fn inference_failure_message(code: Option<i32>, raw: &str) -> String {
    match code {
        Some(c) if (-9..=-6).contains(&c) => format!(
            "Whisper stopped while processing this recording. This usually means the \
             audio was too short, silent, or wasn't speech — or the machine ran low on \
             memory. Try recording again, or switch to a smaller model in Transcription \
             settings. (whisper error {c})"
        ),
        _ => format!("Transcription failed during inference. ({raw})"),
    }
}

/// Compose whisper's `initial_prompt` from the recording's title and the
/// vault's custom vocabulary. Title FIRST, vocabulary LAST: whisper keeps
/// only the trailing `n_text_ctx/2` tokens of an over-long prompt (it
/// truncates from the front), so the user's explicit vocabulary is the part
/// that must survive. `None` when there is nothing to prime with — whisper
/// then behaves exactly as it did before this feature existed.
///
/// Every part is stripped of control characters (`char::is_control`, which
/// covers `\0`) before the trim/emptiness checks. This is the one
/// chokepoint both the title and the vocabulary flow through on their way
/// into whisper's prompt: `engine.rs`'s `set_initial_prompt` does
/// `CString::new(prompt).expect(...)` internally (whisper-rs, not our own
/// code), and a NUL byte anywhere in the prompt panics and kills the named
/// transcription worker thread. `transcriptionVocabulary` lives in a
/// hand-editable `config.json`, so a stray control character is reachable
/// without the app ever writing one itself.
pub fn compose_initial_prompt(title: &str, vocabulary: Option<&str>) -> Option<String> {
    let parts: Vec<String> = [Some(title), vocabulary]
        .into_iter()
        .flatten()
        .map(|s| s.chars().filter(|c| !c.is_control()).collect::<String>())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(". "))
    }
}

/// Threads to hand whisper's encoder, capped at `WHISPER_MAX_THREADS`. The
/// encoder gains little past a handful of threads, and a very high ggml
/// thread count (e.g. ~30 on a high-core i9 under the old `cores - 2`) is a
/// leading suspect in a `-6 "failed to encode"` on otherwise capable
/// hardware. Keeps a 2-thread headroom so a recording started mid-inference
/// isn't CPU-starved, then clamps into `[1, WHISPER_MAX_THREADS]`. Pure and
/// unit-tested here; `engine.rs` (Windows-only) feeds it
/// `available_parallelism()`.
pub const WHISPER_MAX_THREADS: usize = 8;
pub fn whisper_thread_count(available: usize) -> usize {
    available.saturating_sub(2).clamp(1, WHISPER_MAX_THREADS)
}

/// Decode → transcribe → atomically replace the sidecar with the finished
/// transcript. `generated_at` (RFC3339) is passed in so this stays
/// clock-free and testable. `cancel`/`on_progress` are threaded into the
/// engine: cancellation is checked cheaply after decode (before paying for
/// inference) and, since an aborted `full()` also returns `Err`, the error
/// branch consults the token to tell a real engine failure apart from an
/// abort. Either way the sidecar is left as-is (the caller writes a
/// `cancelled`/retryable `failed` note); a `complete` transcript is only ever
/// written on success. `force` (the explicit re-transcribe path) overwrites
/// even a finished `complete` sidecar via `force_write_sidecar`; otherwise
/// the write goes through `replace_if_ours`, which never clobbers a
/// non-regenerable transcript — reported back as `TranscribeOutcome::SkippedForeign`
/// rather than silently folded into `Written`.
pub fn transcribe_recording(
    mp3: &Path,
    transcriber: &dyn Transcriber,
    opts: &TranscribeOptions,
    generated_at: &str,
    force: bool,
    cancel: &CancelToken,
    on_progress: Box<dyn FnMut(i32) + Send>,
) -> Result<TranscribeOutcome, TranscribeError> {
    let started = std::time::Instant::now();
    // Decode is now cancellable mid-file (it polls the same token). An
    // aborted decode returns Err, so disambiguate exactly as the inference
    // error branch below does: the token, not the error text, is the truth.
    let samples = decode::decode_to_16k_mono(mp3, cancel).map_err(|e| {
        if cancel.is_cancelled() {
            TranscribeError::Cancelled
        } else {
            TranscribeError::Failed(e)
        }
    })?;
    if cancel.is_cancelled() {
        return Err(TranscribeError::Cancelled); // cheap to bail before inference
    }
    let duration_secs = samples.len() as u64 / decode::WHISPER_RATE as u64;
    let decode_secs = started.elapsed().as_secs_f32();
    let inference_start = std::time::Instant::now();
    // Honest logging: the log never goes dark on inference start again.
    log::info!(
        "transcribe: inference start {} ({duration_secs}s audio)",
        mp3.display()
    );
    let engine_opts = EngineOptions {
        language: opts.language.as_deref(),
        initial_prompt: opts.initial_prompt.as_deref(),
        vad_model: opts.vad_model.as_deref(),
    };
    let (segments, vad_engaged) =
        match transcriber.transcribe(&samples, &engine_opts, cancel, on_progress) {
            Ok(t) => t,
            // An aborted full() returns Err; the token says whether it was us.
            Err(e) => {
                return Err(if cancel.is_cancelled() {
                    TranscribeError::Cancelled
                } else {
                    TranscribeError::Failed(e)
                })
            }
        };
    let inference_secs = inference_start.elapsed().as_secs_f32();
    let n_segments = segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .count();
    if n_segments == 0 {
        log::warn!(
            "transcribe: no speech detected in {} ({duration_secs}s audio)",
            mp3.display()
        );
    }
    log::info!(
        "transcribe: complete {} — {n_segments} segments, {duration_secs}s audio, decode {decode_secs:.1}s + inference {inference_secs:.1}s",
        mp3.display()
    );
    // Wall-clock of the actual work (decode + inference). Measured here, not in
    // core, so render_transcript stays clock-free and deterministic.
    let processing_secs = started.elapsed().as_secs();
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
        processing_secs,
        // The EFFECTIVE state the transcriber reports, not the request —
        // see Transcriber::transcribe's doc comment. Replaces the old
        // `opts.vad_model.is_some()`, which recorded the setting rather
        // than what actually happened (a VAD-enabled job whose Silero
        // detection failed used to lie "on" in the stats footer).
        vad: vad_engaged,
    };
    let content = transcript::render_transcript(&meta, &segments);
    let path = transcript::transcript_path(mp3);
    if force {
        // Explicit re-transcribe: overwrite even a finished sidecar.
        transcript::force_write_sidecar(&path, &content)
            .map_err(|e| TranscribeError::Failed(format!("write transcript: {e}")))?;
        return Ok(TranscribeOutcome::Written(path));
    }
    match transcript::replace_if_ours(&path, &content)
        .map_err(|e| TranscribeError::Failed(format!("write transcript: {e}")))?
    {
        transcript::ReplaceOutcome::Written => Ok(TranscribeOutcome::Written(path)),
        transcript::ReplaceOutcome::SkippedForeign => {
            log::warn!(
                "transcribe: left an existing non-regenerable sidecar untouched (not overwritten): {}",
                path.display()
            );
            Ok(TranscribeOutcome::SkippedForeign(path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use vault_buddy_core::transcript::{transcript_path, Segment};

    #[test]
    fn decode_codes_get_actionable_guidance_not_raw_jargon() {
        // whisper.cpp's whisper_full returns -6..-9 ("failed to encode/decode")
        // when it aborts mid-inference — in practice a too-short/near-silent/
        // non-speech clip or low memory, not something the raw "Generic whisper
        // error. ... Error code: -9" string conveys to a user.
        for c in [-6, -7, -8, -9] {
            let raw =
                format!("Generic whisper error. Varies depending on the function. Error code: {c}");
            let msg = inference_failure_message(Some(c), &raw);
            assert!(
                msg.to_lowercase().contains("too short") && msg.to_lowercase().contains("memory"),
                "code {c} should get actionable guidance, got: {msg}"
            );
            // The raw whisper-rs jargon must be replaced (not shown verbatim)…
            assert!(
                !msg.contains("Generic whisper error"),
                "code {c} still leaks raw jargon: {msg}"
            );
            // …but the numeric code stays for the log/support trail.
            assert!(
                msg.contains(&c.to_string()),
                "code {c} should still surface the number for support: {msg}"
            );
        }
    }

    #[test]
    fn unknown_or_missing_codes_fall_back_to_the_raw_error() {
        // A code we don't have specific guidance for (or none at all) must not
        // be swallowed — the raw text is the only diagnostic left.
        assert!(
            inference_failure_message(Some(-2), "some raw whisper error")
                .contains("some raw whisper error")
        );
        assert!(
            inference_failure_message(None, "context load failed").contains("context load failed")
        );
    }

    #[test]
    fn whisper_thread_count_keeps_headroom_and_caps_high_core_machines() {
        // Never zero, even on 1-2 logical cores.
        assert_eq!(whisper_thread_count(1), 1);
        assert_eq!(whisper_thread_count(2), 1);
        // Modest machines: logical cores minus the 2-thread headroom.
        assert_eq!(whisper_thread_count(4), 2);
        assert_eq!(whisper_thread_count(9), 7);
        // High-core i9 (the -6 report): capped, NOT ~30.
        assert_eq!(whisper_thread_count(10), WHISPER_MAX_THREADS);
        assert_eq!(whisper_thread_count(32), WHISPER_MAX_THREADS);
        // Defensive: a bogus 0 from a broken probe still yields a usable count.
        assert_eq!(whisper_thread_count(0), 1);
    }

    struct FakeOk;
    impl Transcriber for FakeOk {
        fn transcribe(
            &self,
            _s: &[f32],
            _o: &EngineOptions,
            _c: &CancelToken,
            mut on_progress: Box<dyn FnMut(i32) + Send>,
        ) -> Result<(Vec<Segment>, bool), String> {
            on_progress(100); // exercises the forwarder
            Ok((
                vec![Segment {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "hello world".into(),
                }],
                false,
            ))
        }
    }
    // Sibling of FakeOk with vad_engaged=true — the "on" half of the
    // TranscriptMeta.vad wiring (see transcribe_records_vad_engaged_when_
    // the_engine_reports_it below), so the meta-wiring assertion isn't
    // pinned only against the always-false FakeOk.
    struct FakeOkVad;
    impl Transcriber for FakeOkVad {
        fn transcribe(
            &self,
            _s: &[f32],
            _o: &EngineOptions,
            _c: &CancelToken,
            _p: Box<dyn FnMut(i32) + Send>,
        ) -> Result<(Vec<Segment>, bool), String> {
            Ok((
                vec![Segment {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "hello world".into(),
                }],
                true,
            ))
        }
    }
    struct FakeEmpty;
    impl Transcriber for FakeEmpty {
        fn transcribe(
            &self,
            _s: &[f32],
            _o: &EngineOptions,
            _c: &CancelToken,
            _p: Box<dyn FnMut(i32) + Send>,
        ) -> Result<(Vec<Segment>, bool), String> {
            Ok((vec![], false))
        }
    }

    #[test]
    fn no_speech_writes_a_complete_transcript_with_a_notice() {
        // A successful inference with zero segments is not a failure: it writes a
        // `complete` sidecar that says "No speech detected", not a blank one, and
        // does not error.
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let outcome = transcribe_recording(
            &mp3,
            &FakeEmpty,
            &opts(),
            "2026-07-07T09:00:00+00:00",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        assert!(matches!(outcome, TranscribeOutcome::Written(_)));
        let text = std::fs::read_to_string(transcript_path(&mp3)).unwrap();
        assert!(text.contains("vault-buddy-transcript: complete"));
        assert!(text.contains("_No speech detected._"));
    }

    struct FakeErr;
    impl Transcriber for FakeErr {
        fn transcribe(
            &self,
            _s: &[f32],
            _o: &EngineOptions,
            cancel: &CancelToken,
            _p: Box<dyn FnMut(i32) + Send>,
        ) -> Result<(Vec<Segment>, bool), String> {
            // Mirrors whisper: an aborted full() returns Err; the token disambiguates.
            if cancel.is_cancelled() {
                return Err("aborted".into());
            }
            Err("boom".into())
        }
    }

    fn noop_progress() -> Box<dyn FnMut(i32) + Send> {
        Box::new(|_| {})
    }

    /// Builds a tiny real MP3 inside `dir` and returns its path. Shared by
    /// every test below (DRY) — encoding is the same fixture regardless of
    /// which Transcriber fake or cancel state is under test.
    fn write_tiny_mp3(dir: &std::path::Path) -> PathBuf {
        use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm};
        let mp3 = dir.join("2026-07-04 1405 Meeting.mp3");
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
        std::fs::write(&mp3, out).unwrap();
        mp3
    }

    fn opts() -> TranscribeOptions {
        TranscribeOptions {
            language: Some("en".into()),
            timestamps: true,
            model_label: "whisper-small".into(),
            initial_prompt: None,
            vad_model: None,
        }
    }

    #[test]
    fn transcribe_writes_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let outcome = transcribe_recording(
            &mp3,
            &FakeOk,
            &opts(),
            "2026-07-04T15:00:00+00:00",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        assert!(matches!(outcome, TranscribeOutcome::Written(p) if p == transcript_path(&mp3)));
        let path = transcript_path(&mp3);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("vault-buddy-transcript: complete"));
        assert!(text.contains("[00:00:00] hello world"));
        assert!(text.contains("## Statistics"));
        assert!(text.contains("| Model | whisper-small |"));
        // FakeOk reports vad_engaged=false — pins TranscriptMeta.vad wiring
        // end-to-end (Transcriber::transcribe's bool -> transcribe_recording
        // -> meta.vad -> the rendered stats row), not just core's
        // render_transcript-level unit test.
        assert!(text.contains("| Silence skipping (VAD) | off |"));
    }

    #[test]
    fn transcribe_records_vad_engaged_when_the_engine_reports_it() {
        // Sibling of transcribe_writes_the_sidecar covering the "on" half of
        // the same wiring: a transcriber reporting vad_engaged=true must
        // produce an "on" row, proving the flag actually travels through
        // rather than the stats row defaulting to off regardless.
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        transcribe_recording(
            &mp3,
            &FakeOkVad,
            &opts(),
            "2026-07-04T15:00:00+00:00",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        let text = std::fs::read_to_string(transcript_path(&mp3)).unwrap();
        assert!(text.contains("| Silence skipping (VAD) | on |"));
    }

    #[test]
    fn engine_error_leaves_no_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let err = transcribe_recording(
            &mp3,
            &FakeErr,
            &opts(),
            "t",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap_err();
        assert!(matches!(&err, TranscribeError::Failed(msg) if msg.contains("boom")));
        assert!(!transcript_path(&mp3).exists());
    }

    #[test]
    fn decode_error_leaves_no_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&mp3, b"this is not a valid mp3 stream at all").unwrap();
        let err = transcribe_recording(
            &mp3,
            &FakeOk,
            &opts(),
            "t",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap_err();
        assert!(matches!(err, TranscribeError::Failed(_)));
        assert!(
            !transcript_path(&mp3).exists(),
            "no sidecar when decode fails"
        );
    }

    #[test]
    fn force_regenerates_a_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let path = transcript_path(&mp3);
        std::fs::write(&path, "---\nvault-buddy-transcript: complete\n---\nOLD").unwrap();
        // Without force, a complete transcript is left untouched...
        let outcome = transcribe_recording(
            &mp3,
            &FakeOk,
            &opts(),
            "t",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        assert!(matches!(outcome, TranscribeOutcome::SkippedForeign(p) if p == path));
        assert!(std::fs::read_to_string(&path).unwrap().contains("OLD"));
        // ...with force, it is regenerated.
        let outcome = transcribe_recording(
            &mp3,
            &FakeOk,
            &opts(),
            "t",
            true,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        assert!(matches!(outcome, TranscribeOutcome::Written(p) if p == path));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("OLD"));
        assert!(text.contains("hello world"));
    }

    #[test]
    fn skips_a_complete_sidecar_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let path = transcript_path(&mp3);
        let original = "---\nvault-buddy-transcript: complete\n---\nOLD";
        std::fs::write(&path, original).unwrap();
        let outcome = transcribe_recording(
            &mp3,
            &FakeOk,
            &opts(),
            "t",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        assert!(
            matches!(outcome, TranscribeOutcome::SkippedForeign(p) if p == path),
            "a non-regenerable sidecar must be reported as skipped, not silently treated as success"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "the existing (foreign) transcript body must remain untouched"
        );
    }

    #[test]
    fn cancel_token_flips() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
        assert!(t.clone().is_cancelled(), "clones share the flag");
    }

    #[test]
    fn precancelled_writes_no_complete_sidecar_and_returns_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = transcribe_recording(
            &mp3,
            &FakeErr,
            &opts(),
            "2026-07-06T09:30:00Z",
            false,
            &cancel,
            noop_progress(),
        );
        assert!(matches!(r, Err(TranscribeError::Cancelled)));
        assert!(
            !transcript_path(&mp3).exists()
                || !std::fs::read_to_string(transcript_path(&mp3))
                    .unwrap()
                    .contains("complete")
        );
    }

    #[test]
    fn failure_is_distinguished_from_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let r = transcribe_recording(
            &mp3,
            &FakeErr,
            &opts(),
            "t",
            false,
            &CancelToken::new(),
            noop_progress(),
        );
        assert!(matches!(r, Err(TranscribeError::Failed(_))));
    }

    #[test]
    fn precancelled_bails_even_with_a_transcriber_that_would_succeed() {
        // FakeOk never looks at the cancel token and always returns Ok, so
        // the only way this can come back Cancelled is the after-decode,
        // before-inference bail inside transcribe_recording itself — this
        // isolates that check from the error-branch disambiguation that
        // `precancelled_writes_no_complete_sidecar_and_returns_cancelled`
        // (which uses FakeErr) also happens to exercise.
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = transcribe_recording(&mp3, &FakeOk, &opts(), "t", false, &cancel, noop_progress());
        assert!(
            matches!(r, Err(TranscribeError::Cancelled)),
            "FakeOk ignores the token and would return Ok if the after-decode bail hadn't fired"
        );
    }

    #[test]
    fn on_progress_is_forwarded_to_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = Arc::clone(&calls);
        let progress: Box<dyn FnMut(i32) + Send> = Box::new(move |_pct| {
            calls_clone.fetch_add(1, Ordering::SeqCst);
        });
        transcribe_recording(
            &mp3,
            &FakeOk,
            &opts(),
            "t",
            false,
            &CancelToken::new(),
            progress,
        )
        .unwrap();
        assert!(
            calls.load(Ordering::SeqCst) > 0,
            "on_progress must actually be invoked by the transcriber, not dropped"
        );
    }

    #[test]
    fn compose_initial_prompt_orders_title_first_vocabulary_last() {
        // Vocabulary LAST: whisper truncates an over-long prompt from the
        // FRONT (it keeps the trailing n_text_ctx/2 tokens), and the user's
        // explicit vocabulary is the part that must survive truncation.
        assert_eq!(
            compose_initial_prompt("Budget review", Some("Kubernetes, rmcp")),
            Some("Budget review. Kubernetes, rmcp".to_string())
        );
    }

    #[test]
    fn compose_initial_prompt_handles_missing_parts() {
        assert_eq!(
            compose_initial_prompt("Meeting", None),
            Some("Meeting".to_string())
        );
        assert_eq!(
            compose_initial_prompt("", Some("ggml")),
            Some("ggml".to_string())
        );
        assert_eq!(compose_initial_prompt("", None), None);
        // Whitespace-only parts count as missing.
        assert_eq!(compose_initial_prompt("   ", Some("  ")), None);
        assert_eq!(
            compose_initial_prompt("  Standup  ", Some("  cpal  ")),
            Some("Standup. cpal".to_string())
        );
    }

    #[test]
    fn compose_initial_prompt_strips_control_characters_including_nul() {
        // whisper-rs's set_initial_prompt does CString::new(prompt)
        // .expect(...) internally — a NUL byte (or any other control
        // character) surviving into the composed prompt panics and kills
        // the named transcription worker thread. transcriptionVocabulary
        // lives in a hand-editable config.json, so this is reachable
        // without the app ever writing a control character itself; the
        // title path (built from a capture's file name) gets the same
        // treatment for free since both flow through the same map here.
        assert_eq!(
            compose_initial_prompt("Standup", Some("a\0b\u{7}")),
            Some("Standup. ab".to_string())
        );
        assert_eq!(
            compose_initial_prompt("Team\0Sync", None),
            Some("TeamSync".to_string())
        );
        // Stripping can empty a part entirely — folds into the same
        // "missing part" handling as an originally-blank one.
        assert_eq!(compose_initial_prompt("\0", Some("\u{1}\u{2}")), None);
    }

    #[test]
    fn engine_options_reach_the_transcriber() {
        // The language/prompt/VAD knobs must actually arrive at the engine
        // — a TranscribeOptions field nobody forwards would silently do
        // nothing.
        use std::sync::Mutex;
        // Locally-scoped test fixture — a `type` alias would be more ceremony
        // than the one-off tuple it names.
        #[allow(clippy::type_complexity)]
        struct FakeSeen(Arc<Mutex<Option<(Option<String>, Option<String>, Option<PathBuf>)>>>);
        impl Transcriber for FakeSeen {
            fn transcribe(
                &self,
                _s: &[f32],
                opts: &EngineOptions,
                _c: &CancelToken,
                _p: Box<dyn FnMut(i32) + Send>,
            ) -> Result<(Vec<Segment>, bool), String> {
                *self.0.lock().unwrap() = Some((
                    opts.language.map(str::to_string),
                    opts.initial_prompt.map(str::to_string),
                    opts.vad_model.map(Path::to_path_buf),
                ));
                Ok((vec![], false))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let seen = Arc::new(Mutex::new(None));
        let fake = FakeSeen(Arc::clone(&seen));
        let opts = TranscribeOptions {
            initial_prompt: Some("Standup. cpal".to_string()),
            vad_model: Some(PathBuf::from("/models/silero.bin")),
            ..opts()
        };
        transcribe_recording(
            &mp3,
            &fake,
            &opts,
            "t",
            false,
            &CancelToken::new(),
            noop_progress(),
        )
        .unwrap();
        assert_eq!(
            seen.lock().unwrap().clone(),
            Some((
                Some("en".to_string()), // opts() below seeds language: Some("en")
                Some("Standup. cpal".to_string()),
                Some(PathBuf::from("/models/silero.bin"))
            ))
        );
    }
}
