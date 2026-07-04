//! Local speech-to-text: decode our MP3 to 16 kHz mono PCM and run
//! whisper.cpp (via whisper-rs, behind the `whisper` feature) behind a
//! `Transcriber` trait so orchestration is testable without a real model.

pub mod decode;

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
