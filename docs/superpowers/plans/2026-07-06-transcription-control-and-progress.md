# Transcription Control & Progress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make background transcription observable and steerable — cancel a running/queued job (sticky, no auto-retry), a Transcriptions progress view with a real download-then-inference progress bar, a clean model-download→inference handover, transcription settings on the Record view, and honest inference logging — with no change to the engine, the write path, or the never-clobber contract.

**Architecture:** Core gains a fourth `cancelled` transcript marker (automatically non-regenerable, so the startup scan never re-queues it). The `transcribe` crate grows a `CancelToken` and threads cancel + progress hooks through the `Transcriber` trait into whisper-rs's `set_abort_callback_safe` / `set_progress_callback_safe`. The shell worker tracks an `ActiveJob` with an observable `Phase` and lock-free `AtomicU8` progress, emits every transition, and exposes `cancel_transcription` + `transcription_queue_status`. The frontend replaces the scattered singular transcription fields with one backend-seeded per-job map, adds a `Transcriptions` view + compact summary, and a shared controlled `TranscriptionSettings` component used by both the Record view and vault settings.

**Tech Stack:** Rust (`vault_buddy_core`, `vault_buddy_transcribe` — Linux-tested; `vault-buddy` shell + `engine.rs` — Windows-only compile, CI `windows-app` gate; whisper-rs 0.16), Vue 3 + Pinia + Tailwind 4, Vitest.

## Global Constraints

- **whisper-rs 0.16 callback contract (verified against the pinned source):** `FullParams::set_abort_callback_safe<O,F> where F: FnMut() -> bool + 'static` — returning **true aborts** `full()` (an aborted `full()` returns `Err`). `FullParams::set_progress_callback_safe<O,F> where F: FnMut(i32) + 'static` — the `i32` is progress **percent 0–100**. Both closures are `'static`: clone the token's `Arc` into the abort closure (owned → `'static`); pass `on_progress` **by value** as `Box<dyn FnMut(i32) + Send>` (a borrowed sink cannot be `'static`).
- **Never-clobber, unchanged.** Cancel and every new path leave the `.mp3` and companion `.md` untouched. The `cancelled` sidecar is written **only over our own placeholder** via the existing atomic collision-safe writer (`force_write_sidecar`) — never a `complete` or user-edited file. Auto/recovery paths keep `write_placeholder` + `replace_if_ours`.
- **Sticky cancel = non-regenerable marker.** `is_regenerable()` matches only `pending`/`failed`, so a `cancelled` marker is automatically non-regenerable: `needs_transcription()` → `pending_transcriptions()` (the startup scan) skips it, so it is never auto-re-queued. Force re-transcribe still overwrites it via `force_write_sidecar`.
- **No new deadlock surface.** The queue mutex is **never** held across inference. Inference progress is lock-free (`AtomicU8`). The whisper progress callback runs on the FFI thread: it writes the atomic and `app.emit`s a **throttled** `capture:transcribeProgress` (throttle via a captured last-sent %); it must never take the queue mutex or touch window state (`AppHandle` is `Send+Sync`, so the emit is safe). `cancel_transcription` / `transcription_queue_status` take the mutex only briefly — **cancel writes the `cancelled` sidecar AFTER releasing the mutex** (an fsync must not run under the lock).
- **Cancel can't wedge shutdown.** Quit never waits on the transcription worker; a cancelled/killed job leaves a sidecar the next launch handles per its marker (`cancelled` → left alone; `pending`/`failed` → re-queued).
- **Command naming.** The new shell command is `transcription_queue_status` (the live queue picture) — deliberately distinct from core's `transcript_status()` (the per-sidecar marker classifier) and the existing `capture_status`.
- **Compiles-where split.** `vault_buddy_core` + `vault_buddy_transcribe` orchestration/fakes test on Linux; `engine.rs` (whisper wiring) and the shell compile on Windows only (CI `windows-app` gate). Mirror existing patterns exactly; run `cargo fmt --check`.
- **Every spawned thread is named** (diagnostics invariant): the transcription worker becomes `transcribe-worker` (`std::thread::Builder`), replacing today's bare `std::thread::spawn`.
- **Push bracket:** Task 1 (core marker) is self-contained — **push alone**. Task 2 changes the `Transcriber::transcribe` signature and `transcribe_recording`'s signature/return type, which breaks `engine.rs` and the Windows shell until Tasks 3–4 update them. **HOLD Tasks 2, 3, 4; push 2+3+4 together** so the `windows-app` job never sees a broken intermediate (same discipline as increments 2/3). Frontend Tasks 5–12 each keep `npm run build` + `npm test` green — **push each**.
- **Commit scopes:** `feat(core)`, `feat(transcribe)`, `feat(shell)`, `feat(ui)`.
- **Verification commands** — Rust (from `src-tauri/`): `cargo test -p vault_buddy_core`, `cargo test -p vault_buddy_transcribe`, `cargo fmt --check`, `cargo clippy -p vault_buddy_core -p vault_buddy_transcribe --all-targets -- -D warnings`. Frontend (repo root): `npx vitest run tests/<file>`, `npm test`, `npm run build`.

---

### Task 1: `cancelled` transcript state (core)

**Files:**
- Modify: `src-tauri/core/src/transcript.rs` (add `MARKER_CANCELLED`, `render_cancelled`, `TranscriptStatus::Cancelled`, classify it, tests)

**Interfaces:**
- Consumes: existing `MARKER_PENDING`/`MARKER_FAILED`/`MARKER_COMPLETE`, `yaml_quote`, `transcript_path`, `render_error`, `is_regenerable`, `needs_transcription`, `pending_transcriptions`, `write_placeholder`.
- Produces: `pub fn render_cancelled(mp3_file_name: &str) -> String`; `TranscriptStatus::Cancelled` with `as_dto_str()` → `"cancelled"`; `transcript_status()` returns `Cancelled` for the new marker.

- [ ] **Step 1: Write the failing tests**

In `#[cfg(test)] mod tests` of `transcript.rs`:

```rust
    #[test]
    fn cancelled_marker_is_not_regenerable_and_classifies() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-06 0930 Meeting.mp3");
        let c = render_cancelled("2026-07-06 0930 Meeting.mp3");
        assert!(c.contains("vault-buddy-transcript: cancelled"));
        assert!(c.contains(r#"transcript-of: "2026-07-06 0930 Meeting.mp3""#));
        assert!(!is_regenerable(&c), "cancelled must never be auto-re-queued");
        std::fs::write(transcript_path(&mp3), &c).unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Cancelled);
        assert_eq!(TranscriptStatus::Cancelled.as_dto_str(), "cancelled");
        assert!(!needs_transcription(&mp3), "cancelled sidecar is not work to do");
    }

    #[test]
    fn scan_skips_a_cancelled_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let mp3 = month.join("2026-07-06 0930 Meeting.mp3");
        std::fs::write(&mp3, b"audio").unwrap();
        std::fs::write(transcript_path(&mp3), render_cancelled("2026-07-06 0930 Meeting.mp3")).unwrap();
        assert!(pending_transcriptions(dir.path()).is_empty(), "a cancelled recording must not backfill");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core transcript`
Expected: FAIL to compile — `render_cancelled` / `TranscriptStatus::Cancelled` undefined.

- [ ] **Step 3: Implement**

In `transcript.rs`:

(a) Beside the other markers:
```rust
const MARKER_CANCELLED: &str = "vault-buddy-transcript: cancelled";
```
(b) Beside `render_error`:
```rust
/// A deliberately-cancelled sidecar. Non-regenerable (like `complete`, unlike
/// `pending`/`failed`), so the startup scan never re-queues it — but a forced
/// re-transcribe overwrites it. Same frontmatter/`yaml_quote` discipline.
pub fn render_cancelled(mp3_file_name: &str) -> String {
    format!(
        "---\n{MARKER_CANCELLED}\ntranscript-of: {}\ncreated-by: Vault Buddy\n---\n\n\
         > [!note] Transcription cancelled\n> Re-transcribe from the Recordings list to run it again.\n",
        yaml_quote(mp3_file_name)
    )
}
```
(c) Add the enum variant + DTO string:
```rust
    Cancelled,
```
and in `as_dto_str`:
```rust
            TranscriptStatus::Cancelled => "cancelled",
```
(d) In `transcript_status`, classify it **before** the `Ok(_) => Complete` fallthrough:
```rust
        Ok(c) if c.contains(MARKER_CANCELLED) => TranscriptStatus::Cancelled,
```
`is_regenerable` is unchanged — it matches only `MARKER_PENDING`/`MARKER_FAILED`, so `cancelled` is non-regenerable automatically.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p vault_buddy_core transcript` → PASS. Then `cargo fmt --check` and `cargo clippy -p vault_buddy_core --all-targets -- -D warnings`.

- [ ] **Step 5: Commit + push (self-contained)**

```bash
git add src-tauri/core/src/transcript.rs
git commit -m "feat(core): cancelled transcript marker (sticky, non-regenerable)"
git push
```

---

### Task 2: `CancelToken` + cancel/progress hooks in the engine (transcribe) — HOLD PUSH

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (add `CancelToken`, `TranscribeError`; new `Transcriber::transcribe` signature; thread through `transcribe_recording`; update fakes + tests)
- Modify: `src-tauri/transcribe/src/engine.rs` (wire both whisper-rs safe callbacks — Windows-compiled)

**Interfaces:**
- Consumes: `Transcriber`, `TranscribeOptions`, `decode::decode_to_16k_mono`, `transcript::{render_transcript, replace_if_ours, force_write_sidecar, transcript_path, ReplaceOutcome}`.
- Produces:
  - `pub struct CancelToken(std::sync::Arc<std::sync::atomic::AtomicBool>)` — `new()`, `cancel(&self)`, `is_cancelled(&self) -> bool`, `#[derive(Clone, Default)]`.
  - `pub enum TranscribeError { Cancelled, Failed(String) }`.
  - `fn Transcriber::transcribe(&self, samples: &[f32], language: Option<&str>, cancel: &CancelToken, on_progress: Box<dyn FnMut(i32) + Send>) -> Result<Vec<Segment>, String>`.
  - `pub fn transcribe_recording(mp3: &Path, transcriber: &dyn Transcriber, opts: &TranscribeOptions, generated_at: &str, force: bool, cancel: &CancelToken, on_progress: Box<dyn FnMut(i32) + Send>) -> Result<PathBuf, TranscribeError>`.

- [ ] **Step 1: Write the failing tests**

In `lib.rs` tests, update the fakes to the new signature and add cancellation coverage:

```rust
    struct FakeOk;
    impl Transcriber for FakeOk {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>, _c: &CancelToken, mut on_progress: Box<dyn FnMut(i32) + Send>) -> Result<Vec<Segment>, String> {
            on_progress(100); // exercises the forwarder
            Ok(vec![Segment { start_ms: 0, end_ms: 1000, text: "hello world".into() }])
        }
    }
    struct FakeErr;
    impl Transcriber for FakeErr {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>, cancel: &CancelToken, _p: Box<dyn FnMut(i32) + Send>) -> Result<Vec<Segment>, String> {
            // Mirrors whisper: an aborted full() returns Err; the token disambiguates.
            if cancel.is_cancelled() { return Err("aborted".into()); }
            Err("boom".into())
        }
    }

    fn noop_progress() -> Box<dyn FnMut(i32) + Send> { Box::new(|_| {}) }

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
        let mp3 = write_tiny_mp3(dir.path()); // existing helper used by transcribe_writes_the_sidecar
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = transcribe_recording(&mp3, &FakeErr, &opts(), "2026-07-06T09:30:00Z", false, &cancel, noop_progress());
        assert!(matches!(r, Err(TranscribeError::Cancelled)));
        assert!(!transcript_path(&mp3).exists() || !std::fs::read_to_string(transcript_path(&mp3)).unwrap().contains("complete"));
    }

    #[test]
    fn failure_is_distinguished_from_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        let r = transcribe_recording(&mp3, &FakeErr, &opts(), "t", false, &CancelToken::new(), noop_progress());
        assert!(matches!(r, Err(TranscribeError::Failed(_))));
    }
```

> Note: reuse whatever tiny-mp3 + `opts()` helpers the existing `transcribe_writes_the_sidecar` test already uses; if it inlines them, extract `write_tiny_mp3`/`opts` as local `fn`s in the test module so all tests share them (DRY).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p vault_buddy_transcribe`
Expected: FAIL to compile — `CancelToken` / `TranscribeError` undefined; trait arity mismatch.

- [ ] **Step 3: Implement**

(a) In `lib.rs`, add the token + error and change the trait:
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A shared abort flag polled by whisper's abort callback and checked between
/// stages. Cloning shares the flag (Arc), so the shell holds one and the
/// engine another.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) { self.0.store(true, Ordering::SeqCst); }
    pub fn is_cancelled(&self) -> bool { self.0.load(Ordering::SeqCst) }
}

/// Cancel and failure are different outcomes: cancel writes a `cancelled`
/// sidecar (no scary toast, no auto-retry); failure writes a retryable `failed`.
pub enum TranscribeError { Cancelled, Failed(String) }

pub trait Transcriber {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
        cancel: &CancelToken,
        on_progress: Box<dyn FnMut(i32) + Send>,
    ) -> Result<Vec<Segment>, String>;
}
```
(b) Rewrite `transcribe_recording`'s tail to thread the token/progress, check after decode, and map the error:
```rust
    let started = std::time::Instant::now(); // (already present today for processing_secs — reuse it)
    let samples = decode::decode_to_16k_mono(mp3).map_err(TranscribeError::Failed)?;
    if cancel.is_cancelled() { return Err(TranscribeError::Cancelled); } // cheap to bail before inference
    let duration_secs = samples.len() as u64 / decode::WHISPER_RATE as u64;
    // Honest logging: the log never goes dark on inference start again.
    log::info!("transcribe: inference start {} ({duration_secs}s audio)", mp3.display());
    let segments = match transcriber.transcribe(&samples, opts.language.as_deref(), cancel, on_progress) {
        Ok(s) => s,
        // An aborted full() returns Err; the token says whether it was us.
        Err(e) => return Err(if cancel.is_cancelled() { TranscribeError::Cancelled } else { TranscribeError::Failed(e) }),
    };
    log::info!("transcribe: inference done {} in {}s", mp3.display(), started.elapsed().as_secs());
    // ...existing meta/render_transcript... (processing_secs = started.elapsed().as_secs()), then the write
    // branch (force ? force_write_sidecar : replace_if_ours) returns Ok(path), mapping io errors to TranscribeError::Failed.
```
Keep the existing `force ? force_write_sidecar : replace_if_ours` write branch; wrap its `io::Error` in `TranscribeError::Failed`.

(c) In `engine.rs`, wire both callbacks (place after `set_print_*`, before `state.full`):
```rust
    fn transcribe(&self, samples: &[f32], language: Option<&str>, cancel: &CancelToken, on_progress: Box<dyn FnMut(i32) + Send>) -> Result<Vec<Segment>, String> {
        // ...existing params setup...
        // Owned clone → 'static abort closure; returning true aborts full().
        let cancel = cancel.clone();
        params.set_abort_callback_safe(move || cancel.is_cancelled());
        // Box<dyn FnMut(i32)+Send> is itself FnMut(i32)+'static — pass by value.
        params.set_progress_callback_safe(on_progress);
        state.full(params, samples).map_err(|e| format!("whisper inference: {e}"))?;
        // ...existing segment iteration...
    }
```
Add `use crate::CancelToken;` (and `Box` is in the prelude). The signature import of `Segment` stays.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p vault_buddy_transcribe` → PASS. `cargo fmt --check`; `cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings`. (Do **not** push — held for the bracket.)

- [ ] **Step 5: Commit (HOLD push)**

```bash
git add src-tauri/transcribe/src/lib.rs src-tauri/transcribe/src/engine.rs
git commit -m "feat(transcribe): CancelToken + abort/progress callbacks; typed cancel error"
# DO NOT PUSH — Tasks 3+4 restore the Windows shell; push 2+3+4 together.
```

---

### Task 3: phase-aware transcription worker (shell) — HOLD PUSH

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (add `Phase`, `ActiveJob`, `TranscriptionQueue.active`; name the worker thread; phase transitions + new events in `process_transcription`; thread `CancelToken` + progress into `transcribe_recording`; `cancelled` sidecar + `capture:transcribeCancelled` on abort)

**Interfaces:**
- Consumes (Task 2): `CancelToken`, `TranscribeError`, `transcribe_recording(..., force, cancel, on_progress)`.
- Produces (for Task 4): `struct ActiveJob { mp3: PathBuf, vault_id: String, cancel: CancelToken, started_at_ms: u64, phase: Phase, progress: Arc<AtomicU8> }`, `enum Phase { Downloading { received: u64, total: Option<u64> }, Preparing, Transcribing }`, `TranscriptionQueue.active: Option<ActiveJob>`; events `capture:modelReady { mp3 }`, `capture:transcribeProgress { mp3, progress }`, `capture:transcribeCancelled { mp3 }`, and `capture:modelDownload` now carries `mp3`.

- [ ] **Step 1: Name the worker thread**

In `run_transcription` (currently `std::thread::spawn(move || { ... })`), replace with:
```rust
    std::thread::Builder::new()
        .name("transcribe-worker".into())
        .spawn(move || { /* existing body */ })
        .expect("failed to spawn transcribe-worker thread");
```

- [ ] **Step 2: Add the phase/active-job types + queue field**

Beside `TranscriptionJob`:
```rust
#[derive(Clone)]
enum Phase {
    Downloading { received: u64, total: Option<u64> },
    Preparing,
    Transcribing,
}
impl Phase {
    fn as_str(&self) -> &'static str {
        match self { Phase::Downloading { .. } => "downloading", Phase::Preparing => "preparing", Phase::Transcribing => "transcribing" }
    }
}
struct ActiveJob {
    mp3: PathBuf,
    vault_id: String,
    cancel: CancelToken,
    started_at_ms: u64,
    phase: Phase,
    progress: Arc<AtomicU8>, // 0..100 inference %, written lock-free from the callback
}
```
Add `active: Option<ActiveJob>` to `TranscriptionQueue` (it already holds `pending` + `known`). Add `use std::sync::atomic::{AtomicU8, Ordering};` and `use vault_buddy_transcribe::{CancelToken, TranscribeError};` as needed.

- [ ] **Step 3: Phase transitions + progress in `process_transcription`**

Rework `process_transcription` so each transition is observable (keep the existing config/gate/tier logic and the `job.force` placeholder rule from the recording-indicator increment):

1. Compute `started_at_ms` (use the shell's existing millis helper; grep for `as_millis` usage in this file) and set `active = Some(ActiveJob { …, cancel: CancelToken::new(), phase: Phase::Preparing, progress: Arc::new(AtomicU8::new(0)) })` under the queue mutex (brief), emitting `capture:transcribing { mp3, vaultId }` as today.
2. `ensure_model`: pass a progress callback that, when a download is needed, sets `phase = Downloading { received, total }` (under the mutex, brief) and emits throttled `capture:modelDownload { mp3, model, received, total }` (**now carries `mp3`**).
3. **Handover:** the instant `ensure_model` returns `Ok`, emit `capture:modelReady { mp3 }` and set `phase = Preparing`. This replaces the download row *before* the model-load gap.
4. After the model loads, set `phase = Transcribing`, emit an initial `capture:transcribeProgress { mp3, progress: 0 }`, and call:
```rust
    let cancel = active_cancel_clone; // clone of the ActiveJob's token, taken under the mutex
    let progress = active_progress_clone; // Arc<AtomicU8> clone
    let app_cb = app.clone();
    let mp3_cb = job.mp3.clone();
    let mut last_sent: i32 = -1;
    let mut last_logged: i32 = -1;
    let on_progress: Box<dyn FnMut(i32) + Send> = Box::new(move |p| {
        progress.store(p.clamp(0, 100) as u8, Ordering::Relaxed); // lock-free, no queue mutex
        if p - last_sent >= 5 || p >= 100 { // throttled UI event
            last_sent = p;
            let _ = app_cb.emit("capture:transcribeProgress", serde_json::json!({ "mp3": mp3_cb.to_string_lossy(), "progress": p }));
        }
        if p - last_logged >= 25 || p >= 100 { // honest log: coarse periodic progress
            last_logged = p;
            log::info!("transcribe: {} inference {}%", mp3_cb.display(), p);
        }
    });
    // (inference start/elapsed with audio length is logged inside transcribe_recording — Task 2 — which owns the samples.)
    let result = transcribe_recording(&job.mp3, transcriber, &opts, &generated_at, job.force, &cancel, on_progress);
```
5. Terminal match:
```rust
    match result {
        Ok(path) => { /* existing capture:transcribed emit + log */ }
        Err(TranscribeError::Failed(e)) => fail_transcription(app, &job.mp3, &e),
        Err(TranscribeError::Cancelled) => {
            // Our own placeholder → cancelled; never a complete/user file.
            let _ = vault_buddy_core::transcript::force_write_sidecar(
                &vault_buddy_core::transcript::transcript_path(&job.mp3),
                &vault_buddy_core::transcript::render_cancelled(&file_name(&job.mp3)),
            );
            let _ = app.emit("capture:transcribeCancelled", serde_json::json!({ "mp3": job.mp3.to_string_lossy() }));
            log::info!("transcribe: cancelled {}", job.mp3.display());
        }
    }
```
Clear `active = None` under the mutex when done (in the worker loop, beside the existing `known.remove`). The whisper progress callback and `on_progress` must never take the queue mutex — only the atomic + `app.emit`.

- [ ] **Step 4: Verify (compile is the Windows gate)**

Run (from `src-tauri/`): `cargo fmt --check`. Cannot compile the shell on Linux — the `windows-app` CI job (Tasks 2+3+4 pushed together) is the gate. Mirror the existing emit/log patterns exactly.

- [ ] **Step 5: Commit (HOLD push)**

```bash
git add src-tauri/src/capture_commands.rs
git commit -m "feat(shell): phase-aware transcribe worker (active job, progress, cancel plumbing)"
# DO NOT PUSH yet — Task 4 completes the bracket.
```

---

### Task 4: `cancel_transcription` + `transcription_queue_status` commands (shell) — CLOSES BRACKET

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (two commands + DTOs)
- Modify: `src-tauri/src/lib.rs` (register both in `invoke_handler`)

**Interfaces:**
- Consumes (Task 3): `TranscriptionState`/`TranscriptionQueue.active`, `Phase`, `ActiveJob`.
- Produces (frontend contract): `#[tauri::command] cancel_transcription(app, path: String) -> Result<(), String>`; `#[tauri::command] transcription_queue_status(app) -> TranscriptionQueueDto` with camelCase serde:
  ```jsonc
  { "active": { "mp3","vaultId","phase","progress","received","total","startedAtMs" } | null,
    "queued": [ { "mp3","vaultId" } ], "waitingForRecording": bool }
  ```

- [ ] **Step 1: Add the DTOs**

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveJobDto { mp3: String, vault_id: String, phase: String, progress: u8, received: Option<u64>, total: Option<u64>, started_at_ms: u64 }
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct QueuedDto { mp3: String, vault_id: String }
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionQueueDto { active: Option<ActiveJobDto>, queued: Vec<QueuedDto>, waiting_for_recording: bool }
```

- [ ] **Step 2: `transcription_queue_status`**

```rust
#[tauri::command]
pub fn transcription_queue_status(app: AppHandle) -> TranscriptionQueueDto {
    let state = app.state::<TranscriptionState>();
    let guard = state.inner.lock().unwrap();
    let active = guard.active.as_ref().map(|a| {
        let (received, total) = match a.phase { Phase::Downloading { received, total } => (Some(received), total), _ => (None, None) };
        ActiveJobDto {
            mp3: a.mp3.to_string_lossy().into_owned(), vault_id: a.vault_id.clone(),
            phase: a.phase.as_str().to_string(), progress: a.progress.load(Ordering::Relaxed),
            received, total, started_at_ms: a.started_at_ms,
        }
    });
    let queued = guard.pending.iter().map(|j| QueuedDto { mp3: j.mp3.to_string_lossy().into_owned(), vault_id: j.vault_id.clone() }).collect();
    // "waiting" = there is work but nothing active because a recording is live.
    let waiting_for_recording = active.is_none() && !guard.pending.is_empty() && is_recording(&app);
    TranscriptionQueueDto { active, queued, waiting_for_recording }
}
```

- [ ] **Step 3: `cancel_transcription` — mutex-brief, sidecar-outside-mutex**

```rust
#[tauri::command]
pub fn cancel_transcription(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    // Phase 1: fast bookkeeping under the mutex; decide what to write after.
    let write_cancelled = {
        let state = app.state::<TranscriptionState>();
        let mut guard = state.inner.lock().unwrap();
        if guard.active.as_ref().map(|a| a.mp3 == mp3).unwrap_or(false) {
            guard.active.as_ref().unwrap().cancel.cancel(); // aborts inference; the worker writes the cancelled sidecar
            return Ok(()); // worker owns the terminal bookkeeping for the active job
        }
        // Pending job: drop it now; write its sidecar AFTER releasing the lock.
        let before = guard.pending.len();
        guard.pending.retain(|j| j.mp3 != mp3);
        if guard.pending.len() == before { return Err("No such transcription in the queue.".into()); }
        guard.known.remove(&mp3);
        true
    }; // <-- mutex released here
    if write_cancelled {
        let name = mp3.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let _ = vault_buddy_core::transcript::force_write_sidecar(
            &vault_buddy_core::transcript::transcript_path(&mp3),
            &vault_buddy_core::transcript::render_cancelled(&name),
        );
        let _ = app.emit("capture:transcribeCancelled", serde_json::json!({ "mp3": mp3.to_string_lossy() }));
    }
    Ok(())
}
```

- [ ] **Step 4: Register both**

In `src-tauri/src/lib.rs` `invoke_handler![...]`, add `capture_commands::cancel_transcription` and `capture_commands::transcription_queue_status` beside `retranscribe`/`transcribe_recording_now`.

- [ ] **Step 5: Commit + push 2+3+4 together (bracket)**

```bash
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): cancel_transcription + transcription_queue_status commands"
git push   # publishes Tasks 2+3+4 as one green windows-app compile
```
Confirm the `windows-app` job is green before continuing.

---

### Task 5: transcription event + DTO types (frontend)

**Files:**
- Modify: `src/types.ts` (add `Phase`, `TranscriptionJob`, `TranscriptionQueueStatus`, new event payload types)

**Interfaces:**
- Produces:
```ts
export type Phase = "queued" | "downloading" | "preparing" | "transcribing" | "done" | "failed" | "cancelled";
export interface TranscriptionJob {
  mp3: string; vaultId: string; name: string;
  phase: Phase; progress: number | null;   // 0..1 for downloading/transcribing
  model: string | null; error: string | null; startedAtMs: number | null;
}
export interface TranscriptionQueueStatus {
  active: { mp3: string; vaultId: string; phase: "downloading" | "preparing" | "transcribing"; progress: number; received: number | null; total: number | null; startedAtMs: number } | null;
  queued: { mp3: string; vaultId: string }[];
  waitingForRecording: boolean;
}
export interface TranscribeProgress { mp3: string; progress: number }
export interface ModelReady { mp3: string }
export interface TranscribeCancelled { mp3: string }
```
Extend the existing `ModelDownload` type with `mp3: string`.

- [ ] **Step 1–2:** Add the types; `npm run build` (vue-tsc) stays green (types only). No test needed for a types-only change.
- [ ] **Step 3: Commit + push**
```bash
git add src/types.ts
git commit -m "feat(ui): transcription queue + progress event types"
git push
```

---

### Task 6: per-job transcription model in the `capture` store (frontend)

**Files:**
- Modify: `src/stores/capture.ts` (replace singular transcription fields with a keyed map + getters + actions + seeded init)
- Modify: `tests/capture-store.test.ts` (job-map reducer coverage)
- Delete: `src/components/TranscriptionStatus.vue` + `tests/transcription-status.test.ts` (its store fields are removed here — deleting it now keeps `npm run build` green; the new summary arrives in Task 11)
- Modify: `src/components/ActionPanel.vue` (drop the `<TranscriptionStatus>` element + import — the list view has no summary line until Task 11)

**Interfaces:**
- Consumes (Task 5): `Phase`, `TranscriptionJob`, `TranscriptionQueueStatus`, event payload types. Commands `transcription_queue_status`, `cancel_transcription`, `retranscribe`, `open_transcript`.
- Produces: state `transcriptions: Record<string, TranscriptionJob>`, `waitingForRecording: boolean`; getters `activeTranscription`, `queuedTranscriptions`, `finishedTranscriptions`, `transcribingVaultId`; actions `cancelTranscription(mp3)`, `retranscribe(mp3)`, `openTranscript(mp3)`.

- [ ] **Step 1: Write the failing tests** (extend `tests/capture-store.test.ts`)

```ts
  const active = () => useCaptureStore().activeTranscription;

  it("seeds the job map from transcription_queue_status on init", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
      if (cmd === "transcription_queue_status")
        return { active: { mp3: "a.mp3", vaultId: "v1", phase: "transcribing", progress: 40, received: null, total: null, startedAtMs: 1 }, queued: [{ mp3: "b.mp3", vaultId: "v1" }], waitingForRecording: false };
    });
    const store = useCaptureStore();
    await store.init();
    expect(store.transcriptions["a.mp3"].phase).toBe("transcribing");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(0.4);
    expect(store.transcriptions["b.mp3"].phase).toBe("queued");
    expect(store.queuedTranscriptions.map((j) => j.mp3)).toEqual(["b.mp3"]);
  });

  it("modelReady clears download progress and moves to preparing", async () => {
    mockIPC((cmd) => { if (cmd === "capture_status" || cmd === "transcription_queue_status") return cmd === "capture_status" ? { recording: false, vaultId: null, startedAtMs: null } : { active: null, queued: [], waitingForRecording: false }; });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    state.eventHandlers["capture:modelDownload"]!({ payload: { mp3: "a.mp3", model: "small", received: 5, total: 10 } });
    expect(store.transcriptions["a.mp3"].phase).toBe("downloading");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(0.5);
    state.eventHandlers["capture:modelReady"]!({ payload: { mp3: "a.mp3" } });
    expect(store.transcriptions["a.mp3"].phase).toBe("preparing");
    expect(store.transcriptions["a.mp3"].progress).toBeNull();
    state.eventHandlers["capture:transcribeProgress"]!({ payload: { mp3: "a.mp3", progress: 12 } });
    expect(store.transcriptions["a.mp3"].phase).toBe("transcribing");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(0.12);
  });

  it("cancelled event moves the job to cancelled; transcribingVaultId clears", async () => {
    mockIPC((cmd) => cmd === "capture_status" ? { recording: false, vaultId: null, startedAtMs: null } : { active: null, queued: [], waitingForRecording: false });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    expect(store.transcribingVaultId).toBe("v1");
    state.eventHandlers["capture:transcribeCancelled"]!({ payload: { mp3: "a.mp3" } });
    expect(store.transcriptions["a.mp3"].phase).toBe("cancelled");
    expect(store.transcribingVaultId).toBeNull();
  });

  it("cancelTranscription invokes cancel_transcription with the path", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => { calls.push({ cmd, args }); if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null }; if (cmd === "transcription_queue_status") return { active: null, queued: [], waitingForRecording: false }; });
    const store = useCaptureStore();
    await store.init();
    await store.cancelTranscription("a.mp3");
    expect(calls).toContainEqual({ cmd: "cancel_transcription", args: { path: "a.mp3" } });
  });
```

- [ ] **Step 2: Run to verify they fail** — `npx vitest run tests/capture-store.test.ts` → FAIL (map/getters/actions undefined).

- [ ] **Step 3: Implement** — In `capture.ts`:
  - Remove `transcribing`, `modelDownload`, `transcriptError`, `transcriptFailedMp3`, `transcribingVaultId`, `lastTranscribed` from state; add `transcriptions: {} as Record<string, TranscriptionJob>` and `waitingForRecording: false`.
  - Add a private `upsert(mp3, patch)` helper (assign a new object into the reactive record so Vue tracks it) and `nameOf(mp3)` (basename without `.mp3`).
  - In `init()`: after the `capture_status` resync, call `transcription_queue_status` and seed the map (active → its phase; each queued → `queued`; set `waitingForRecording`). Replace the transcription listeners so each event upserts the keyed job:
    - `capture:transcribing {mp3,vaultId}` → `{ phase: "preparing", vaultId, name, progress: null, error: null, startedAtMs: Date.now() }`
    - `capture:modelDownload {mp3,received,total,model}` → `{ phase: "downloading", model, progress: total ? received/total : null }`
    - `capture:modelReady {mp3}` → `{ phase: "preparing", progress: null }`
    - `capture:transcribeProgress {mp3,progress}` → `{ phase: "transcribing", progress: clamp01(progress/100) }`
    - `capture:transcribed {mp3}` → `{ phase: "done", progress: 1 }`
    - `capture:transcribeFailed {mp3,message}` → `{ phase: "failed", error: message, progress: null }`
    - `capture:transcribeCancelled {mp3}` → `{ phase: "cancelled", progress: null }`
  - Getters: `activeTranscription` = the one job whose phase ∈ downloading/preparing/transcribing (or null); `queuedTranscriptions` = phase `queued`; `finishedTranscriptions` = phase ∈ done/failed/cancelled, newest-first, bounded (e.g. last 20); `transcribingVaultId` = `activeTranscription?.vaultId ?? null`.
  - Actions: `cancelTranscription(mp3)` → `invoke("cancel_transcription",{path:mp3})` (catch→warn); `retranscribe(mp3)` → `invoke("retranscribe",{path:mp3})`; `openTranscript(mp3)` → `invoke("open_transcript",{path:mp3})` (catch→warn).

- [ ] **Step 4: Delete the orphaned status component** — the removed fields (`transcribing`, `modelDownload`, `transcriptError`, `transcriptFailedMp3`, `lastTranscribed`, and the `retryTranscription`/`openTranscript`/`dismissTranscribed` actions) were read **only** by `TranscriptionStatus.vue`. Delete `src/components/TranscriptionStatus.vue` + `tests/transcription-status.test.ts`, and remove its element + import from `ActionPanel.vue` (Recordings.vue keeps its own local state until Task 12, so it still compiles). `transcribingVaultId` survives as a **getter**, so `ActionPanel`→`VaultList` is unaffected.
- [ ] **Step 5: Run to verify** — `npx vitest run tests/capture-store.test.ts` → PASS; `npm test` (full suite, catches the deleted-component references); `npm run build` → green.
- [ ] **Step 6: Commit + push** — `git add src/stores/capture.ts tests/capture-store.test.ts src/components/ActionPanel.vue && git rm src/components/TranscriptionStatus.vue tests/transcription-status.test.ts && git commit -m "feat(ui): per-job transcription store model (fixes stale Recordings state)"`; `git push`.

> The old `RENAME_PROMPT_MS` / recording/rename state in this store is untouched — only the transcription fields are refactored.

---

### Task 7: `transcriptions` panel view + `back()` target (frontend)

**Files:**
- Modify: `src/stores/vaults.ts` (view union + `openTranscriptions` + `back()`); `tests/vaults-store.test.ts`

**Interfaces:**
- Produces: `view` union gains `"transcriptions"`; `openTranscriptions()`; `back()` maps `transcriptions` → `showList()`.

- [ ] **Step 1: Failing test**
```ts
  it("opens and backs out of the transcriptions view", () => {
    const s = useVaultsStore();
    s.openTranscriptions();
    expect(s.view).toBe("transcriptions");
    s.back();
    expect(s.view).toBe("list");
  });
```
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** add `"transcriptions"` to the `view` type union; `openTranscriptions() { this.view = "transcriptions"; }`; in `back()` add `if (this.view === "transcriptions") return this.showList();`; `showList()` needs no new id to clear (the view has no vault-scoped id). 
- [ ] **Step 4:** run → PASS; `npm test`.
- [ ] **Step 5:** commit + push — `feat(ui): transcriptions panel view + back target`.

---

### Task 8: shared controlled `TranscriptionSettings.vue` (frontend)

**Files:**
- Create: `src/components/TranscriptionSettings.vue`; Test: `tests/transcription-settings.test.ts`
- Modify: `src/components/CaptureSettings.vue` (use the extracted component)

**Interfaces:**
- Produces: `<TranscriptionSettings :model-value="{transcribe,transcriptionModel,transcriptionLanguage,transcriptTimestamps}" @update:model-value="..."/>` — a **controlled** component with no persistence of its own (emits `update:modelValue` with the full sub-object on any field change).

- [ ] **Step 1: Failing test** — mount with a model value; toggle transcribe; assert an `update:modelValue` event carries the merged object with `transcribe` flipped; assert the model/language/timestamps controls render and emit.
- [ ] **Step 2:** run → FAIL (component missing).
- [ ] **Step 3:** Extract the transcribe block (toggle + model select + language input + timestamps checkbox) from `CaptureSettings.vue` into `TranscriptionSettings.vue` as `defineProps<{ modelValue: {...} }>()` + `defineEmits(["update:modelValue"])`, emitting a spread-merged object per change. Replace that block in `CaptureSettings.vue` with `<TranscriptionSettings v-model="...">` bound to its existing reactive config object (CaptureSettings keeps its own Save button/persistence).
- [ ] **Step 4:** run the new test + `npx vitest run tests/capture-settings.test.ts` → PASS; `npm run build`.
- [ ] **Step 5:** commit + push — `feat(ui): extract controlled TranscriptionSettings component`.

---

### Task 9: transcription settings on the Record view (frontend)

**Files:**
- Modify: `src/components/RecordMode.vue` (render `TranscriptionSettings`, save vault config on change); `tests/record-mode.test.ts`

**Interfaces:**
- Consumes: `TranscriptionSettings` (Task 8); `get_capture_config`/`set_capture_config` (RecordMode already loads config).

- [ ] **Step 1: Failing test** — mount RecordMode (mock `get_capture_config` returning a config); change a transcription field via the child; assert `set_capture_config` is invoked with the full vault config carrying the updated transcription fields.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** In `RecordMode.vue`, render `<TranscriptionSettings v-model="transcription">` above the Meeting/Voice buttons, seeded from the loaded `CaptureConfig`; on `update:modelValue`, merge into the loaded config and `invoke("set_capture_config", { id: vaultId, config })`. Keep the existing "config read never blocks recording" fallback (a read error → defaults, settings still editable).
- [ ] **Step 4:** run → PASS; `npm run build`.
- [ ] **Step 5:** commit + push — `feat(ui): transcription settings on the Record view (saves vault config)`.

---

### Task 10: `Transcriptions.vue` progress view + ActionPanel registration (frontend)

**Files:**
- Create: `src/components/Transcriptions.vue`; Test: `tests/transcriptions.test.ts`
- Modify: `src/components/ActionPanel.vue` (register the `transcriptions` view slot + header title)

**Interfaces:**
- Consumes: `capture` store getters `activeTranscription`, `queuedTranscriptions`, `finishedTranscriptions`, `waitingForRecording`; actions `cancelTranscription`, `retranscribe`, `openTranscript`.

- [ ] **Step 1: Failing tests** — with a seeded store: (a) an active `transcribing` job renders a progress bar at its % + a Cancel button that calls `cancelTranscription`; (b) an active `downloading` job renders the download %; (c) `waitingForRecording` renders "Waiting for the recording to finish…"; (d) a queued job renders with Cancel; (e) finished done/failed/cancelled rows render Open / Re-transcribe / Re-transcribe respectively; (f) the "taking longer than expected" hint appears when a transcribing job's progress hasn't advanced for the stuck threshold (drive via fake timers + repeated identical `transcribeProgress`).
- [ ] **Step 2:** run → FAIL (component missing).
- [ ] **Step 3:** Build `Transcriptions.vue` (mirror `Recordings.vue` structure): an **Active** block (name · vault · phase label · `<progress>`-style bar bound to `progress` · elapsed from `startedAtMs` · Cancel), a **Queued** list (Cancel each), a **Finished this session** list (done→Open, failed→error+Re-transcribe, cancelled→Re-transcribe). Track a per-mp3 `lastProgressChangeMs` locally to raise the stuck hint after ~2 min without advance. Testids: `transcription-active`, `transcription-progress`, `transcription-cancel`, `transcription-queued`, `transcription-finished`, `transcription-stuck-hint`.
- [ ] **Step 4:** In `ActionPanel.vue`, add the `v-else-if="view === 'transcriptions'"` slot rendering `<Transcriptions/>` and the header title case "Transcriptions" (back button already covers non-list views via Task 7). Run tests + `npx vitest run tests/action-panel.test.ts` → PASS; `npm run build`.
- [ ] **Step 5:** commit + push — `feat(ui): Transcriptions progress view`.

---

### Task 11: compact transcription summary indicator (frontend)

**Files:**
- Create: `src/components/TranscriptionSummary.vue`; Test: `tests/transcription-summary.test.ts`
- Modify: `src/components/ActionPanel.vue` (add `<TranscriptionSummary>` on the list view — the old `<TranscriptionStatus>` was already removed in Task 6)

**Interfaces:**
- Consumes: `activeTranscription`, `queuedTranscriptions`, `finishedTranscriptions`; opens the view via `vaults.openTranscriptions()`.

- [ ] **Step 1: Failing test** — active job → renders `⟳ Transcribing "X" — 42% · +N queued` and clicking calls `openTranscriptions`; a failed-only state → `⚠ 1 transcription failed`; idle → renders nothing.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Build `TranscriptionSummary.vue` (one line, `role="button"`, `@click` → `store.openTranscriptions()`), deriving its label from the getters. In `ActionPanel.vue` add `<TranscriptionSummary v-if="view === 'list'">` where `<TranscriptionStatus>` used to sit, and import it.
- [ ] **Step 4:** run → PASS; `npm test`; `npm run build`.
- [ ] **Step 5:** commit + push — `feat(ui): compact transcription summary replaces single-line status`.

---

### Task 12: `Recordings.vue` live-map state + cancel + spinner (frontend)

**Files:**
- Modify: `src/components/Recordings.vue`; `tests/recordings.test.ts`

**Interfaces:**
- Consumes: `capture` store `transcriptions` map + `activeTranscription`; actions `cancelTranscription`, `retranscribe`.

- [ ] **Step 1: Failing tests** —
  - A row whose mp3 has an **active** job (queued/downloading/preparing/transcribing) in the store map renders an animated spinner and a **disabled** re-transcribe button + a **Cancel** button that calls `cancelTranscription`.
  - A row whose sidecar is `pending` but is **not** in the live map (crash-stuck) keeps the re-transcribe button **enabled** (preserves the recording-indicator-era "stuck pending stays re-transcribable" guarantee — regression-linked).
  - Leaving and re-mounting the view (new component instance) preserves the disabled/spinner state because it derives from the store map, not a local `ref`.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Replace the component-local `transcribingMp3` `Set` + the three `capture:transcrib*` listeners with derived state from the store: `isActive(mp3) = ['queued','downloading','preparing','transcribing'].includes(store.transcriptions[mp3]?.phase)`. Re-transcribe button `:disabled="isActive(r.mp3)"`; add a Cancel button `v-if="isActive(r.mp3)"` → `store.cancelTranscription(r.mp3)`; swap the static `…` glyph for a spinner when `isActive`. Remove the now-dead local listener/unlisten plumbing (the store owns the events).
- [ ] **Step 4:** run → PASS; `npm test`; `npm run build`.
- [ ] **Step 5:** commit + push — `feat(ui): Recordings row derives transcribe state from the store; adds cancel + spinner`.

---

## Notes for the executor

- **Bracket reminder:** push Task 1 alone; **hold Tasks 2–4 and push together**; then push Tasks 5–12 individually. After the 2+3+4 push, confirm the `windows-app` CI job is green before starting Task 5.
- **Store refactor blast radius:** the only consumer of the removed store fields (`transcribing`, `modelDownload`, `transcriptError`, `transcriptFailedMp3`, `lastTranscribed`, and the old retry/open/dismiss actions) is `TranscriptionStatus.vue`, which **Task 6 deletes in the same commit** — so `npm run build` stays green after every task. `Recordings.vue` keeps its own local transcribe state until Task 12; `transcribingVaultId` survives as a getter, so `ActionPanel`→`VaultList` is unaffected. Between Task 6 and Task 11 the list view simply shows no summary line (feature-in-progress, still green).
- **`Date.now()` in the store/components is fine** (frontend) — only workflow scripts forbid it.
