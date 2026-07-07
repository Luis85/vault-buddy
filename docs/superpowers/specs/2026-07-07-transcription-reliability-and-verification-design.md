# Design — Transcription reliability, verification & backend hardening

- **Date:** 2026-07-07
- **Status:** Approved
- **Source:** Follow-up to the abort-callback fix
  (`fix(transcribe): wire whisper abort callback correctly`, commit `dc6b214`)
  and the hardening pass (`fix(transcribe): harden download, decode, FFI, and
  worker paths`, commit `4c4718e`). Those landed in the repo but have **never
  run on a real build** — see the finding below. This increment proves the
  fixed engine works end-to-end, then hardens the transcription backend for
  reliability, logging, and error handling. Extends
  [Increment 3](2026-07-04-increment-3-local-speech-to-text-design.md) and
  [Increment 4](2026-07-06-transcription-control-and-progress-design.md).

## The finding that anchors this work

The running app on the developer machine is **v0.4.2** (`vault-buddy.log`,
`Vault Buddy v0.4.2 starting`), which is commit `a91cb82` — **before** the
abort fix (`dc6b214`). The log shows every transcription still failing with the
old Undefined-Behavior signature:

```
16:26:58  transcribe: inference start …2026-07-07 0901 Meeting.mp3 (2922s audio)
16:26:59  whisper: transcribing with 8 threads
16:27:17  whisper_full_with_state: failed to encode
16:27:17  whisper inference failed: code=Some(-6) n_threads=8 cancelled=false samples=46767543 (~2922s audio)
```

**What it proves.** The `-6` fires ~18 s into a **2922 s (49-minute)**
recording — i.e. whisper aborts at the *first* encode window — with
`cancelled=false`, `n_threads=8` (already capped, so not the thread-count
suspect) on a 64 GB machine (so not OOM). This is exactly the abort-UB the
committed fix targets: whisper-rs 0.16's `set_abort_callback_safe` reads a
garbage byte as the abort bool.

**What it does not prove.** That the *fixed* build actually produces a
transcript. The fix is unit-tested at the FFI level
(`wired_abort_callback_reflects_token_not_garbage`) but has **never** loaded a
real model and transcribed real audio. **No transcription has ever succeeded on
this machine.** Closing that evidence gap is this increment's first job.

## Goal

Make local transcription **provably reliable**: prove the fixed engine
transcribes a real recording end-to-end, then harden the transcription backend
so it consistently completes, never loses the worker to a bad job, never caches
a corrupt model, never silently drops a write, and logs every outcome. Improve
the code structure of the area we touch. No change to the audio-quality path or
the never-clobber write contract.

## Scope

### In scope

1. **End-to-end verification** — a committed dev harness that transcribes a real
   file with the fixed engine (run here for ground truth), plus a durable
   Windows-only regression test that guards the `-6` abort.
2. **Module extraction** — move the transcription orchestration out of the
   ~1740-line `capture_commands.rs` into its own shell module; move pure
   decision-logic into `vault_buddy_core` with tests.
3. **Reliability hardening** — empty/no-speech transcript handling, a
   crash-proof worker loop, and SHA-256 integrity verification of downloaded
   models.
4. **Error handling & logging** — stop swallowing sidecar-write failures; one
   consistent, structured outcome log line per job (with decode vs inference
   timing).
5. **Dependency review** — a written review of every crate in the transcription
   path (no upgrades this increment).

### Out of scope (deferred)

- **The audio-quality path.** No change to resampling (linear, no anti-alias
  filter) or the stereo→mono downmix. "Reliability first" — accuracy is
  whisper's job unless the path is actively wrong.
- **Upgrading `whisper-rs`/`whisper-rs-sys`.** We just stabilized the engine
  with hand-rolled FFI wiring; an upgrade risks reintroducing the `-6`. Reviewed
  and documented here; a bump is its own future increment.
- **Chunked/windowed inference for long recordings.** The 49-min clip holds
  ~187 MB of PCM; a multi-hour clip is larger but well within 64 GB. Bounding
  peak memory via windowing is a separate design.
- **Parallel workers / GPU.** Still one worker, one job at a time.
- **Any frontend change.** The events and DTOs are unchanged; this is a backend
  increment.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Prove the fix | A committed `examples/transcribe_file.rs` (print-only) run here on real recordings, **plus** an `#[ignore]` Windows-only regression test | Ground truth in minutes without a GUI/installer build; the test is the durable `-6` guard. Print-only means the diagnostic never writes a vault. |
| Verification gate | If the harness produces no text, **stop** and switch to systematic-debugging | The whole hardening effort assumes the fix works; confirm the premise before building on it. |
| Transcription module | Extract into `src-tauri/src/transcription.rs`; shared shell helpers become `pub(crate)` | `capture_commands.rs` mixes capture + recovery + transcription; the transcription block is cohesive and reads/testable better alone. |
| Pure logic to core | A small `core::throttle::EmitThrottle` (the emit/log throttles) | Currently inline closure state in `process_transcription` — untestable in the shell. Pure + unit-tested on Linux. |
| No-speech result | Zero segments → still a `complete` sidecar with an explicit "_No speech detected._" body + a `warn` log | whisper legitimately returns no segments for silence; a blank `complete` file looks broken. It's a success-with-notice, not a scary failure. |
| Worker resilience | Wrap each job in `catch_unwind`; a panicking job fails only itself, drops the cached model, clears `active`, and the worker survives | A worker-thread panic today silently stops **all** future transcriptions until relaunch. |
| Model integrity | Pin an expected SHA-256 per `ModelTier`; hash the stream during download; reject + delete on mismatch (retryable) | A complete-but-corrupt download passes the length check, fails to load, and the self-heal re-downloads the same bytes — a possible loop. A checksum is decisive. |
| Hash source | `base` derived from the on-disk `ggml-base.bin`; `small` from Hugging Face `ggerganov/whisper.cpp`, both re-verified during implementation | The ggml `.bin` files are served raw (no content-encoding), so the published hash equals the bytes we write. |
| Swallowed writes | Every `let _ = transcript::…write…` on the cancel/fail/placeholder/success paths logs `warn` on `Err` | The sidecar is the note's source of truth; a silent write failure makes the embed lie (violates the "no swallowed error" invariant). |
| whisper-rs version | **Stay pinned** (0.16 / -sys 0.15) | See dependency review; the FFI workarounds are proven and tested. |

## Architecture

Honors the repo's "what compiles where" split: pure logic in
`vault_buddy_core` (Linux-tested), engine wiring behind the `whisper` feature
(Windows CI gate), thin shell wiring. No frontend change.

### `vault_buddy_transcribe` — verification + integrity

**Verification harness (`examples/transcribe_file.rs`, `whisper` feature).**
`cargo run -p vault_buddy_transcribe --features whisper --example transcribe_file -- <mp3> [base|small]`.
Resolves the model from the standard cache (`model_path(tier)`) or a
`VB_TEST_MODEL` override, decodes via `decode_to_16k_mono`, runs
`WhisperTranscriber`, and **prints** segment count + text + timing. It never
writes a sidecar — a pure read/inference diagnostic. Committed because it is a
genuinely useful support/QA tool; it only builds on demand behind the feature.

**Regression test (`engine.rs`, `whisper` feature, `#[ignore]`).**
`real_model_transcribes_without_spurious_abort`: loads a real model (cache or
`VB_TEST_MODEL`), transcribes a short signal, and asserts `transcribe()`
returns **`Ok`** (not `Err` carrying `-6`). When `VB_TEST_AUDIO` points at a
real speech clip it additionally asserts ≥1 non-empty segment. Skips cleanly
(passing, with a logged reason) when no model is present, so it is safe in any
suite; a Windows dev runs it with `-- --ignored`. Names the failure mode in a
comment (the abort UB).

**Model integrity (`model.rs`).** `ModelTier` gains
`fn sha256(&self) -> &'static str`. `download_stream` feeds each written chunk
into a `sha2::Sha256`; after the existing completeness checks it compares the
hex digest to the expected value and, on mismatch, deletes the `.part` and
returns a retryable `Err` naming the mismatch. Hashing during the write avoids a
second pass over ~150 MB. New dependency: `sha2` (pure Rust, widely used). Tests
extend the M1/M2a injected-`ureq::Agent` harness: a tiny payload with its known
hash succeeds; a wrong expected hash is rejected and leaves no `.part`/dest.

### `vault_buddy_core` — pure logic + no-speech render

- **`throttle::EmitThrottle`** — a tiny value+delta gate:
  `should_emit(value, terminal) -> bool` firing when `value` advanced by
  ≥ `min_delta` since the last emit, or `terminal` is true. Replaces the two
  hand-inlined throttles (download bytes ≥ 4 MB; progress percent ≥ 5 / ≥ 25 for
  the log). Unit-tested (monotonic, terminal, first-call behavior).
- **`transcript::render_transcript`** — when `segments` is empty, render a body
  of "_No speech detected._" under the same frontmatter; status stays
  `complete`. Unit-tested (`empty_segments_render_a_no_speech_body_not_blank`).

### `vault_buddy_transcribe::lib` — orchestration

`transcribe_recording` logs a `warn` and keeps `complete` when segments are
empty (the render change above makes the body honest), and splits the timing it
already measures into `decode_secs` + `inference_secs` for the completion log.
No signature change.

### Tauri shell — `transcription.rs` (new module)

Move out of `capture_commands.rs` (mechanical; behavior-preserving):
`TranscriptionJob`, `Phase`, `ActiveJob`, `TranscriptionQueue`, `Enqueued`,
`TranscriptionState`; `enqueue_transcription`, `scan_and_enqueue`, `set_phase`,
`process_transcription`, `ensure_model`, `emit_cancelled`, `fail_transcription`,
`owning_vault_id`, `run_transcription` (the worker), `cancel_transcription`; the
commands `transcribe_recording_now`, `retranscribe`, `transcription_queue_status`
and their DTOs. `lib.rs` gains `mod transcription;` and the command/`manage`/
startup paths update from `capture_commands::` to `transcription::`. Shared
helpers `is_recording`, `now_ms`, `toast` become `pub(crate)` in their current
home and are imported (transcription genuinely depends on capture's
`is_recording` gate).

**Crash-proof worker.** The per-job body becomes:

```rust
let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    process_transcription(&app, &job, &mut loaded)
}));
if result.is_err() {
    // A panicking job must not kill the worker (that silently stops ALL
    // future transcriptions). Log, fail just this job, drop the cached
    // model (it may be mid-load / inconsistent), and continue.
    log::error!("transcribe: worker caught a panic on {}", job.mp3.display());
    *loaded = None;
    fail_transcription(&app, &job.mp3, "internal error during transcription");
}
// active cleared as today
```

The seam that runs one job under `catch_unwind` is factored so a unit test can
drive it with a panicking closure and assert the loop would continue.

**No swallowed writes.** `emit_cancelled`, `fail_transcription`, the forced
placeholder write, and `write_placeholder` log `warn` on a write `Err`
(path + reason), keeping best-effort behavior but making failure observable.

**Consistent outcome logging.** One structured line per terminal state, same
shape: `transcribe: complete {mp3} — {n} segments, {audio}s audio, decode {d}s + inference {i}s`
and matching `failed` / `cancelled` / `skipped` lines.

## Error handling / invariants (unchanged contract)

- **Never harms audio/note; never clobbers a finished or hand-edited
  transcript.** Every write still goes through the atomic collision-safe writer
  and `replace_if_ours` / `force_write_sidecar`; the no-speech and no-swallow
  changes only affect *our own* regenerable sidecar and the log.
- **No new deadlock surface.** The queue mutex is still never held across
  download/load/inference; `catch_unwind` wraps a call that already takes the
  mutex only briefly. Locks stay poison-safe (`lock_ignoring_poison`).
- **Integrity failure is retryable, not sticky.** A hash mismatch deletes the
  file and returns `Err`; the job fails (`failed` sidecar) and a later scan or
  manual retry re-downloads — same self-heal contract, now decisive.
- **The verification harness is read-only.** It prints; it never writes a vault.
- **Diagnostics invariants upheld.** The worker thread stays named; no error is
  swallowed; the `whisper` engine still routes native logs via
  `install_logging_hooks`.

## Testing

Same split as prior increments.

- **`vault_buddy_core` (Linux CI):** `EmitThrottle` (delta / terminal / first
  call); `render_transcript` empty-segments body; existing transcript tests
  unchanged.
- **`vault_buddy_transcribe` default (Linux CI):** SHA-256 accept/reject via the
  injected `ureq::Agent`; existing decode/model/orchestration tests unchanged.
- **`vault_buddy_transcribe` `whisper` (Windows gate):** existing FFI wiring
  tests; the new `#[ignore]` real-model regression test (skips without a model).
- **Shell (Windows gate):** the one-job-under-`catch_unwind` seam continues on a
  panicking closure; existing dedup/`enqueue` tests unchanged; the module move
  keeps every test green.
- **Full gate (run on Windows):** `cargo fmt --check`; clippy `-D warnings` for
  core, transcribe (default **and** `--features whisper`), and the shell; all
  Rust tests; `vue-tsc` + Vitest (unchanged, must stay green).
- **Empirical ground truth (this session):** run the harness on the short voice
  notes (12–23 s) then the 49-min meeting; confirm real text.

## Increment ordering (for the plan)

1. **Verify** — harness + regression test; run it; **confirm the fix** (gate).
2. **Extract** — transcription module + `EmitThrottle` to core (behavior-preserving).
3. **Harden** — no-speech render, crash-proof worker, SHA-256 integrity.
4. **Error/logging** — no-swallow writes, consistent outcome line, decode/inference timing.
5. **Dep review** — write the appendix below into final form.

Front-loads the "does the fix even work?" risk before investing in structure and
hardening.

## Success criteria

1. The fixed engine transcribes a real recording end-to-end (proven by the
   harness here and guarded by the regression test), with no `-6` abort.
2. A recording with no speech yields a `complete` transcript that reads
   "No speech detected," not a blank file, and logs a warning.
3. A job that panics fails only itself; the worker keeps processing subsequent
   jobs.
4. A corrupt/truncated model download is rejected by SHA-256 and never cached;
   the next attempt re-downloads.
5. No sidecar-write failure is swallowed — each logs a warning; the log records
   one consistent outcome line per job with decode/inference timing.
6. The transcription orchestration lives in its own module; the extracted pure
   logic is unit-tested in `vault_buddy_core`.
7. The full Windows gate (fmt, clippy default+whisper+shell, all Rust tests,
   `vue-tsc` + Vitest) passes.

## Appendix — dependency review (transcription path)

No upgrades this increment; recorded so "review every package" is auditable.

| Crate | Version | Role | Assessment |
| --- | --- | --- | --- |
| `whisper-rs` | 0.16 | whisper.cpp bindings | **Pinned.** Our `engine.rs` hand-wires the abort + progress callbacks around upstream bug #277 (abort UB) and the progress/language closure leaks. A future release that fixes these would let us delete `wire_abort_callback` / `wire_progress_callback` and use the safe setters. Re-evaluate as its own increment — not now, having just stabilized it. |
| `whisper-rs-sys` | 0.15 | vendored whisper.cpp (~v1.8.x), static-linked | **Pinned** with the above. Windows CI compile gate. |
| `symphonia` | 0.5 (`default-features = false`, `["mp3"]`) | MP3 → PCM decode | Healthy; MP3 feature only. No change. |
| `ureq` | 2 | model download HTTP | Timeouts + completeness added last pass. `sha2` now closes the corrupt-body gap. No change. |
| `sha2` | new (latest 0.10.x) | model integrity | Add. Pure Rust, ubiquitous, no build complications. |
| `mp3lame-encoder` | 0.2 (dev-dependency) | encodes test MP3s | Test-only in transcribe; the reserve-before-encode SIGSEGV caveat is already documented. No change. |
| `log` | 0.4 | logging facade | No change. |
