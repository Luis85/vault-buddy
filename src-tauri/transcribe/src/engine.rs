//! whisper.cpp binding (whisper-rs), compiled only with the `whisper`
//! feature. Static-linked — no runtime DLL. Not Linux-tested; the
//! windows-app CI job is the compile gate.

use crate::{CancelToken, Transcriber};
use std::ffi::c_void;
use std::path::Path;
use vault_buddy_core::transcript::Segment;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// The abort wiring installed into a `FullParams`: the boxed token whose
/// address ggml holds as `user_data`. This guard MUST outlive the `full()`
/// call — ggml dereferences `user_data` from its worker threads — so callers
/// keep it on the stack for the whole inference. The `callback`/`user_data`
/// fields (test-only) are the exact C function pointer + `user_data` we handed
/// whisper.cpp, so the regression test can invoke them directly (no model, no
/// audio needed).
struct AbortWiring {
    // Owns the heap allocation `user_data` points at. Boxed so its address is
    // stable across the move out of `wire_abort_callback`.
    _token: Box<CancelToken>,
    #[cfg(test)]
    callback: unsafe extern "C" fn(*mut c_void) -> bool,
    #[cfg(test)]
    user_data: *mut c_void,
}

/// Wire whisper's abort callback to `cancel`, WITHOUT going through
/// whisper-rs 0.16's `set_abort_callback_safe`. That wrapper monomorphizes its
/// trampoline as `trampoline::<F>` (the concrete closure) but installs a
/// `user_data` that points at a `Box<Box<dyn FnMut() -> bool>>` (a fat
/// pointer); the trampoline then reads a garbage byte as the abort bool
/// (upstream tazz4843/whisper-rs#277, still open). On some CPUs that byte is
/// consistently truthy, so whisper.cpp aborts every encode window at 0% and
/// EVERY transcription fails with `-6 "failed to encode"` — with our own
/// cancel token never set (`cancelled=false` in the log). Here the trampoline
/// and the `user_data` type agree: `user_data` is a `*const CancelToken`, and
/// the trampoline casts it straight back, so the abort reflects the real token.
fn wire_abort_callback(params: &mut FullParams, cancel: &CancelToken) -> AbortWiring {
    // The trampoline's `user_data` is exactly `*const CancelToken` — no boxing
    // of a trait object, so no thin/fat pointer mismatch.
    unsafe extern "C" fn trampoline(user_data: *mut c_void) -> bool {
        // Safety: `user_data` is the address of the `CancelToken` inside the
        // `Box` held by the returned `AbortWiring`, which the caller keeps
        // alive for the whole `full()` call. `is_cancelled()` is an atomic
        // load, safe to call from ggml's worker threads.
        let token = &*(user_data as *const CancelToken);
        token.is_cancelled()
    }

    let token = Box::new(cancel.clone());
    let user_data = &*token as *const CancelToken as *mut c_void;
    // SAFETY: the trampoline reads `user_data` as `*const CancelToken`, which
    // is exactly what we install; the box keeps it alive (see AbortWiring).
    unsafe {
        params.set_abort_callback(Some(trampoline));
        params.set_abort_callback_user_data(user_data);
    }
    AbortWiring {
        _token: token,
        #[cfg(test)]
        callback: trampoline,
        #[cfg(test)]
        user_data,
    }
}

/// Route whisper.cpp + ggml's native (C-level) logs into the Rust `log`
/// crate — and thus Vault Buddy's log files — instead of their default
/// stderr sink, which a windowed Windows build silently discards. Without
/// this the engine's own diagnostics (model-load details, and the context
/// behind an aborted inference like the `-9` that `inference_failure_message`
/// maps) went nowhere at all. Call ONCE before the first context is created;
/// re-installing the same global hook is harmless. Relies on whisper-rs's
/// `log_backend` feature (enabled in Cargo.toml).
pub fn install_logging_hooks() {
    whisper_rs::install_logging_hooks();
}

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
        // Thread count is capped (see whisper_thread_count): the encoder
        // gains little past a handful of threads, and a very high ggml thread
        // count is a leading suspect in `-6 failed to encode` on high-core
        // machines. The helper also keeps CPU headroom so a recording started
        // mid-inference isn't starved (whisper.full() is a blocking
        // multi-minute FFI call that cannot yield once running).
        let n_threads = crate::whisper_thread_count(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2),
        ) as std::os::raw::c_int;
        params.set_n_threads(n_threads);
        log::info!("whisper: transcribing with {n_threads} threads");
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
        // Wire the abort callback ourselves rather than via whisper-rs's
        // `set_abort_callback_safe`, which is broken (see wire_abort_callback:
        // a trampoline/user_data type mismatch made it read a garbage abort
        // bool and fail every encode with `-6`). `_abort` owns the boxed token
        // ggml dereferences from its worker threads, so it must stay alive for
        // the whole `full()` call below — do NOT drop or `let _ =` it.
        let _abort = wire_abort_callback(&mut params, cancel);
        // Box<dyn FnMut(i32)+Send> is itself FnMut(i32)+'static — pass by
        // value. Progress uses whisper-rs's safe setter, which (unlike abort)
        // wires its trampoline correctly.
        params.set_progress_callback_safe(on_progress);
        state.full(params, samples).map_err(|e| {
            // whisper.cpp's whisper_full failures arrive as GenericError(rc)
            // carrying the raw return code (e.g. -9 = "failed to decode" in the
            // sampling loop). Hand the code to the shared, unit-tested mapper so
            // the user sees actionable guidance instead of "Generic whisper
            // error. ... Error code: -9"; the raw text is kept for other codes.
            let raw = e.to_string();
            let code = match e {
                whisper_rs::WhisperError::GenericError(c) => Some(c),
                _ => None,
            };
            // Self-diagnosing: a bare `-6` told us nothing last time. Record
            // the conditions the next failure needs — whether it was OUR abort
            // (cancel set) vs a genuine compute failure, the thread count (the
            // leading `-6` suspect on high-core machines), and the clip size —
            // so the log alone says which. whisper.cpp's own "failed to
            // encode/decode" line is already captured via install_logging_hooks.
            log::error!(
                "whisper inference failed: code={code:?} n_threads={n_threads} cancelled={} samples={} (~{}s audio)",
                cancel.is_cancelled(),
                samples.len(),
                samples.len() / crate::decode::WHISPER_RATE as usize
            );
            crate::inference_failure_message(code, &raw)
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: whisper-rs 0.16's `FullParams::set_abort_callback_safe`
    // monomorphizes its C trampoline as `trampoline::<F>` (the concrete
    // closure type) while the `user_data` it installs points at a
    // `Box<Box<dyn FnMut() -> bool>>` — a fat pointer. The trampoline then
    // reinterprets that as the (thin) closure and reads a garbage byte as the
    // "abort" bool (upstream bug tazz4843/whisper-rs#277, still open). On this
    // machine that byte is consistently truthy, so whisper.cpp aborted EVERY
    // encode window at 0% and every transcription failed with
    // `-6 "failed to encode"` (or `-9`), with our own cancel token never set.
    // `wire_abort_callback` bypasses the broken wrapper with a correctly-typed
    // trampoline, so the installed C callback reflects the token exactly.
    #[test]
    fn wired_abort_callback_reflects_token_not_garbage() {
        let cancel = CancelToken::new();
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // Installs the callback into `params` AND hands back the exact C
        // function pointer + user_data whisper.cpp will call, so we can invoke
        // it here without a model or any audio. The guard owns the boxed token
        // and must outlive the callback.
        let wiring = wire_abort_callback(&mut params, &cancel);

        // Not cancelled → must return false. The broken `set_abort_callback_safe`
        // returns a garbage bool here (truthy on this machine), which is the
        // whole bug.
        assert!(
            !unsafe { (wiring.callback)(wiring.user_data) },
            "an un-cancelled token must not report an abort"
        );

        cancel.cancel();
        assert!(
            unsafe { (wiring.callback)(wiring.user_data) },
            "a cancelled token must report an abort"
        );
    }
}
