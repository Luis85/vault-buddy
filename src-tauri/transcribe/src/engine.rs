//! whisper.cpp binding (whisper-rs), compiled only with the `whisper`
//! feature. Static-linked — no runtime DLL. Not Linux-tested; the
//! windows-app CI job is the compile gate.

use crate::{CancelToken, Transcriber};
use std::ffi::{c_int, c_void};
use std::path::Path;
use vault_buddy_core::transcript::Segment;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperSysContext,
    WhisperSysState,
};

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

/// The progress wiring installed into a `FullParams`: the closure whose (inner)
/// heap address ggml holds as `user_data`. Like `AbortWiring` this guard MUST
/// outlive the `full()` call, but it exists for a DIFFERENT reason — it OWNS
/// the closure so the closure is FREED when the guard drops. The
/// `callback`/`user_data` fields (test-only) are the exact C function pointer +
/// `user_data` we handed whisper.cpp, so the regression test can invoke them
/// directly (no model, no audio needed).
///
/// `on_progress` only needs `Send`, not `Sync`, because — unlike
/// `abort_callback`, which whisper.cpp threads down into ggml's own compute
/// plan (`ggml_backend_set_abort_callback` → `cplan.abort_callback`, invoked
/// from inside a spawned worker thread in `ggml_graph_compute_thread`) —
/// `progress_callback` is only ever invoked from `whisper_full_with_state`'s
/// own single-threaded outer segment loop, on whichever thread called
/// `full()` (checked directly against the vendored whisper.cpp/ggml source,
/// not assumed): one call per seek/segment iteration, never concurrently.
//
// `_closure` is a `Box<Box<dyn FnMut>>` on purpose, NOT a redundant allocation:
// `user_data` must be a THIN pointer, but `&mut dyn FnMut` is a FAT pointer
// (data + vtable) that silently loses its vtable half when cast to
// `*mut c_void` — the exact fat/thin mismatch class that broke abort. The OUTER
// box lets us hand out `&mut *outer` (a `&mut Box<dyn FnMut>`, i.e. a thin
// pointer to the inner fat box). Boxed so the inner box's heap address stays
// stable across the move out of `wire_progress_callback` — moving the outer box
// moves only its stack-side thin pointer, never its heap allocation (the same
// reasoning as `AbortWiring::_token`).
#[allow(clippy::redundant_allocation)]
struct ProgressWiring {
    _closure: Box<Box<dyn FnMut(i32) + Send>>,
    #[cfg(test)]
    callback:
        unsafe extern "C" fn(*mut WhisperSysContext, *mut WhisperSysState, c_int, *mut c_void),
    #[cfg(test)]
    user_data: *mut c_void,
}

/// Wire whisper's progress callback to `on_progress` WITHOUT whisper-rs 0.16's
/// `set_progress_callback_safe`. That wrapper's trampoline is type-correct
/// (unlike the abort one), but it `Box::into_raw`s the closure and NOTHING ever
/// reclaims it — `FullParams` has no `Drop` that touches
/// `progress_callback_user_data` — so every transcription job leaks the closure
/// and everything it captures (an `AppHandle` + `PathBuf`) permanently. Here the
/// returned guard owns the closure and frees it on drop, after `full()`
/// returns. The trampoline and the installed `user_data` type agree: `user_data`
/// is a `*mut Box<dyn FnMut(i32) + Send>` (a thin pointer to the inner fat box)
/// and the trampoline casts it straight back, so no thin/fat mismatch (see the
/// `ProgressWiring` note for why the double box is load-bearing).
#[allow(clippy::redundant_allocation)]
fn wire_progress_callback(
    params: &mut FullParams,
    on_progress: Box<dyn FnMut(i32) + Send>,
) -> ProgressWiring {
    // `progress` is whisper.cpp's percent-complete (0..=100) as `c_int`; the
    // ctx/state pointers are unused (we never reach into whisper state here).
    unsafe extern "C" fn trampoline(
        _ctx: *mut WhisperSysContext,
        _state: *mut WhisperSysState,
        progress: c_int,
        user_data: *mut c_void,
    ) {
        // Safety: `user_data` is the address of the inner `Box<dyn FnMut(i32) +
        // Send>` owned by the returned `ProgressWiring`, which the caller keeps
        // alive for the whole `full()` call. The cast target matches EXACTLY
        // what we installed below — a thin pointer to the inner fat box — so the
        // vtable is intact and we call the real closure, not a garbage read.
        let closure = &mut *(user_data as *mut Box<dyn FnMut(i32) + Send>);
        closure(progress);
    }

    // Double-box: `on_progress` is already the inner fat `Box<dyn FnMut>`;
    // wrapping it once more yields an outer box we can take a THIN pointer into.
    // `&mut *closure` is `&mut Box<dyn FnMut>` — a thin pointer to the inner fat
    // box, which is what the trampoline reconstructs.
    let mut closure = Box::new(on_progress);
    let user_data = &mut *closure as *mut Box<dyn FnMut(i32) + Send> as *mut c_void;
    // SAFETY: the trampoline reads `user_data` as `*mut Box<dyn FnMut(i32) +
    // Send>`, which is exactly what we install; the box keeps it alive (see
    // ProgressWiring).
    unsafe {
        params.set_progress_callback(Some(trampoline));
        params.set_progress_callback_user_data(user_data);
    }
    ProgressWiring {
        _closure: closure,
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

/// Guard against upstream whisper.cpp#3750: on Windows MSVC static builds
/// (exactly the shipped `gpu` configuration) the ggml backend registry's
/// one-shot function-local-static constructor can hit a `vk::SystemError`
/// inside `ggml_vk_instance_init()`; `ggml_backend_vk_reg()` catches every
/// exception and returns null, `register_backend(null)` silently returns,
/// and the only failure log (`VK_LOG_DEBUG`) is compiled out of release
/// ggml — leaving the registry permanently CPU-only while the UI toggle
/// claims GPU (all verified against the whisper.cpp source vendored in
/// whisper-rs-sys 0.15). The remedy is the upstream workaround: after the
/// registry has constructed, re-attempt `ggml_backend_vk_reg()` OURSELVES
/// and register a non-null result explicitly — `vk_reg` is idempotent (a
/// static reg struct + once-guarded instance init) and exception-safe (its
/// internal try/catch is the very thing that returned null the first
/// time).
///
/// Do NOT "fix" this by calling `whisper_rs::vulkan::list_devices()` as
/// the first Vulkan touch instead (the recipe upstream's reporter used):
/// its `ggml_backend_vk_get_device_count()` path runs
/// `ggml_vk_instance_init()` with NO catch, so on a machine with a missing
/// or pre-1.2 Vulkan loader the throw unwinds across the C ABI into Rust —
/// UB/abort instead of the CPU fallback. It is only safe to enumerate
/// devices once the instance is known-good (which is when this fn does
/// it, as the GAP-62 "which GPU is it" diagnostic).
#[cfg(feature = "whisper-vulkan")]
fn ensure_vulkan_backend_registered() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        use whisper_rs::whisper_rs_sys as sys;
        // Touch the registry FIRST so its constructor runs now: on healthy
        // setups it registers Vulkan itself, and the by-name scan below
        // must see that to avoid a duplicate registration (the registry
        // does not dedupe).
        let registered = (0..sys::ggml_backend_reg_count()).any(|i| {
            let reg = sys::ggml_backend_reg_get(i);
            !reg.is_null()
                && std::ffi::CStr::from_ptr(sys::ggml_backend_reg_name(reg)).to_bytes()
                    == b"Vulkan"
        });
        if !registered {
            let vk = sys::ggml_backend_vk_reg();
            if vk.is_null() {
                // Instance init failed (caught inside C++) — no loader, no
                // ICD, or pre-1.2 Vulkan. whisper falls back to CPU; say so
                // where the silent upstream path never did.
                log::warn!(
                    "vulkan: backend failed to initialize (missing/old Vulkan driver?) — transcription runs on CPU"
                );
                return;
            }
            sys::ggml_backend_register(vk);
            log::info!(
                "vulkan: backend registered explicitly (the ggml registry constructor had silently skipped it — whisper.cpp#3750)"
            );
        }
        // Diagnostics, now that the instance is known-good: name the
        // devices whisper can pick from (GAP-62's first question after a
        // driver fault is "which GPU").
        let devices = whisper_rs::vulkan::list_devices();
        if devices.is_empty() {
            log::info!("vulkan: backend registered but no devices found — whisper falls back to CPU");
        }
        for d in &devices {
            log::info!(
                "vulkan: device {}: {} ({} MiB free / {} MiB total)",
                d.id,
                d.name,
                d.vram.free / (1024 * 1024),
                d.vram.total / (1024 * 1024)
            );
        }
    });
}

impl WhisperTranscriber {
    /// `use_gpu` maps to whisper.cpp's context flag. whisper-rs's
    /// `WhisperContextParameters::default()` sets
    /// `use_gpu: cfg!(feature = "_gpu")` (`src/whisper_ctx.rs:476`) — FALSE
    /// on a CPU-only build, but TRUE on a Vulkan build (the `vulkan`
    /// feature enables `_gpu`), so a shipped GPU build already defaults to
    /// "on". The explicit `.use_gpu(use_gpu)` set below still matters for
    /// BOTH directions, but especially OFF: without it, the "Use GPU
    /// (Vulkan)" toggle's off position would be a silent no-op on a
    /// shipped build — the context would keep defaulting to GPU regardless
    /// of what the user picked. On a CPU-only build the flag is inert (no
    /// GPU backend is compiled in), and on a Vulkan build with no usable
    /// device whisper.cpp falls back to CPU at context init — its own log
    /// line (routed through install_logging_hooks) is the audit trail;
    /// deliberately NO per-transcript device claim (the VAD stats-row
    /// lesson: never record intent as engagement).
    pub fn load(model_path: &Path, use_gpu: bool) -> Result<Self, String> {
        // Gated on the toggle on purpose: with GPU off we must not touch
        // Vulkan at all — instance creation can itself crash a broken
        // driver, and the toggle is exactly the escape hatch for that
        // machine (GAP-62).
        #[cfg(feature = "whisper-vulkan")]
        if use_gpu {
            ensure_vulkan_backend_registered();
        }
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        // Pass the `&Path` straight through rather than round-tripping via
        // `to_string_lossy()`: `WhisperContext::new_with_params` takes it by
        // `AsRef<Path>` and converts with its own `path_to_bytes` internally,
        // so this avoids a lossy UTF-8 conversion for no benefit.
        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| format!("load model {}: {e}", model_path.display()))?;
        Ok(Self { ctx })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &self,
        samples: &[f32],
        opts: &crate::EngineOptions,
        cancel: &CancelToken,
        on_progress: Box<dyn FnMut(i32) + Send>,
    ) -> Result<(Vec<Segment>, bool), String> {
        // VAD (optional): filter non-speech out of `samples` before paying
        // for inference. Reimplemented in Rust via crate::vad rather than
        // FullParams' enable_vad/set_vad_model_path/set_vad_params, which
        // are DEAD on this call path — state.full() below calls
        // whisper_full_with_state, and (verified against the vendored
        // whisper.cpp source) that function never reads params.vad at
        // all; only whisper_full/whisper_full_parallel (the no-state entry
        // points, unreachable from whisper-rs) apply VAD filtering.
        // `vad_engaged` reports what ACTUALLY happened — a detect failure
        // degrades silently to an unfiltered run, so a VAD-enabled vault
        // whose job degraded honestly reports "off" in the transcript's
        // stats footer, not the setting.
        let mut owned_filtered: Option<Vec<f32>> = None;
        let mut vad_map: Option<Vec<crate::vad::SpanMap>> = None;
        let mut vad_engaged = false;
        if let Some(vad_model) = opts.vad_model {
            match crate::vad::detect_speech_centiseconds(vad_model, samples) {
                Err(e) => {
                    log::warn!(
                        "vad: speech detection failed ({e}) for {}; transcribing unfiltered audio",
                        vad_model.display()
                    );
                    // Self-heal, mirroring the corrupt-main-model posture: a
                    // cached silero that fails detection is likely
                    // corrupt/incompatible, and ensure_vad_model returns any
                    // existing file unchecked — leaving it means every
                    // future job silently degrades until the user
                    // hand-deletes it. Removal is best-effort (a locked file
                    // just retries next time); the ~1 MB redownload is
                    // cheap and SHA-verified. A transient (non-file) detect
                    // failure costs one spurious redownload — accepted.
                    if let Err(rm) = std::fs::remove_file(vad_model) {
                        log::warn!(
                            "vad: could not remove failing model {}: {rm}",
                            vad_model.display()
                        );
                    }
                }
                Ok(segs) => {
                    let spans = crate::vad::spans_from_centiseconds(&segs, samples.len());
                    if spans.is_empty() {
                        // All-silence: nothing for whisper to do. The
                        // existing zero-segment path already renders "No
                        // speech detected" — return before paying for a
                        // state/full() call at all.
                        return Ok((Vec::new(), true));
                    }
                    if crate::vad::spans_cover_all(&spans, samples.len()) {
                        // Full-buffer speech (Codex P2): VAD ran and found
                        // nothing to trim. Skip filter_samples entirely —
                        // copying `samples` into an identical `filtered`
                        // buffer would allocate a SECOND full-length f32
                        // buffer (~230 MB/hour of 16 kHz mono audio),
                        // doubling peak memory during inference for zero
                        // benefit on a long meeting. `owned_filtered`/
                        // `vad_map` stay None, so `run_samples` below falls
                        // through to `samples` directly and the timestamp
                        // remap step is skipped — correct, since with no
                        // filtering the whisper output is already on the
                        // original timeline. `vad_engaged` still reports
                        // true: VAD ran, it just removed nothing.
                        vad_engaged = true;
                    } else {
                        let (filtered, map) = crate::vad::filter_samples(samples, &spans);
                        owned_filtered = Some(filtered);
                        vad_map = Some(map);
                        vad_engaged = true;
                    }
                }
            }
        }
        let run_samples: &[f32] = owned_filtered.as_deref().unwrap_or(samples);

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
        if let Some(lang) = opts.language {
            // NOTE: `set_language` leaks a small `CString` per job — whisper-rs
            // `CString::into_raw()`s the language and `FullParams` has no `Drop`
            // that reclaims `fp.language`. Unlike the progress closure (an
            // `AppHandle` + `PathBuf`, owned/freed above), this is a bounded
            // few-bytes-per-job upstream leak (a 2-letter code + NUL), and it is
            // UNFIXABLE from here: the only public setter is this leaking one,
            // and `FullParams::fp` is `pub(crate)`, so we cannot point the C
            // struct at a `CString` we own without transferring ownership to it.
            // Left as-is deliberately; revisit if whisper-rs adds a non-leaking
            // language API.
            params.set_language(Some(lang));
        }
        if let Some(prompt) = opts.initial_prompt {
            // NOTE: same bounded upstream leak class as `set_language` above —
            // whisper-rs `CString::into_raw()`s the prompt and `FullParams`
            // has no `Drop` reclaiming it. A prompt is a title plus a short
            // vocabulary line, a few hundred bytes per job at most; accepted
            // for the same pub(crate) reason documented on set_language.
            params.set_initial_prompt(prompt);
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
        // Wire progress the same hand-rolled way, for a DIFFERENT reason:
        // whisper-rs's `set_progress_callback_safe` is type-correct but LEAKS —
        // it `Box::into_raw`s the closure and `FullParams` has no `Drop` freeing
        // `progress_callback_user_data`. Our progress closure captures an
        // `AppHandle` + `PathBuf`, so every job would leak that permanently.
        // `wire_progress_callback` OWNS the closure in `_progress` and frees it
        // when the guard drops after `full()`. Like `_abort` it must outlive the
        // `full()` call below — do NOT drop or `let _ =` it.
        let _progress = wire_progress_callback(&mut params, on_progress);
        state.full(params, run_samples).map_err(|e| {
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
            // `run_samples.len()` (not `samples.len()`) so a VAD-filtered run
            // logs what whisper ACTUALLY chewed on.
            log::error!(
                "whisper inference failed: code={code:?} n_threads={n_threads} cancelled={} samples={} (~{}s audio)",
                cancel.is_cancelled(),
                run_samples.len(),
                run_samples.len() / crate::decode::WHISPER_RATE as usize
            );
            crate::inference_failure_message(code, &raw)
        })?;

        // whisper-rs 0.16: iterate WhisperSegment objects via state.as_iter();
        // timestamps are in centiseconds, converted to ms below (×10) and,
        // when VAD filtered the input, remapped from the filtered timeline
        // back to the original one via `vad_map`. t0 and t1 use different
        // `TimestampKind`s: an exact span-boundary tie means "speech
        // resumed" for a start but "speech stopped" for an end, and using
        // the same rule for both would render a segment that begins right
        // after a VAD-collapsed gap as starting a whole gap too early.
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
            let t0_ms = segment.start_timestamp().max(0) as u64 * 10;
            let t1_ms = segment.end_timestamp().max(0) as u64 * 10;
            let (start_ms, end_ms) = match &vad_map {
                Some(map) => (
                    crate::vad::remap_ms(t0_ms, map, crate::vad::TimestampKind::Start),
                    crate::vad::remap_ms(t1_ms, map, crate::vad::TimestampKind::End),
                ),
                None => (t0_ms, t1_ms),
            };
            out.push(Segment {
                start_ms,
                end_ms,
                text,
            });
        }
        Ok((out, vad_engaged))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression for the silent-CPU GPU build (upstream whisper.cpp#3750):
    // the guard must (a) never unwind a Vulkan exception across the FFI
    // boundary — on a machine with a loader but no usable device/ICD (this
    // CI runner, headless Windows runners) instance init throws inside C++
    // and must be swallowed there, not abort the process — and (b) never
    // register the Vulkan backend twice: on a healthy-GPU machine the ggml
    // registry's own constructor may have already registered it, and on any
    // machine a repeat call must be a no-op. Runs wherever the
    // `whisper-vulkan` feature compiles (Windows CI; Linux with the SDK).
    #[cfg(feature = "whisper-vulkan")]
    #[test]
    fn vulkan_registration_guard_never_aborts_and_never_duplicates() {
        ensure_vulkan_backend_registered();
        ensure_vulkan_backend_registered();
        let vk_regs = unsafe {
            use whisper_rs::whisper_rs_sys as sys;
            (0..sys::ggml_backend_reg_count())
                .filter(|&i| {
                    let reg = sys::ggml_backend_reg_get(i);
                    !reg.is_null()
                        && std::ffi::CStr::from_ptr(sys::ggml_backend_reg_name(reg)).to_bytes()
                            == b"Vulkan"
                })
                .count()
        };
        assert!(
            vk_regs <= 1,
            "Vulkan backend registered {vk_regs} times — the guard must dedupe by name"
        );
    }

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

    // Regression: the whole reason this crate hand-wires the abort callback is
    // that whisper-rs 0.16's set_abort_callback_safe read a garbage byte as the
    // abort bool and made whisper.cpp abort every encode window with
    // -6 "failed to encode". This is the end-to-end guard: with a real model it
    // must run full() to Ok, not abort. #[ignore] because it needs a ~150 MB
    // model that CI cannot host; a Windows dev runs it with `-- --ignored`.
    // Provide VB_TEST_MODEL (a ggml .bin) and optionally VB_TEST_AUDIO (a speech
    // clip); with no model it skips (passes) rather than failing spuriously.
    #[test]
    #[ignore]
    fn real_model_transcribes_without_spurious_abort() {
        use crate::decode::decode_to_16k_mono;
        use crate::{CancelToken, Transcriber};
        let model = std::env::var("VB_TEST_MODEL")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| crate::model::model_path(crate::model::ModelTier::Base));
        let Some(model) = model.filter(|p| p.exists()) else {
            eprintln!("skipping: no VB_TEST_MODEL and no cached base model");
            return;
        };
        let cancel = CancelToken::new();
        // A short synthetic 16 kHz tone: enough to reach the encoder (where the
        // -6 fired), even if it yields no speech segments.
        let samples: Vec<f32> = if let Ok(audio) = std::env::var("VB_TEST_AUDIO") {
            decode_to_16k_mono(std::path::Path::new(&audio), &cancel).expect("decode VB_TEST_AUDIO")
        } else {
            (0..16_000)
                .map(|i| (i as f32 / 16_000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.2)
                .collect()
        };
        // VB_TEST_GPU=1 exercises the GPU path on a manual Windows run.
        let t = WhisperTranscriber::load(&model, std::env::var("VB_TEST_GPU").is_ok())
            .expect("load model");
        // Optional priming/VAD paths for a manual (Windows dev / local) run:
        // VB_TEST_VOCAB primes the prompt; VAD engages when the silero model
        // is already cached. Both default off so the test's original -6
        // regression coverage is unchanged. This automated run can't tell a
        // VAD-on pass apart from a VAD-off one by assertion alone (both must
        // simply not abort/error, and — VAD being all-or-nothing on
        // whether the cached silero model exists — a single run can't
        // observe both states anyway): a manual comparison, run once with
        // the silero model cached and once without on the SAME
        // VB_TEST_AUDIO clip (ideally one with an audible quiet stretch),
        // should observe fewer-or-equal segments and a shorter wall-clock
        // inference with VAD on than off, since the filtered buffer
        // whisper actually chews on is shorter.
        let vocab = std::env::var("VB_TEST_VOCAB").ok();
        let vad = crate::model::vad_model_path().filter(|p| p.exists());
        let opts = crate::EngineOptions {
            language: None,
            initial_prompt: vocab.as_deref(),
            vad_model: vad.as_deref(),
        };
        let out = t.transcribe(&samples, &opts, &cancel, Box::new(|_| {}));
        assert!(
            out.is_ok(),
            "fixed engine must not abort at the first encode window (the -6 bug): {}",
            out.err().unwrap_or_default()
        );
        if std::env::var("VB_TEST_AUDIO").is_ok() {
            let (segments, vad_engaged) = out.unwrap();
            eprintln!("vad_engaged={vad_engaged}");
            assert!(
                !segments.is_empty(),
                "a real speech clip must yield at least one segment"
            );
            // With or without VAD, timestamps must stay on the original
            // timeline: monotonically non-decreasing starts, end >= start.
            for w in segments.windows(2) {
                assert!(
                    w[0].start_ms <= w[1].start_ms,
                    "segment starts out of order"
                );
            }
            assert!(segments.iter().all(|s| s.end_ms >= s.start_ms));
        }
    }

    // Regression / correctness gate for the leak fix: whisper-rs 0.16's
    // `set_progress_callback_safe` is type-correct but LEAKS — it
    // `Box::into_raw`s the closure and `FullParams` has no `Drop` reclaiming
    // `progress_callback_user_data`, so every job leaks the closure's captures
    // (an AppHandle + PathBuf) forever. `wire_progress_callback` owns the
    // closure instead, and to keep `user_data` a THIN pointer it DOUBLE-boxes
    // (a `&mut dyn FnMut` is a fat pointer that would drop its vtable when cast
    // to `*mut c_void` — the same fat/thin mismatch that broke abort). This
    // test proves the installed C trampoline recovers the REAL closure through
    // `user_data` and forwards the value — not garbage — by invoking it
    // directly with a sentinel and null ctx/state (the trampoline never
    // dereferences those), needing no model or audio.
    #[test]
    fn wired_progress_callback_invokes_real_closure() {
        use std::sync::atomic::{AtomicI32, Ordering};
        use std::sync::Arc;

        let seen = Arc::new(AtomicI32::new(-1));
        let seen_cb = Arc::clone(&seen);
        let on_progress: Box<dyn FnMut(i32) + Send> =
            Box::new(move |p: i32| seen_cb.store(p, Ordering::SeqCst));

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // Installs the callback into `params` AND hands back the exact C
        // function pointer + user_data whisper.cpp will call. The guard owns
        // the (double-boxed) closure and must outlive the callback.
        let wiring = wire_progress_callback(&mut params, on_progress);

        // Sentinel forwarded verbatim proves the closure — not a fat/thin
        // pointer garbage read — ran. Null ctx/state are safe: the trampoline
        // ignores them.
        unsafe {
            (wiring.callback)(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                42,
                wiring.user_data,
            );
        }
        assert_eq!(
            seen.load(Ordering::SeqCst),
            42,
            "the trampoline must invoke the real closure with the forwarded progress value"
        );
    }
}
