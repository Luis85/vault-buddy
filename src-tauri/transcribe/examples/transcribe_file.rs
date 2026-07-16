//! Print-only transcription harness (never writes a vault). Proves the fixed
//! whisper engine transcribes real audio end-to-end.
//!
//! Run (Windows, model already cached):
//!   cargo run -p vault_buddy_transcribe --features whisper \
//!     --example transcribe_file -- "<path-to.mp3>" [base|small|medium]
//!
//! The model is resolved from VB_TEST_MODEL (a .bin path) or the standard
//! cache (model_path(tier)).
use std::path::PathBuf;
use vault_buddy_transcribe::decode::decode_to_16k_mono;
use vault_buddy_transcribe::engine::WhisperTranscriber;
use vault_buddy_transcribe::model::{model_path, ModelTier};
use vault_buddy_transcribe::{CancelToken, EngineOptions, Transcriber};

fn main() {
    let mut args = std::env::args().skip(1);
    let audio = args.next().expect("usage: transcribe_file <mp3> [tier]");
    let tier = ModelTier::from_str(&args.next().unwrap_or_else(|| "base".into()));

    let model = std::env::var("VB_TEST_MODEL")
        .ok()
        .map(PathBuf::from)
        .or_else(|| model_path(tier))
        .expect("no model path resolvable");
    eprintln!("model: {}", model.display());
    assert!(model.exists(), "model not found at {}", model.display());

    let cancel = CancelToken::new();
    let started = std::time::Instant::now();
    let samples = decode_to_16k_mono(std::path::Path::new(&audio), &cancel).expect("decode");
    eprintln!(
        "decoded {} samples (~{}s) in {}s",
        samples.len(),
        samples.len() / 16_000,
        started.elapsed().as_secs()
    );

    let t = WhisperTranscriber::load(&model).expect("load model");
    let inf = std::time::Instant::now();
    let opts = EngineOptions {
        language: None,
        initial_prompt: None,
        vad_model: None,
    };
    let segments = t
        .transcribe(
            &samples,
            &opts,
            &cancel,
            Box::new(|p| eprintln!("progress {p}%")),
        )
        .expect("transcribe returned Err (the -6 abort would land here)");
    eprintln!(
        "OK: {} segments in {}s",
        segments.len(),
        inf.elapsed().as_secs()
    );
    for s in &segments {
        println!("[{:>7}ms] {}", s.start_ms, s.text);
    }
}
