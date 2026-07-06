//! whisper.cpp binding (whisper-rs), compiled only with the `whisper`
//! feature. Static-linked — no runtime DLL. Not Linux-tested; the
//! windows-app CI job is the compile gate.

use crate::{CancelToken, Transcriber};
use std::path::Path;
use vault_buddy_core::transcript::Segment;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct WhisperTranscriber {
    ctx: WhisperContext,
}

impl WhisperTranscriber {
    pub fn load(model_path: &Path) -> Result<Self, String> {
        // Pass the `&Path` straight through rather than round-tripping via
        // `to_string_lossy()`: `WhisperContext::new_with_params` takes it by
        // `AsRef<Path>` and converts with its own `path_to_bytes` internally,
        // so this avoids a lossy UTF-8 conversion for no benefit.
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| format!("load model {}: {e}", model_path.display()))?;
        Ok(Self { ctx })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        cancel: &CancelToken,
        on_progress: Box<dyn FnMut(i32) + Send>,
    ) -> Result<Vec<Segment>, String> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("whisper state: {e}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // Leave CPU headroom so a recording started mid-inference isn't
        // starved (the worker already postpones STARTING a job while
        // recording, but whisper.full() is a blocking multi-minute FFI call
        // that cannot yield once running).
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).max(1))
            .unwrap_or(2) as std::os::raw::c_int;
        params.set_n_threads(n_threads);
        // Always transcribe in the spoken/selected language — never translate
        // to English. The multilingual models (small especially) otherwise
        // drift to English translation on auto-detect; pinning the task off is
        // the reliable fix, and a pinned language (settings dropdown) removes
        // the drift entirely.
        params.set_translate(false);
        if let Some(lang) = language {
            params.set_language(Some(lang));
        }
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        // Owned clone → 'static abort closure; returning true aborts full().
        let cancel = cancel.clone();
        params.set_abort_callback_safe(move || cancel.is_cancelled());
        // Box<dyn FnMut(i32)+Send> is itself FnMut(i32)+'static — pass by value.
        params.set_progress_callback_safe(on_progress);
        state
            .full(params, samples)
            .map_err(|e| format!("whisper inference: {e}"))?;

        // whisper-rs 0.16: iterate WhisperSegment objects via state.as_iter();
        // timestamps are in centiseconds, converted to ms below (×10).
        let mut out = Vec::new();
        for segment in state.as_iter() {
            let text = segment
                .to_str_lossy()
                .unwrap_or_default()
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            let t0 = segment.start_timestamp().max(0) as u64;
            let t1 = segment.end_timestamp().max(0) as u64;
            out.push(Segment {
                start_ms: t0 * 10,
                end_ms: t1 * 10,
                text,
            });
        }
        Ok(out)
    }
}
