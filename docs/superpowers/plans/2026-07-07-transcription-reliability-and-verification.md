# Transcription Reliability & Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the committed abort-callback fix transcribes a real recording end-to-end, then harden the transcription backend (no-speech handling, crash-proof worker, SHA-256 model integrity), tighten logging/error handling, and extract the transcription orchestration into its own module.

**Architecture:** Pure logic lands in `vault_buddy_core` (Linux-tested); the whisper engine + verification harness sit behind the `whisper` feature (Windows compile gate); the shell worker moves into a focused `transcription` module. No frontend, no audio-quality-path, and no dependency-version changes.

**Tech Stack:** Rust (Tauri v2 shell, `vault_buddy_core`, `vault_buddy_transcribe`), whisper-rs 0.16 (pinned) + static whisper.cpp behind `--features whisper`, `symphonia` (MP3 decode), `ureq` (download), `sha2` (new), Vitest/vue-tsc (unchanged).

## Global Constraints

- **whisper-rs = 0.16, whisper-rs-sys = 0.15 stay pinned.** Do not bump them.
- **The `whisper` feature is Windows-only.** The `windows-app` CI job is the compile gate. Locally, run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.
- **Vault writes only via the sanctioned sidecar path.** Never clobber a `complete`/hand-edited transcript; use `replace_if_ours` / `force_write_sidecar` and the atomic writer only.
- **Diagnostics invariants:** every spawned thread is named; no error is swallowed (caught failures go through `log::warn!`/`log::error!`); the verification harness is read-only (prints, never writes a vault).
- **Shell is PowerShell:** chain commands with `;`, never `&&`.
- **Commits:** Conventional Commits (`feat`/`fix`/`refactor`/`docs`/`test` + scope `transcribe`/`shell`/`core`). Imperative subject; body explains the *why*/failure mode. Regression tests name the failure mode in a comment.
- **TDD:** failing test first, then minimal implementation.

---

### Task 1: End-to-end verification harness + regression guard

Proves the fixed engine actually transcribes real audio (the whole plan's premise) and leaves a durable `-6` guard. Runs on Windows with the real `base` model already at `%APPDATA%\vault-buddy\models\ggml-base.bin`.

**Files:**
- Create: `src-tauri/transcribe/examples/transcribe_file.rs`
- Modify: `src-tauri/transcribe/src/engine.rs` (add one `#[ignore]` test in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `WhisperTranscriber::load(&Path) -> Result<Self,String>`, `Transcriber::transcribe(&self, &[f32], Option<&str>, &CancelToken, Box<dyn FnMut(i32)+Send>) -> Result<Vec<Segment>,String>`, `decode::decode_to_16k_mono(&Path, &CancelToken) -> Result<Vec<f32>,String>`, `model::model_path(ModelTier) -> Option<PathBuf>`.
- Produces: nothing other tasks consume.

- [ ] **Step 1: Write the print-only harness example**

Create `src-tauri/transcribe/examples/transcribe_file.rs`:

```rust
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
use vault_buddy_transcribe::{CancelToken, Transcriber};

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
    let segments = t
        .transcribe(&samples, None, &cancel, Box::new(|p| eprintln!("progress {p}%")))
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
```

- [ ] **Step 2: Build the example (no run yet) to verify it compiles**

Run: `cargo build -p vault_buddy_transcribe --features whisper --example transcribe_file`
Expected: compiles; `Finished`.

- [ ] **Step 3: Run it on a SHORT real recording for ground truth**

Run (adjust the path to a real short capture, e.g. a ~12–23 s voice note):
`cargo run -p vault_buddy_transcribe --features whisper --example transcribe_file -- "C:\Projects\Project Delivery\01 - Projects\PMO.PreSales\Voice Notes\2026\07\2026-07-06 2301 Voice Note.mp3" base`
Expected: `OK: N segments` with N ≥ 1 and printed transcript lines — **no `-6`/panic**.

**GATE:** If this returns `Err` (the `-6` abort) or zero segments on clearly-spoken audio, STOP the plan and switch to superpowers:systematic-debugging — the premise (fix works) has failed and the remaining hardening is premature.

- [ ] **Step 4: (Optional, longer) run it on the 49-min meeting**

Run: `cargo run -p vault_buddy_transcribe --features whisper --example transcribe_file -- "C:\Projects\Project Delivery\01 - Projects\PMO.PreSales\Meetings\2026\07\2026-07-07 0901 Meeting.mp3" base`
Expected: completes with many segments (minutes of CPU inference); confirms long-input stability.

- [ ] **Step 5: Add the durable regression test**

In `src-tauri/transcribe/src/engine.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
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
    let t = WhisperTranscriber::load(&model).expect("load model");
    let out = t.transcribe(&samples, None, &cancel, Box::new(|_| {}));
    assert!(
        out.is_ok(),
        "fixed engine must not abort at the first encode window (the -6 bug): {out:?}"
    );
    if std::env::var("VB_TEST_AUDIO").is_ok() {
        assert!(
            !out.unwrap().is_empty(),
            "a real speech clip must yield at least one segment"
        );
    }
}
```

- [ ] **Step 6: Verify the ignored test builds and is skipped by default**

Run: `cargo test -p vault_buddy_transcribe --features whisper real_model_transcribes_without_spurious_abort`
Expected: compiles; reports `0 passed; ... 1 ignored` (not run by default).

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/transcribe/examples/transcribe_file.rs src-tauri/transcribe/src/engine.rs
git commit -m "test(transcribe): end-to-end verification harness + -6 regression guard"
```

---

### Task 2: `EmitThrottle` pure helper in core

Extracts the "emit only every N units" decision (currently inline, untestable, in the shell's download callback + progress closure) into a tested core primitive.

**Files:**
- Create: `src-tauri/core/src/throttle.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod throttle;`)
- Test: in `throttle.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `vault_buddy_core::throttle::EmitThrottle`; `EmitThrottle::new(min_delta: u64) -> EmitThrottle`; `EmitThrottle::should_emit(&mut self, value: u64, terminal: bool) -> bool`.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/core/src/throttle.rs`:

```rust
//! A tiny value-delta gate: fire an emit only when a monotonically-growing
//! value has advanced by at least `min_delta` since the last emit, or when a
//! terminal tick (100% / final byte) forces one. Pulled out of the shell so
//! the throttling decision is unit-tested instead of inlined per call site.

pub struct EmitThrottle {
    min_delta: u64,
    last: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_first_then_every_min_delta_and_on_terminal() {
        let mut t = EmitThrottle::new(5);
        assert!(t.should_emit(0, false), "first call always emits");
        assert!(!t.should_emit(3, false), "below delta: suppressed");
        assert!(t.should_emit(5, false), "reached delta from last emit (0)");
        assert!(!t.should_emit(9, false), "9-5=4 < 5: suppressed");
        assert!(t.should_emit(9, true), "terminal forces an emit even below delta");
        // terminal recorded 9 as last, so next delta measures from 9
        assert!(!t.should_emit(13, false), "13-9=4 < 5");
        assert!(t.should_emit(14, false), "14-9=5 >= 5");
    }

    #[test]
    fn large_deltas_for_byte_counts() {
        let mut t = EmitThrottle::new(4_000_000);
        assert!(t.should_emit(0, false));
        assert!(!t.should_emit(3_999_999, false));
        assert!(t.should_emit(4_000_000, false));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vault_buddy_core throttle`
Expected: FAIL — `EmitThrottle::new` / `should_emit` not found.

- [ ] **Step 3: Write minimal implementation**

Add above the `#[cfg(test)]` block in `throttle.rs`:

```rust
impl EmitThrottle {
    pub fn new(min_delta: u64) -> Self {
        Self { min_delta, last: None }
    }

    /// True when `value` should be emitted: the first call, any call whose
    /// value advanced by >= `min_delta` since the last emit, or any `terminal`
    /// call. Records the approved value so the next delta measures from it.
    pub fn should_emit(&mut self, value: u64, terminal: bool) -> bool {
        let fire = terminal
            || match self.last {
                None => true,
                Some(prev) => value.saturating_sub(prev) >= self.min_delta,
            };
        if fire {
            self.last = Some(value);
        }
        fire
    }
}
```

Add to `src-tauri/core/src/lib.rs` (with the other `pub mod` lines):

```rust
pub mod throttle;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vault_buddy_core throttle`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/core/src/throttle.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): add EmitThrottle value-delta gate for progress throttling"
```

---

### Task 3: No-speech transcript body (core)

A `full()` that returns zero segments (silence/non-speech) must render an explicit notice, not a blank `complete` sidecar that looks broken.

**Files:**
- Modify: `src-tauri/core/src/transcript.rs` (`render_transcript`, ~lines 76-106)
- Test: `src-tauri/core/src/transcript.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `render_transcript(&TranscriptMeta, &[Segment]) -> String` (signature unchanged).
- Produces: same signature; body now contains `_No speech detected._` when no non-empty segment exists.

- [ ] **Step 1: Write the failing test**

Add to `transcript.rs` tests:

```rust
#[test]
fn empty_segments_render_a_no_speech_body_not_a_blank_one() {
    // whisper legitimately returns no segments for silence/non-speech; a blank
    // `complete` sidecar looks broken. It must stay `complete` (not a failure)
    // but say so, and still carry the stats table.
    let t = render_transcript(&meta(), &[]);
    assert!(t.contains("vault-buddy-transcript: complete"));
    assert!(t.contains("_No speech detected._"));
    assert!(t.contains("## Statistics"));
    // All-empty-text segments take the same path.
    let t2 = render_transcript(&meta(), &[seg(0, 10, "   "), seg(10, 20, "")]);
    assert!(t2.contains("_No speech detected._"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vault_buddy_core empty_segments_render_a_no_speech_body`
Expected: FAIL — body lacks `_No speech detected._`.

- [ ] **Step 3: Write minimal implementation**

In `render_transcript`, replace the segment loop (currently lines ~93-103) with a `wrote_any`-tracking version and a fallback line before the stats call:

```rust
    let mut wrote_any = false;
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        wrote_any = true;
        if meta.timestamps {
            out.push_str(&format!("{} {text}\n\n", format_timestamp(s.start_ms)));
        } else {
            out.push_str(&format!("{text}\n\n"));
        }
    }
    if !wrote_any {
        // Zero segments (or all-empty) is a valid whisper result for silence —
        // a complete transcript with an honest notice, not a blank body.
        out.push_str("_No speech detected._\n\n");
    }
    out.push_str(&render_stats(meta, segments));
    out
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vault_buddy_core transcript`
Expected: PASS — the new test plus all existing transcript tests (the `stats_speaking_rate` case already renders `&[]` and must still find `## Statistics`).

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/core/src/transcript.rs
git commit -m "fix(core): render 'No speech detected' for an empty transcript instead of a blank body"
```

---

### Task 4: SHA-256 model integrity (transcribe)

A complete-but-corrupt download passes the length check, fails to load, and the self-heal re-fetches the same bytes — a possible loop. Verify the published hash during download and reject a mismatch decisively.

**Files:**
- Modify: `src-tauri/transcribe/Cargo.toml` (add `sha2`)
- Modify: `src-tauri/transcribe/src/model.rs` (`ModelTier::sha256`, `download_stream` signature + hashing, `download_model` call site, existing test call sites)
- Test: `src-tauri/transcribe/src/model.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `download_stream(agent, url, dir, file_name, min_size, cancel, on_progress)`.
- Produces: `ModelTier::sha256(&self) -> &'static str`; new `download_stream(agent, url, dir, file_name, min_size, expected_sha256: &str, cancel, on_progress)` (empty `expected_sha256` skips verification).

- [ ] **Step 1: Add the `sha2` dependency**

In `src-tauri/transcribe/Cargo.toml` under `[dependencies]` (after `ureq = "2"`):

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Write the failing tests**

Add to `model.rs` tests (mirrors the existing localhost-server pattern from `download_without_content_length_still_succeeds`):

```rust
#[test]
fn download_matching_sha256_succeeds() {
    // The full body hashes to the expected value → finalized into place.
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut req = [0u8; 1024];
            let _ = sock.read(&mut req);
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello world");
            let _ = sock.flush();
        }
    });
    // sha256("hello world")
    let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    let agent = model_download_agent();
    let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = crate::CancelToken::new();
    let mut progress = |_r: u64, _t: Option<u64>| {};
    let res = download_stream(&agent, &url, dir.path(), "ggml-base.bin", 0, expected, &cancel, &mut progress);
    let _ = server.join();
    assert!(res.is_ok(), "a body matching its hash must finalize: {res:?}");
    assert!(dir.path().join("ggml-base.bin").exists());
}

#[test]
fn download_wrong_sha256_is_rejected_and_leaves_no_files() {
    // Regression: a complete-but-corrupt model (right length, wrong bytes)
    // passes the size/length checks, so only a checksum can reject it. A
    // mismatch must delete the .part and never finalize `dest` — otherwise the
    // shell short-circuits on dest.exists() and loads a corrupt model forever.
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut req = [0u8; 1024];
            let _ = sock.read(&mut req);
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello world");
            let _ = sock.flush();
        }
    });
    let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
    let agent = model_download_agent();
    let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = crate::CancelToken::new();
    let mut progress = |_r: u64, _t: Option<u64>| {};
    let res = download_stream(&agent, &url, dir.path(), "ggml-base.bin", 0, wrong, &cancel, &mut progress);
    let _ = server.join();
    assert!(res.is_err(), "a hash mismatch must be an Err, got {res:?}");
    assert!(!dir.path().join("ggml-base.bin").exists(), "corrupt model must not finalize");
    assert!(!dir.path().join("ggml-base.bin.part").exists(), "the .part must be cleaned up");
}

#[test]
fn tier_sha256_values_are_lowercase_hex_of_expected_length() {
    for t in [ModelTier::Base, ModelTier::Small, ModelTier::Medium] {
        let h = t.sha256();
        assert_eq!(h.len(), 64, "sha256 hex is 64 chars for {t:?}");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p vault_buddy_transcribe sha256`
Expected: FAIL — `sha256` method missing and `download_stream` arity is wrong (won't compile). Compilation failure counts as red.

- [ ] **Step 4: Implement `ModelTier::sha256`**

In `model.rs`, add to `impl ModelTier` (after `min_size`), using the canonical Hugging Face LFS hashes (verified 2026-07-07; the on-disk base file matched exactly):

```rust
    /// Canonical SHA-256 of the ggml file on Hugging Face
    /// (ggerganov/whisper.cpp). Verified during download so a complete-but-
    /// corrupt fetch is rejected instead of cached and reloaded forever.
    pub fn sha256(&self) -> &'static str {
        match self {
            ModelTier::Base => "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
            ModelTier::Small => "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            ModelTier::Medium => "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        }
    }
```

- [ ] **Step 5: Thread the hash through `download_model` → `download_stream`**

In `download_model`, pass the tier's hash (add the argument in the call, after `tier.min_size()`):

```rust
    download_stream(
        &agent,
        &tier.url(),
        &dir,
        tier.file_name(),
        tier.min_size(),
        tier.sha256(),
        cancel,
        on_progress,
    )
```

- [ ] **Step 6: Add hashing to `download_stream`**

Change the signature to add `expected_sha256: &str` (after `min_size`):

```rust
fn download_stream(
    agent: &ureq::Agent,
    url: &str,
    dir: &Path,
    file_name: &str,
    min_size: u64,
    expected_sha256: &str,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
```

At the top of `model.rs` add the imports (near the existing `use` lines):

```rust
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
```

Before the read loop (right after `let mut received: u64 = 0;`) add:

```rust
    let mut hasher = Sha256::new();
```

Inside the loop, right after the successful `write_all` (after `received += n as u64;`), feed the hasher:

```rust
        hasher.update(&buf[..n]);
```

After the `received != total` completeness block and **before** the final rename, add the checksum gate:

```rust
    // Integrity: a complete-but-corrupt body clears both the size floor and the
    // Content-Length check, so only the published hash can reject it. An empty
    // expected hash means "unverified" (kept for tests / a hypothetical future
    // tier); every real tier supplies one. `file` is already dropped above.
    if !expected_sha256.is_empty() {
        let digest = hasher.finalize();
        let mut actual = String::with_capacity(digest.len() * 2);
        for b in digest.iter() {
            let _ = write!(actual, "{b:02x}");
        }
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("model checksum mismatch: got {actual}"));
        }
    }
```

- [ ] **Step 7: Update the three existing `download_stream` test call sites**

`stalled_download_errors_instead_of_hanging`, `truncated_download_under_content_length_errors_and_leaves_no_files`, and `download_without_content_length_still_succeeds` each call `download_stream`. Add `""` (skip verification) in the new `expected_sha256` position, immediately after the `min_size`/`0` argument. Example for the truncated test:

```rust
        let res = download_stream(
            &agent,
            &url,
            dir.path(),
            "ggml-base.bin",
            0,
            "",
            &cancel,
            &mut progress,
        );
```

Apply the same one-argument insertion in the other two call sites.

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p vault_buddy_transcribe model`
Expected: PASS — the three new tests plus all existing model tests green.

- [ ] **Step 9: fmt + clippy the crate (default features)**

Run: `cargo fmt -p vault_buddy_transcribe; cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings`
Expected: no diff from fmt; clippy clean.

- [ ] **Step 10: Commit**

```powershell
git add src-tauri/transcribe/Cargo.toml src-tauri/transcribe/Cargo.lock src-tauri/Cargo.lock src-tauri/transcribe/src/model.rs
git commit -m "fix(transcribe): verify downloaded model SHA-256 to reject corrupt fetches"
```

---

### Task 5: Completion logging, no-speech warning & decode/inference timing (transcribe)

Make the outcome log honest and one-line, split decode vs inference time, and warn on a no-speech result. The render already handles the empty body (Task 3).

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (`transcribe_recording`, ~lines 127-209; `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `transcript::render_transcript`, `decode::decode_to_16k_mono`, `decode::WHISPER_RATE`.
- Produces: `transcribe_recording` signature unchanged; adds a `FakeEmpty` test transcriber locally.

- [ ] **Step 1: Write the failing test**

In `lib.rs` tests, add a fake that returns no segments and assert the end-to-end no-speech behavior:

```rust
struct FakeEmpty;
impl Transcriber for FakeEmpty {
    fn transcribe(
        &self,
        _s: &[f32],
        _l: Option<&str>,
        _c: &CancelToken,
        _p: Box<dyn FnMut(i32) + Send>,
    ) -> Result<Vec<Segment>, String> {
        Ok(vec![])
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vault_buddy_transcribe no_speech_writes_a_complete`
Expected: FAIL — the sidecar lacks `_No speech detected._` only if Task 3 is not yet merged; if Task 3 is merged it will already contain the notice but the `warn` log below is still added. Either way, add the implementation in Step 3. (If it passes immediately because Task 3 landed, proceed — the value here is the log + the regression lock.)

- [ ] **Step 3: Add timing split + no-speech warn + one-line completion log**

In `transcribe_recording`, replace the timing and post-inference logging. Current shape: `let started = std::time::Instant::now();` … decode … inference … `let processing_secs = started.elapsed().as_secs();`. Change to measure decode and inference separately and log once:

Right after the existing `let started = std::time::Instant::now();` keep it (it's the total clock). After the decode call and the cheap cancel check, add:

```rust
    let decode_secs = started.elapsed().as_secs();
    let inference_start = std::time::Instant::now();
```

Replace the existing `log::info!("transcribe: inference done …")` block (after `segments` is obtained) with:

```rust
    let inference_secs = inference_start.elapsed().as_secs();
    let n_segments = segments.iter().filter(|s| !s.text.trim().is_empty()).count();
    if n_segments == 0 {
        log::warn!(
            "transcribe: no speech detected in {} ({duration_secs}s audio)",
            mp3.display()
        );
    }
    log::info!(
        "transcribe: complete {} — {n_segments} segments, {duration_secs}s audio, decode {decode_secs}s + inference {inference_secs}s",
        mp3.display()
    );
```

Keep `let processing_secs = started.elapsed().as_secs();` (total, for the stats meta) where it already is, after the log line above.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vault_buddy_transcribe`
Expected: PASS — new test + all existing lib tests green.

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt -p vault_buddy_transcribe; cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): one-line completion log with decode/inference timing + no-speech warning"
```

---

### Task 6: Extract the transcription orchestration into `transcription.rs` (shell)

Behavior-preserving move out of the ~1740-line `capture_commands.rs`. **No logic changes** in this task — only relocation, visibility, and import/path updates. The Windows compile + existing tests are the gate.

**Files:**
- Create: `src-tauri/src/transcription.rs`
- Modify: `src-tauri/src/capture_commands.rs` (remove the moved items; widen shared helpers to `pub(crate)`)
- Modify: `src-tauri/src/lib.rs` (add `mod transcription;`; repoint `manage`, `invoke_handler`, and startup call)

**Interfaces:**
- Consumes: `crate::capture_commands::{is_recording, now_ms, toast}` (widened to `pub(crate)`), `vault_buddy_core::sync_util::lock_ignoring_poison`, `vault_buddy_transcribe::*`.
- Produces (now under `crate::transcription::`): `TranscriptionState`, commands `transcribe_recording_now`, `retranscribe`, `cancel_transcription`, `transcription_queue_status`, and `run_transcription(&AppHandle)`.

- [ ] **Step 1: Widen shared helpers to `pub(crate)`**

In `capture_commands.rs`, change `fn now_ms()` (line ~178) and `fn toast(` (line ~185) to `pub(crate) fn`. Leave `pub fn is_recording` as is (already `pub`).

- [ ] **Step 2: Create `transcription.rs` with the moved code**

Create `src-tauri/src/transcription.rs`. Move these items **verbatim** out of `capture_commands.rs` into it (cut from the source file):

- Types: `TranscriptionJob`, `Phase` (+ `impl Phase`), `ActiveJob`, `TranscriptionQueue` (+ `impl` with `enqueue`), `Enqueued`, `TranscriptionState`, `ActiveJobDto`, `QueuedDto`, `TranscriptionQueueDto`.
- Fns: `enqueue_transcription`, `scan_and_enqueue`, `set_phase`, `process_transcription`, `ensure_model`, `emit_cancelled`, `fail_transcription`, `owning_vault_id`, `run_transcription`.
- Commands: `transcribe_recording_now`, `retranscribe`, `cancel_transcription`, `transcription_queue_status`.
- The `#[cfg(test)] mod tests` items covering `enqueue` dedup (the four `force_*`/`plain_enqueue_*` tests) — move them into a `#[cfg(test)] mod tests` in `transcription.rs`.

Add this module header + imports at the top of `transcription.rs`:

```rust
//! Local transcription orchestration: the job queue, the single background
//! worker, model download/prepare, and the panel-facing commands. Extracted
//! from capture_commands.rs — the vault-write and never-clobber contracts are
//! unchanged. The worker yields to a live recording (is_recording) and never
//! holds the queue mutex across download/load/inference.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture_commands::{is_recording, now_ms, toast};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery};
use vault_buddy_transcribe::engine::WhisperTranscriber;
use vault_buddy_transcribe::model::{download_model, model_path, ModelTier};
use vault_buddy_transcribe::{
    transcribe_recording, CancelToken, TranscribeError, TranscribeOptions, TranscribeOutcome,
};
```

Then remove now-unused imports from `capture_commands.rs` (the compiler will flag them: e.g. `AtomicU8`, `Condvar`, the `vault_buddy_transcribe::*` imports, `transcribe_recording` etc. — delete only those the shell no longer uses; `Mutex`, `Sender`, capture types remain).

- [ ] **Step 3: Register the module and repoint `lib.rs`**

In `src-tauri/src/lib.rs`:
- Add near the other `mod` declarations: `mod transcription;`
- Line ~190: `.manage(capture_commands::TranscriptionState::default())` → `.manage(transcription::TranscriptionState::default())`
- Lines ~273-276: repoint the four commands to `transcription::`:

```rust
            transcription::transcribe_recording_now,
            transcription::retranscribe,
            transcription::cancel_transcription,
            transcription::transcription_queue_status,
```

- Line ~374: `capture_commands::run_transcription(app.handle());` → `transcription::run_transcription(app.handle());`

- [ ] **Step 4: Build the shell + run its tests**

Run: `cargo build -p vault-buddy; cargo test -p vault-buddy`
Expected: compiles; the four `enqueue` dedup tests pass under `capture_commands::tests` → now `transcription::tests`. Fix any leftover import/visibility errors the compiler names (this is the whole point of the gate).

- [ ] **Step 5: fmt + clippy the shell**

Run: `cargo fmt -p vault-buddy; cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: clean (notably no `unused_imports`).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/transcription.rs src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "refactor(shell): extract transcription orchestration into its own module"
```

---

### Task 7: Crash-proof the transcription worker (shell)

A panic in `process_transcription` today kills the `transcribe-worker` thread and silently stops **all** future transcriptions. Wrap each job in `catch_unwind` (the pattern `lib.rs` already uses for the metronome), fail just that job, and continue.

**Files:**
- Modify: `src-tauri/src/transcription.rs` (`run_transcription` worker loop; add `catch_job` seam + test)

**Interfaces:**
- Produces: `fn catch_job<F: FnOnce()>(f: F) -> bool` (`true` = completed, `false` = panicked) — internal to the module.

- [ ] **Step 1: Write the failing test**

In `transcription.rs` tests:

```rust
#[test]
fn catch_job_survives_a_panicking_job() {
    // Regression: a panic in process_transcription must NOT propagate out of
    // the worker loop (that silently stops all future transcriptions). The
    // seam reports the panic so the loop can fail just that job and continue.
    assert!(super::catch_job(|| {}), "a normal job reports completed");
    assert!(
        !super::catch_job(|| panic!("boom")),
        "a panicking job is caught and reported, not propagated"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vault-buddy catch_job_survives`
Expected: FAIL — `catch_job` not found.

- [ ] **Step 3: Add the seam and use it in the worker**

Add the seam (module-level in `transcription.rs`):

```rust
/// Run one job body, converting a panic into a `false` so the worker loop can
/// fail just that job and keep going. Mirrors the `catch_unwind` guard lib.rs
/// uses around the metronome tick; the build is not `panic=abort` and the
/// panic hook only logs, so the unwind is catchable on this worker thread.
fn catch_job<F: FnOnce()>(f: F) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_ok()
}
```

In `run_transcription`, replace the direct call

```rust
                process_transcription(&app, &job, &mut loaded);
```

with the guarded form:

```rust
                let completed = catch_job(|| process_transcription(&app, &job, &mut loaded));
                if !completed {
                    // The job panicked. Fail just this recording, drop the
                    // cached model (it may be mid-load / inconsistent), and let
                    // the loop continue — one bad job must not stop the worker.
                    log::error!("transcribe: worker caught a panic on {}", job.mp3.display());
                    loaded = None;
                    fail_transcription(&app, &job.mp3, "internal error during transcription");
                }
```

(The existing `active = None` clear block after it is unchanged and still runs.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vault-buddy catch_job_survives`
Expected: PASS.

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt -p vault-buddy; cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/transcription.rs
git commit -m "fix(shell): catch per-job panics so one bad job can't kill the transcription worker"
```

---

### Task 8: No-swallow sidecar writes + EmitThrottle swap + aligned outcome logs (shell)

Stop silently dropping sidecar-write failures (the sidecar is the note's source of truth), and replace the two inline throttles with the tested `EmitThrottle`.

**Files:**
- Modify: `src-tauri/src/transcription.rs` (`emit_cancelled`, `fail_transcription`, the forced-placeholder + `write_placeholder` writes in `process_transcription`, `ensure_model` download callback, the progress closure)

**Interfaces:**
- Consumes: `vault_buddy_core::throttle::EmitThrottle` (Task 2).

- [ ] **Step 1: Log sidecar-write failures instead of swallowing them**

In `emit_cancelled`, change the swallowed write:

```rust
    if let Err(e) = vault_buddy_core::transcript::replace_if_ours(
        &vault_buddy_core::transcript::transcript_path(mp3),
        &vault_buddy_core::transcript::render_cancelled(&name),
    ) {
        log::warn!("transcribe: writing cancelled sidecar for {} failed: {e}", mp3.display());
    }
```

In `fail_transcription`, change the swallowed write:

```rust
    if let Err(e) = vault_buddy_core::transcript::replace_if_ours(&path, &content) {
        log::warn!("transcribe: writing failed sidecar for {} failed: {e}", mp3.display());
    }
```

In `process_transcription`, the forced-placeholder branch (`force_write_sidecar` for the placeholder) and the plain `write_placeholder(&job.mp3)` call — wrap each:

```rust
            if let Err(e) = vault_buddy_core::transcript::force_write_sidecar(
                &vault_buddy_core::transcript::transcript_path(&job.mp3),
                &vault_buddy_core::transcript::render_placeholder(&name),
            ) {
                log::warn!("transcribe: writing placeholder for {} failed: {e}", job.mp3.display());
            }
```

```rust
    } else if let Err(e) = vault_buddy_core::transcript::write_placeholder(&job.mp3) {
        log::warn!("transcribe: writing placeholder for {} failed: {e}", job.mp3.display());
    }
```

(Leave the `let _ = app.emit(...)` event calls as-is — an emit failure is genuinely ignorable, no window to receive it.)

- [ ] **Step 2: Swap the download throttle to `EmitThrottle`**

Add `use vault_buddy_core::throttle::EmitThrottle;` to the imports. In `ensure_model`, replace `let mut last_emit: u64 = 0;` and the `if received.saturating_sub(last_emit) >= 4_000_000 || Some(received) == total {` gate:

```rust
    let mut throttle = EmitThrottle::new(4_000_000);
    download_model(tier, cancel, &mut |received, total| {
        if throttle.should_emit(received, Some(received) == total) {
            set_phase(&app, Phase::Downloading { received, total });
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({
                    "mp3": mp3.to_string_lossy(),
                    "model": tier.as_str(),
                    "received": received,
                    "total": total,
                }),
            );
        }
    })
```

- [ ] **Step 3: Swap the progress throttles to `EmitThrottle`**

In `process_transcription`, replace `let mut last_sent: i32 = -1;` / `let mut last_logged: i32 = -1;` and the two `p - last_* >= N` gates inside the `on_progress` closure:

```rust
    let mut emit_throttle = EmitThrottle::new(5);
    let mut log_throttle = EmitThrottle::new(25);
    let on_progress: Box<dyn FnMut(i32) + Send> = Box::new(move |p| {
        let p = p.clamp(0, 100);
        progress.store(p as u8, Ordering::Relaxed); // lock-free, no queue mutex
        let terminal = p >= 100;
        if emit_throttle.should_emit(p as u64, terminal) {
            let _ = app_cb.emit(
                "capture:transcribeProgress",
                serde_json::json!({ "mp3": mp3_cb.to_string_lossy(), "progress": p }),
            );
        }
        if log_throttle.should_emit(p as u64, terminal) {
            log::info!("transcribe: {} inference {}%", mp3_cb.display(), p);
        }
    });
```

- [ ] **Step 4: Build + test + fmt + clippy the shell**

Run: `cargo test -p vault-buddy; cargo fmt -p vault-buddy; cargo clippy -p vault-buddy --all-targets -- -D warnings`
Expected: compiles; existing tests pass; clean clippy.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/transcription.rs
git commit -m "fix(shell): log sidecar-write failures and use EmitThrottle for progress/download"
```

---

### Task 9: Dependency review note in DEVELOPMENT.md

The full review table lives in the spec; add a short pointer + summary where contributors look.

**Files:**
- Modify: `docs/DEVELOPMENT.md`

- [ ] **Step 1: Append a transcription-dependencies subsection**

Find the dependencies/architecture area of `docs/DEVELOPMENT.md` and add:

```markdown
### Transcription dependencies

The local speech-to-text path pins `whisper-rs` 0.16 / `whisper-rs-sys` 0.15
deliberately: `src-tauri/transcribe/src/engine.rs` hand-wires the abort and
progress callbacks around upstream whisper-rs bugs (abort UB #277; the
progress/language closure leaks). A future whisper-rs that fixes these would
let us delete that wiring — tracked as a standalone upgrade, not done casually,
having just stabilized the engine. `sha2` verifies downloaded model integrity;
`symphonia` (MP3-only) decodes; `ureq` downloads with connect/idle timeouts.
Full review: `docs/superpowers/specs/2026-07-07-transcription-reliability-and-verification-design.md`.
```

- [ ] **Step 2: Commit**

```powershell
git add docs/DEVELOPMENT.md
git commit -m "docs: note pinned transcription dependencies and the review location"
```

---

### Task 10: Full workspace verification gate

**Files:** none (verification only).

- [ ] **Step 1: Format + core/transcribe checks**

Run: `cargo fmt --check`
Then: `cargo clippy -p vault_buddy_core --all-targets -- -D warnings; cargo test -p vault_buddy_core`
Then: `cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings; cargo test -p vault_buddy_transcribe`
Expected: all clean/green.

- [ ] **Step 2: whisper-feature build + tests**

Run: `cargo clippy -p vault_buddy_transcribe --features whisper --all-targets -- -D warnings; cargo test -p vault_buddy_transcribe --features whisper`
Expected: clean; FFI wiring tests pass; `real_model_transcribes_without_spurious_abort` reports `ignored`.

- [ ] **Step 3: Shell build + tests**

Run: `cargo clippy -p vault-buddy --all-targets -- -D warnings; cargo test -p vault-buddy`
Expected: clean; dedup + `catch_job` tests pass.

- [ ] **Step 4: Frontend gate (must remain green — no frontend changes)**

Run (repo root): `npm run build; npm test`
Expected: `vue-tsc` clean; Vitest all pass.

- [ ] **Step 5: Confirm working tree is committed**

Run: `git status --short`
Expected: empty (every task committed).

---

## Self-Review

**Spec coverage:**
- End-to-end verification → Task 1 (harness + ignored regression test + ground-truth run + gate). ✓
- Module extraction → Task 6; pure logic to core → Task 2 (`EmitThrottle`) + Task 8 (swap). ✓
- No-speech handling → Task 3 (render) + Task 5 (warn + end-to-end test). ✓
- Crash-proof worker → Task 7. ✓
- SHA-256 model integrity → Task 4 (with real pinned hashes). ✓
- No-swallow writes + consistent outcome log + decode/inference timing → Task 8 + Task 5. ✓
- Dependency review → spec appendix + Task 9 pointer. ✓
- Testing/rollout gate → Task 10. ✓

**Placeholder scan:** No TBD/TODO; every code step shows real code; hashes are the verified canonical values; commands are exact. ✓

**Type consistency:** `EmitThrottle::new(u64)` + `should_emit(&mut self, u64, bool) -> bool` used identically in Tasks 2/8. `download_stream(..., expected_sha256: &str, ...)` defined in Task 4 Step 6 and called with `tier.sha256()` (Step 5) and `""` (Step 7) consistently. `catch_job<F: FnOnce()>(f) -> bool` defined and used in Task 7. `render_transcript` signature unchanged (Task 3). Shell items repointed `capture_commands::` → `transcription::` consistently (Task 6). ✓
