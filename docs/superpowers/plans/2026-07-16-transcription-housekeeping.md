# Transcription Housekeeping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detected-language reporting for auto-language vaults and an in-app model management card (list + guarded delete) — per spec `docs/superpowers/specs/2026-07-16-transcription-housekeeping-design.md`, Increment A of the whisper program, on PR #61.

**Architecture:** The `Transcriber` trait's tuple return graduates to an `EngineOutput` struct (mechanical refactor first, behavior second); detection is captured in the engine only when the job ran on auto, threads through `TranscriptMeta`, and renders as an honest stats-row suffix plus a queryable frontmatter line. Model management adds a registry-artifact enumeration + listing helper in the transcribe crate, a purge-request slot in the shell's transcription queue (so the worker releases its mmap'd cached model before a delete), two IPC commands in a new `model_commands.rs`, and a self-contained settings card.

**Tech Stack:** whisper-rs 0.16 (pinned; `full_lang_id_from_state` + crate-level `get_lang_str`), Rust workspace crates, Vue 3 + Vitest.

## Global Constraints

- whisper-rs stays pinned at 0.16; no dependency changes.
- Detection is reported ONLY when `opts.language.is_none()` AND inference actually ran (all-silence short-circuit, degraded runs, pinned languages → `None`). Detection failure is never a job failure.
- A pinned-language transcript must be BYTE-IDENTICAL to today (the existing exact-string render tests are the guarantee — they must pass unmodified).
- Frontmatter key: `detected-language` (kebab-case, yaml-quoted, emitted only when present). Stats row: `| Language | auto (detected: de) |` when present, unchanged otherwise.
- Artifact ids: `"base" | "small" | "medium" | "turbo" | "vad"`. `delete_transcription_model` must validate the id STRICTLY against the artifact list — `ModelTier::from_str` defaults unknown input to Small and MUST NOT be used for deletion (a garbage id would delete the Small model).
- `list_transcription_models` is sync; `delete_transcription_model` is **async** (its bounded retry sleeps; sync commands must never block the main thread). Both registered in `lib.rs`'s `generate_handler!` and AGENTS.md's IPC table (count goes 61 → 63).
- Delete command order: refuse while any transcription job is active → post purge request + notify worker → `spawn_blocking` remove with bounded retry (20 × 100 ms) → `NotFound` counts as success ("the path is clear").
- UI copy: card heading `Transcription models`; VAD row label `VAD (silence filter)`; absent rows show `not downloaded (~<size>)`; the delete confirm names the re-download cost.
- Commit style: Conventional Commits; run `npm run check:loc` after every Rust-touching task (this branch forgot twice; CI caught both).

---

### Task H1: `EngineOutput` struct (mechanical refactor, no behavior change)

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (trait ~line 61-77, `transcribe_recording` destructure, all six test fakes)
- Modify: `src-tauri/transcribe/src/engine.rs` (impl signature + both `Ok(...)` returns)
- Modify: `src-tauri/transcribe/examples/transcribe_file.rs` (destructure)

**Interfaces:**
- Produces (consumed by H2, and the shape Increment B's wire payload will mirror):

```rust
/// What one engine run produced. A struct (not a tuple) so the next field
/// stops rippling through every `Transcriber` implementor — this is the
/// third widening of this signature on one branch.
pub struct EngineOutput {
    pub segments: Vec<Segment>,
    /// Whether this run actually filtered non-speech via VAD (the
    /// EFFECTIVE state — unchanged semantics from the tuple's bool).
    pub vad_engaged: bool,
    /// Whisper's detected language code (e.g. "de"): Some only when the
    /// job ran on auto AND inference actually ran. Captured in H2; every
    /// H1 construction site sets `None`.
    pub detected_language: Option<String>,
}
```

Trait: `fn transcribe(&self, samples: &[f32], opts: &EngineOptions, cancel: &CancelToken, on_progress: Box<dyn FnMut(i32) + Send>) -> Result<EngineOutput, String>;`

- [ ] **Step 1: Make the change compile-driven (the RED is the compile break)**

Capture the baseline: `cd src-tauri/transcribe && cargo test --lib 2>&1 | tail -1` (expect the current pass count — record it). Add the `EngineOutput` struct above the trait in `lib.rs`, change the trait's return type, then follow the compiler through every implementor:
- `engine.rs`: the all-silence short-circuit `Ok((Vec::new(), true))` → `Ok(EngineOutput { segments: Vec::new(), vad_engaged: true, detected_language: None })`; the final `Ok((out, vad_engaged))` → `Ok(EngineOutput { segments: out, vad_engaged, detected_language: None })`.
- `lib.rs` `transcribe_recording`: `let (segments, vad_engaged) = match ...` → `let EngineOutput { segments, vad_engaged, detected_language: _ } = match ...` (H2 consumes the field; the explicit `_` binding documents the interim).
- All six fakes in `lib.rs` tests (`FakeOk`, `FakeEmpty`, `FakeErr`, `FakeCancelsThenSucceeds`, `FakeSeen`, `FakeOkVad`) and `examples/transcribe_file.rs`: same struct construction, `detected_language: None`.

- [ ] **Step 2: Verify byte-identical behavior**

Run: `cd src-tauri/transcribe && cargo test && cargo test --features whisper && cargo clippy --all-targets -- -D warnings`
Expected: the SAME pass counts as the baseline — zero test-body changes beyond construction syntax; any assertion edit means behavior drifted (stop and re-check).
Then: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && npm --prefix /home/user/vault-buddy run check:loc`

- [ ] **Step 3: Commit**

```bash
git add src-tauri/transcribe/src/lib.rs src-tauri/transcribe/src/engine.rs src-tauri/transcribe/examples/transcribe_file.rs
git commit -m "refactor(transcribe): EngineOutput struct replaces the widening result tuple

Third signature change on this branch — future fields (detected language
lands next) stop rippling through every Transcriber implementor. No
behavior change: every construction site sets detected_language: None and
all existing tests pass unmodified."
```

---

### Task H2: detected-language capture + rendering

**Files:**
- Modify: `src-tauri/transcribe/src/engine.rs` (capture after `state.full`)
- Modify: `src-tauri/transcribe/src/lib.rs` (thread into `TranscriptMeta`; one new fake + test)
- Modify: `src-tauri/core/src/transcript.rs` (`TranscriptMeta` field, frontmatter line, stats row, `meta()` fixture, tests)

**Interfaces:**
- Consumes: H1's `EngineOutput.detected_language`.
- Produces: `TranscriptMeta.detected_language: Option<String>`; frontmatter `detected-language: "de"`; stats `| Language | auto (detected: de) |`.

- [ ] **Step 1: Write the failing core tests**

In `src-tauri/core/src/transcript.rs` tests:

```rust
    #[test]
    fn detected_language_renders_in_frontmatter_and_stats_only_when_present() {
        // Auto-language vaults finally learn what whisper detected. The
        // label stays honest ("detected" — whisper's first-window
        // classification, not a guarantee) and the `language:` field keeps
        // recording the SETTING, wire-stable.
        let mut m = meta();
        m.language = None; // auto
        m.detected_language = Some("de".to_string());
        let t = render_transcript(&m, &[]);
        assert!(t.contains(r#"detected-language: "de""#));
        assert!(t.contains("| Language | auto (detected: de) |"));

        // Absent detection: no frontmatter line, plain row — exactly today.
        m.detected_language = None;
        let t = render_transcript(&m, &[]);
        assert!(!t.contains("detected-language"));
        assert!(t.contains("| Language | auto |"));
    }
```

(The pinned-language byte-identity guarantee needs no new test: `real_transcript_is_complete_not_regenerable` and `transcript_ends_with_a_stats_table` already assert exact strings for a pinned `es` transcript and must pass unmodified.)

- [ ] **Step 2: Run to verify compile failure**

Run: `cd src-tauri/core && cargo test detected_language`
Expected: COMPILE ERROR — no field `detected_language`.

- [ ] **Step 3: Implement core**

`TranscriptMeta` gains, after `pub vad: bool,`:

```rust
    /// Whisper's detected language code, present only for auto-language
    /// runs where inference actually ran (see EngineOutput). Renders as an
    /// honest "(detected: xx)" suffix + a queryable frontmatter line —
    /// `language:` itself keeps recording the SETTING.
    pub detected_language: Option<String>,
```

In `render_transcript`, directly after the `language:` frontmatter line:

```rust
    if let Some(detected) = &meta.detected_language {
        out.push_str(&format!("detected-language: {}\n", yaml_quote(detected)));
    }
```

In `render_stats`, the language binding becomes:

```rust
    let language = match (&meta.language, &meta.detected_language) {
        (None, Some(detected)) => format!("auto (detected: {detected})"),
        (setting, _) => setting.as_deref().unwrap_or("auto").to_string(),
    };
```

(and the format string's `| Language | {language} |` stays as-is). Update the core `meta()` fixture with `detected_language: None,`.

- [ ] **Step 4: Engine capture + pipeline threading**

`engine.rs`, after `state.full(params, run_samples)...?` and before the segment loop:

```rust
        // Detection is only meaningful on auto: with a pinned language the
        // id is just the pin echoed back, and reporting it as "detected"
        // would be the setting masquerading as an observation. Failures
        // degrade to None — reporting is garnish, never a job outcome.
        let detected_language = if opts.language.is_none() {
            state
                .full_lang_id_from_state()
                .ok()
                .and_then(whisper_rs::get_lang_str)
                .map(str::to_string)
        } else {
            None
        };
```

(If `get_lang_str`'s parameter type differs from `full_lang_id_from_state`'s return — c_int vs i32 — follow the compiler with an `as` cast at the call site.) The final return carries it: `Ok(EngineOutput { segments: out, vad_engaged, detected_language })`. The all-silence short-circuit stays `None` (whisper never ran).

`lib.rs` `transcribe_recording`: bind the field (`detected_language` instead of `_`) and add to the `TranscriptMeta` construction: `detected_language: detected_language.filter(|_| opts.language.is_none()),` — belt-and-braces: the engine already gates on auto, but the pipeline must not trust an over-eager future implementor (this is exactly the honest-reporting pattern `vad_engaged` uses).

New lib test:

```rust
    struct FakeDetects;
    impl Transcriber for FakeDetects {
        fn transcribe(
            &self,
            _s: &[f32],
            _o: &EngineOptions,
            _c: &CancelToken,
            _p: Box<dyn FnMut(i32) + Send>,
        ) -> Result<EngineOutput, String> {
            Ok(EngineOutput {
                segments: vec![Segment { start_ms: 0, end_ms: 1000, text: "hallo".into() }],
                vad_engaged: false,
                detected_language: Some("de".to_string()),
            })
        }
    }

    #[test]
    fn detected_language_reaches_the_sidecar_on_auto_but_never_on_a_pinned_language() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_tiny_mp3(dir.path());
        // Auto: the detection lands in frontmatter + stats.
        let auto_opts = TranscribeOptions { language: None, ..opts() };
        transcribe_recording(&mp3, &FakeDetects, &auto_opts, "t", false, &CancelToken::new(), noop_progress()).unwrap();
        let text = std::fs::read_to_string(transcript_path(&mp3)).unwrap();
        assert!(text.contains(r#"detected-language: "de""#));
        assert!(text.contains("| Language | auto (detected: de) |"));
        // Pinned (opts() pins "en"): even an engine that (wrongly) reports
        // a detection is filtered at the pipeline — the setting is what
        // renders. force=true because the auto run above already wrote a
        // COMPLETE sidecar this second write must replace.
        transcribe_recording(&mp3, &FakeDetects, &opts(), "t", true, &CancelToken::new(), noop_progress()).unwrap();
        let text = std::fs::read_to_string(transcript_path(&mp3)).unwrap();
        assert!(!text.contains("detected-language"));
        assert!(text.contains("| Language | en |"));
    }
```

(`opts()` is the existing fixture at the bottom of `lib.rs` tests — it pins `language: Some("en")`; struct-update from a fresh `opts()` call is fine, all fields are owned.)

Also extend the `#[ignore]` real-model test: when `VB_TEST_AUDIO` is set and no language pinned, assert `out.detected_language.is_some()` with a comment (manual Windows/real-model run).

- [ ] **Step 5: Verify**

Run: `cd src-tauri/core && cargo test && cd ../transcribe && cargo test && cargo test --features whisper && cargo clippy --all-targets -- -D warnings`
Then: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check` and `npm run check:loc` from the repo root (ratchet with an accurate justification if tripped).
Expected: all green; the pre-existing exact-string render tests pass UNMODIFIED.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/transcript.rs src-tauri/transcribe/src/lib.rs src-tauri/transcribe/src/engine.rs scripts/loc-baseline.json
git commit -m "feat(transcribe): report whisper's detected language on auto-language runs

Stats row reads 'auto (detected: de)' and the sidecar gains a queryable
detected-language frontmatter line — only when the job ran on auto and
inference actually ran; pinned-language transcripts stay byte-identical.
The pipeline re-filters on the setting so an over-eager engine can never
smuggle a 'detection' into a pinned-language transcript."
```

---

### Task H3: model artifact registry + listing helper

**Files:**
- Modify: `src-tauri/transcribe/src/model.rs` (artifact enumeration + listing; tests)

**Interfaces:**
- Produces (consumed by H5):

```rust
pub struct ModelArtifact {
    pub id: &'static str,        // "base"|"small"|"medium"|"turbo"|"vad"
    pub file_name: &'static str,
    /// For UI display on absent rows; exact for turbo/vad (their pins
    /// record the real size), approximate for the older tiers.
    pub approx_download_bytes: u64,
}
pub fn model_artifacts() -> [ModelArtifact; 5];
pub struct ArtifactStatus {
    pub id: String,
    pub file_name: String,
    pub present: bool,
    pub size_bytes: Option<u64>, // real on-disk size when present
}
pub fn list_artifacts_in(dir: &Path) -> Vec<ArtifactStatus>;
```

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn model_artifacts_cover_every_tier_plus_vad_with_valid_ids() {
        let arts = model_artifacts();
        let ids: Vec<_> = arts.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["base", "small", "medium", "turbo", "vad"]);
        // Every speech tier's file name agrees with the tier registry —
        // the card and the downloader must never disagree on a path.
        for t in [ModelTier::Base, ModelTier::Small, ModelTier::Medium, ModelTier::Turbo] {
            assert!(arts.iter().any(|a| a.file_name == t.file_name()), "{t:?}");
        }
        assert!(arts.iter().any(|a| a.file_name == VAD_MODEL_FILE));
        assert!(arts.iter().all(|a| a.approx_download_bytes > 0));
    }

    #[test]
    fn list_artifacts_reports_presence_and_real_sizes_ignoring_part_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ggml-base.bin"), vec![0u8; 1234]).unwrap();
        // A half-download must not read as present (the janitor's business).
        std::fs::write(dir.path().join("ggml-small.bin.part"), b"partial").unwrap();
        let statuses = list_artifacts_in(dir.path());
        assert_eq!(statuses.len(), 5);
        let base = statuses.iter().find(|s| s.id == "base").unwrap();
        assert!(base.present);
        assert_eq!(base.size_bytes, Some(1234));
        let small = statuses.iter().find(|s| s.id == "small").unwrap();
        assert!(!small.present);
        assert_eq!(small.size_bytes, None);
    }
```

- [ ] **Step 2: Run to verify compile failure**

Run: `cd src-tauri/transcribe && cargo test model_artifacts`
Expected: COMPILE ERROR.

- [ ] **Step 3: Implement**

```rust
/// The complete set of downloadable model artifacts — the four speech
/// tiers plus the Silero VAD model — for the models-management surface.
/// One list so the card, the deleter, and the downloaders can never
/// disagree about what exists or where it lives.
pub struct ModelArtifact {
    pub id: &'static str,
    pub file_name: &'static str,
    /// UI display for absent rows. Exact for turbo/vad (their download
    /// pins record the real byte counts); round approximations for the
    /// older tiers, whose pins only record a floor.
    pub approx_download_bytes: u64,
}

pub fn model_artifacts() -> [ModelArtifact; 5] {
    [
        ModelArtifact { id: "base", file_name: ModelTier::Base.file_name(), approx_download_bytes: 148_000_000 },
        ModelArtifact { id: "small", file_name: ModelTier::Small.file_name(), approx_download_bytes: 488_000_000 },
        ModelArtifact { id: "medium", file_name: ModelTier::Medium.file_name(), approx_download_bytes: 1_600_000_000 },
        ModelArtifact { id: "turbo", file_name: ModelTier::Turbo.file_name(), approx_download_bytes: 574_041_195 },
        ModelArtifact { id: "vad", file_name: VAD_MODEL_FILE, approx_download_bytes: 885_098 },
    ]
}

pub struct ArtifactStatus {
    pub id: String,
    pub file_name: String,
    pub present: bool,
    pub size_bytes: Option<u64>,
}

/// Stat every known artifact under `dir`. Takes the dir (rather than
/// resolving %APPDATA% itself) so it's testable against a tempdir — the
/// command layer feeds it `model_dir()`.
pub fn list_artifacts_in(dir: &Path) -> Vec<ArtifactStatus> {
    model_artifacts()
        .into_iter()
        .map(|a| {
            let size_bytes = std::fs::metadata(dir.join(a.file_name)).ok().map(|m| m.len());
            ArtifactStatus {
                id: a.id.to_string(),
                file_name: a.file_name.to_string(),
                present: size_bytes.is_some(),
                size_bytes,
            }
        })
        .collect()
}
```

- [ ] **Step 4: Verify + commit**

Run: `cd src-tauri/transcribe && cargo test && cargo clippy --all-targets -- -D warnings`, then fmt + check:loc from the roots as usual.

```bash
git add src-tauri/transcribe/src/model.rs scripts/loc-baseline.json
git commit -m "feat(transcribe): model artifact registry + on-disk listing

One enumeration (four tiers + silero) so the models card, deleter, and
downloaders can never disagree; listing takes the dir for tempdir tests
and ignores .part files (a half-download is the janitor's business)."
```

---

### Task H4: purge-request slot in the transcription queue

**Files:**
- Modify: `src-tauri/src/transcription.rs` (queue field + methods + worker loop + tests)

**Interfaces:**
- Consumes: nothing new.
- Produces (consumed by H5): `pub(crate) fn request_model_purge(app: &AppHandle, id: &str)` and `pub(crate) fn is_any_transcription_active(app: &AppHandle) -> bool`.

- [ ] **Step 1: Write the failing queue tests**

```rust
    #[test]
    fn purge_request_round_trips_and_is_one_shot() {
        let mut q = TranscriptionQueue::default();
        assert_eq!(q.take_purge(), None);
        q.request_purge("small");
        assert_eq!(q.take_purge(), Some("small".to_string()));
        assert_eq!(q.take_purge(), None, "one-shot: taken means gone");
        // A second request before the worker wakes overwrites — deleting
        // two models back-to-back must not strand the first request as a
        // stale drop of the wrong tier later.
        q.request_purge("base");
        q.request_purge("turbo");
        assert_eq!(q.take_purge(), Some("turbo".to_string()));
    }

    #[test]
    fn any_active_reflects_the_active_slot() {
        // The delete command's refusal gate, at the queue-logic level:
        // deleting a model out from under a running job would race its
        // guaranteed-live mmap (and possibly its terminal write's tier).
        let mut q = TranscriptionQueue::default();
        assert!(!q.any_active());
        q.active = Some(active_job("X")); // existing test helper
        assert!(q.any_active());
    }
```

- [ ] **Step 2: Run to verify compile failure**

Run: `cd src-tauri && cargo test -p vault-buddy --lib purge_request`
Expected: COMPILE ERROR.

- [ ] **Step 3: Implement**

`TranscriptionQueue` gains:

```rust
    /// A one-shot request (artifact id) for the worker to drop its cached
    /// transcriber before the delete command unlinks the model file —
    /// whisper.cpp mmaps the model, and Windows refuses to delete a mapped
    /// file, so an idle worker's cache would otherwise block deletion
    /// forever. Latest-wins on overwrite (see the test).
    pending_purge: Option<String>,
```

with methods on the impl:

```rust
    fn request_purge(&mut self, id: &str) {
        self.pending_purge = Some(id.to_string());
    }
    fn take_purge(&mut self) -> Option<String> {
        self.pending_purge.take()
    }
    /// Whether the worker is presently on a job — the delete command's
    /// refusal gate (see model_commands.rs).
    fn any_active(&self) -> bool {
        self.active.is_some()
    }
```

Worker loop (`run_transcription`, the wait scope currently at ~line 763): the idle wait must also wake for a purge, and the purge must be applied to the thread-local `loaded` cache. Today's block is a bare scope:

```rust
                {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    while guard.pending.is_empty() {
                        // The Condvar guard is poisonable too — recover it the
                        // same way `lock_ignoring_poison` recovers the mutex, so
                        // a panic elsewhere can't wedge the worker permanently on
                        // a poisoned wait.
                        guard = state.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                    }
                }
```

Replace it with (the peek-only comment above the scope and everything after — the `is_recording` gate onward — stay untouched; a purge posted while the worker sleeps in that gate's 30 s retry is still taken here on the next loop iteration, since the wait condition is already false):

```rust
                let purge = {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    while guard.pending.is_empty() && guard.pending_purge.is_none() {
                        // The Condvar guard is poisonable too — recover it the
                        // same way `lock_ignoring_poison` recovers the mutex, so
                        // a panic elsewhere can't wedge the worker permanently on
                        // a poisoned wait.
                        guard = state.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                    }
                    guard.take_purge()
                };
                // Drop the cached transcriber BEFORE any delete attempt can
                // race the mmap (the requesting command retries the unlink
                // while we get here). "vad" is accepted as a no-op for
                // symmetry — the worker never caches the silero model.
                if let Some(id) = purge {
                    if loaded.as_ref().map(|(t, _, _)| t.as_str()) == Some(id.as_str()) {
                        log::info!("transcribe: dropping cached {id} model for deletion");
                        loaded = None;
                    }
                    // A purge with no pending work: loop back to the wait
                    // rather than falling through to the recording gate.
                    let state = app.state::<TranscriptionState>();
                    let guard = lock_ignoring_poison(&state.inner);
                    if guard.pending.is_empty() {
                        continue;
                    }
                }
```

(`ModelTier::as_str()` exists — `model.rs:32` — and returns the tier key, which matches the artifact ids for the four speech tiers.)

Shell helpers (beside `enqueue_transcription`):

```rust
/// Post a one-shot cache-purge request and wake the worker — the delete
/// command's first half (see model_commands.rs for the second).
pub(crate) fn request_model_purge(app: &AppHandle, id: &str) {
    let state = app.state::<TranscriptionState>();
    let mut guard = lock_ignoring_poison(&state.inner);
    guard.request_purge(id);
    state.cv.notify_all();
}

/// Whether ANY transcription job is currently in flight — the delete
/// command refuses while one is (its terminal write may target the model
/// being deleted, and mid-inference the mmap is guaranteed live).
pub(crate) fn is_any_transcription_active(app: &AppHandle) -> bool {
    let state = app.state::<TranscriptionState>();
    let guard = lock_ignoring_poison(&state.inner);
    guard.any_active()
}
```

- [ ] **Step 4: Verify + commit**

Run: `cd src-tauri && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` (needs `../dist`; `npm run build` first if the container recycled), then `npm run check:loc` (transcription.rs is a ratcheted hotspot — append an accurate justification if tripped).

```bash
git add src-tauri/src/transcription.rs scripts/loc-baseline.json
git commit -m "feat(shell): one-shot model-purge request in the transcription queue

The worker drops its cached (mmap'd) transcriber on request so a model
delete can unlink the file on Windows; latest-wins overwrite, wakes the
idle wait, and a purge with no pending work loops back to waiting."
```

---

### Task H5: `list_transcription_models` + `delete_transcription_model` commands

**Files:**
- Create: `src-tauri/src/model_commands.rs`
- Modify: `src-tauri/src/lib.rs` (`mod model_commands;` + both handlers in `generate_handler!`)

**Interfaces:**
- Consumes: H3's `model_artifacts`/`list_artifacts_in`, H4's `request_model_purge`/`is_any_transcription_active`, `model::model_dir()`.
- Produces (consumed by H6): wire DTO `[{ id, fileName, present, sizeBytes: number|null, approxDownloadBytes }]`; `delete_transcription_model(id)` errors: `"Unknown model id: <id>"`, `"A transcription is running — try again when it finishes."`, and the still-locked message below.

- [ ] **Step 1: Write the failing test**

In the new `src-tauri/src/model_commands.rs` (tests at the bottom):

```rust
    #[test]
    fn delete_rejects_unknown_ids_strictly() {
        // ModelTier::from_str defaults unknown input to Small — using it
        // here would let a garbage id delete the Small model. The command
        // must validate against the artifact list instead.
        assert!(artifact_file_name("garbage").is_none());
        assert!(artifact_file_name("").is_none());
        assert_eq!(artifact_file_name("small").unwrap(), "ggml-small.bin");
        assert_eq!(artifact_file_name("vad").unwrap(), "ggml-silero-v5.1.2.bin");
    }
```

- [ ] **Step 2: Run to verify compile failure**

Run: `cd src-tauri && cargo test -p vault-buddy --lib delete_rejects`
Expected: COMPILE ERROR (module/fn missing).

- [ ] **Step 3: Implement**

```rust
//! Model-management IPC: list the transcription model cache and delete a
//! cached artifact so the next job re-downloads it (SHA-verified) — the
//! user-facing remedy for a suspect cached model (docs/Gaps.md GAP-14).

use std::path::PathBuf;
use tauri::AppHandle;
use vault_buddy_transcribe::model::{list_artifacts_in, model_artifacts, model_dir};

use crate::transcription::{is_any_transcription_active, request_model_purge};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatusDto {
    pub id: String,
    pub file_name: String,
    pub present: bool,
    pub size_bytes: Option<u64>,
    pub approx_download_bytes: u64,
}

/// Strict id → file-name lookup. Deliberately NOT ModelTier::from_str,
/// which defaults unknown input to Small — a typo'd id must be an error,
/// never a deletion of the wrong model.
fn artifact_file_name(id: &str) -> Option<&'static str> {
    model_artifacts()
        .into_iter()
        .find(|a| a.id == id)
        .map(|a| a.file_name)
}

#[tauri::command]
pub fn list_transcription_models() -> Vec<ModelStatusDto> {
    let approx: std::collections::HashMap<&str, u64> = model_artifacts()
        .into_iter()
        .map(|a| (a.id, a.approx_download_bytes))
        .collect();
    let Some(dir) = model_dir() else {
        return Vec::new(); // unresolvable %APPDATA%: an empty card, not an error
    };
    list_artifacts_in(&dir)
        .into_iter()
        .map(|s| ModelStatusDto {
            approx_download_bytes: approx.get(s.id.as_str()).copied().unwrap_or(0),
            id: s.id,
            file_name: s.file_name,
            present: s.present,
            size_bytes: s.size_bytes,
        })
        .collect()
}

/// Async: the bounded retry below sleeps while the worker drops its
/// cached (mmap'd) transcriber — a sync command would block the main
/// thread for up to ~2 s.
#[tauri::command]
pub async fn delete_transcription_model(app: AppHandle, id: String) -> Result<(), String> {
    let file_name = artifact_file_name(&id).ok_or_else(|| format!("Unknown model id: {id}"))?;
    if is_any_transcription_active(&app) {
        return Err("A transcription is running — try again when it finishes.".to_string());
    }
    let dir = model_dir().ok_or("cannot resolve model directory")?;
    let path: PathBuf = dir.join(file_name);
    request_model_purge(&app, &id);
    tauri::async_runtime::spawn_blocking(move || {
        // Ride out the worker's cache drop: Windows refuses to unlink a
        // file the (idle) worker still has mapped, and the purge request
        // is serviced on its thread's next wake. 20 × 100 ms bounds the
        // wait; NotFound is success (the contract is "the path is clear").
        let mut last_err: Option<std::io::Error> = None;
        for _ in 0..20 {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        Err(format!(
            "Couldn't delete the model — it is still in use ({}). It will be deletable after the next transcription finishes or an app restart.",
            last_err.map(|e| e.to_string()).unwrap_or_default()
        ))
    })
    .await
    .map_err(|e| format!("delete task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    // (Step 1's test lives here.)
}
```

In `lib.rs`: add `mod model_commands;` beside the other command modules and register `model_commands::list_transcription_models, model_commands::delete_transcription_model` in `generate_handler!`. Check whether H3's items need `pub` re-exports through `vault_buddy_transcribe::model` (they are declared `pub` in H3 — the `use` above works as written).

- [ ] **Step 4: Verify + commit**

Run: `cd src-tauri && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`, then `npm run check:loc`.

```bash
git add src-tauri/src/model_commands.rs src-tauri/src/lib.rs scripts/loc-baseline.json
git commit -m "feat(shell): list/delete transcription models over the artifact registry

Strict id validation (from_str's default-to-Small must never route a
deletion), active-job refusal, purge-then-bounded-retry unlink for the
Windows mmap lock, NotFound-as-success. Delete is async — the retry
sleeps and sync commands must never block the main thread."
```

---

### Task H6: Transcription models card (UI)

**Files:**
- Create: `src/components/TranscriptionModelsCard.vue`
- Modify: `src/components/BuddySettings.vue` (mount in `#integrations`, after the GPU card)
- Modify: `src/types.ts` (`TranscriptionModelStatus`)
- Test: `tests/transcription-models-card.test.ts`

**Interfaces:**
- Consumes: H5's wire DTO + error strings.
- Produces: testids `model-row-<id>`, `model-delete-<id>`, `model-confirm-<id>`, `model-cancel-<id>`, `models-error`.

- [ ] **Step 1: Write the failing tests**

```ts
import { flushPromises, mount } from "@vue/test-utils";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

import TranscriptionModelsCard from "../src/components/TranscriptionModelsCard.vue";

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  vi.clearAllMocks();
});

const MODELS = [
  { id: "base", fileName: "ggml-base.bin", present: false, sizeBytes: null, approxDownloadBytes: 148_000_000 },
  { id: "small", fileName: "ggml-small.bin", present: true, sizeBytes: 487_654_321, approxDownloadBytes: 488_000_000 },
  { id: "vad", fileName: "ggml-silero-v5.1.2.bin", present: true, sizeBytes: 885_098, approxDownloadBytes: 885_098 },
];

function mountWith(failDelete = false) {
  const calls: { cmd: string; payload?: unknown }[] = [];
  mockIPC((cmd, payload) => {
    calls.push({ cmd, payload });
    if (cmd === "list_transcription_models") return MODELS;
    if (cmd === "delete_transcription_model") {
      if (failDelete) throw new Error("still in use");
      return null;
    }
    return null;
  });
  active = mount(TranscriptionModelsCard, { attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TranscriptionModelsCard", () => {
  it("lists every artifact with real sizes for present and approx for absent", async () => {
    const { wrapper } = mountWith();
    await flushPromises();
    expect(wrapper.get('[data-testid="model-row-small"]').text()).toContain("465 MB");
    expect(wrapper.get('[data-testid="model-row-base"]').text()).toContain("not downloaded");
    expect(wrapper.get('[data-testid="model-row-base"]').text()).toContain("141 MB");
    // Absent rows have no delete affordance.
    expect(wrapper.find('[data-testid="model-delete-base"]').exists()).toBe(false);
  });

  it("delete requires the in-panel confirm; cancel makes no IPC call", async () => {
    const { wrapper, calls } = mountWith();
    await flushPromises();
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    // Confirm state visible, nothing deleted yet.
    expect(calls.some((c) => c.cmd === "delete_transcription_model")).toBe(false);
    await wrapper.get('[data-testid="model-cancel-small"]').trigger("click");
    expect(calls.some((c) => c.cmd === "delete_transcription_model")).toBe(false);
    // Confirm path actually deletes and re-lists.
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    await wrapper.get('[data-testid="model-confirm-small"]').trigger("click");
    await flushPromises();
    const del = calls.find((c) => c.cmd === "delete_transcription_model");
    expect(del?.payload).toEqual({ id: "small" });
    expect(calls.filter((c) => c.cmd === "list_transcription_models").length).toBe(2);
  });

  it("surfaces a failed delete inline and keeps the row", async () => {
    const { wrapper } = mountWith(true);
    await flushPromises();
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    await wrapper.get('[data-testid="model-confirm-small"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="models-error"]').text()).toContain("still in use");
    expect(wrapper.find('[data-testid="model-row-small"]').exists()).toBe(true);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/transcription-models-card.test.ts`
Expected: FAIL (component missing).

- [ ] **Step 3: Implement**

`src/types.ts`:

```ts
/** One transcription model artifact's cache status (models card). */
export interface TranscriptionModelStatus {
  id: string;
  fileName: string;
  present: boolean;
  sizeBytes: number | null;
  approxDownloadBytes: number;
}
```

`TranscriptionModelsCard.vue` — self-contained card (match the surrounding card markup in BuddySettings' Integrations tab exactly, like TranscriptionAppSettings did). Script logic:

```ts
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { TranscriptionModelStatus } from "../types";

const LABELS: Record<string, string> = {
  base: "Base",
  small: "Small",
  medium: "Medium",
  turbo: "Turbo",
  vad: "VAD (silence filter)",
};

const models = ref<TranscriptionModelStatus[]>([]);
const confirmingId = ref<string | null>(null);
const busyId = ref<string | null>(null);
const error = ref<string | null>(null);

function formatSize(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  return `${Math.round(bytes / 1_048_576)} MB`;
}

async function refresh() {
  try {
    models.value = await invoke<TranscriptionModelStatus[]>("list_transcription_models");
  } catch (e) {
    error.value = String(e);
    logWarning(`list_transcription_models failed: ${String(e)}`);
  }
}

onMounted(refresh);

async function confirmDelete(id: string) {
  confirmingId.value = null;
  busyId.value = id;
  error.value = null;
  try {
    await invoke("delete_transcription_model", { id });
    await refresh();
  } catch (e) {
    error.value = String(e);
    logWarning(`delete_transcription_model(${id}) failed: ${String(e)}`);
  } finally {
    busyId.value = null;
  }
}
```

Template — the exact card shell `TranscriptionAppSettings.vue` uses (section → uppercase h2 → `rounded-xl border border-white/10 bg-white/5` body; error = `text-xs text-rose-400`):

```html
<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Transcription models
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div
        v-for="m in models"
        :key="m.id"
        :data-testid="`model-row-${m.id}`"
        class="flex items-center justify-between gap-2"
      >
        <div class="text-sm text-slate-200">
          {{ LABELS[m.id] ?? m.id }}
          <span class="block text-xs text-slate-500">
            <template v-if="m.present">{{ formatSize(m.sizeBytes ?? 0) }}</template>
            <template v-else>not downloaded (~{{ formatSize(m.approxDownloadBytes) }})</template>
          </span>
        </div>
        <div
          v-if="confirmingId === m.id"
          class="flex items-center gap-1.5 text-right"
        >
          <span class="text-xs text-slate-400">
            Deleting frees the disk — downloading again costs
            ~{{ formatSize(m.approxDownloadBytes) }}.
          </span>
          <button
            :data-testid="`model-confirm-${m.id}`"
            type="button"
            class="rounded-md bg-rose-500/20 px-2 py-1 text-xs text-rose-300 hover:bg-rose-500/30 disabled:opacity-50"
            :disabled="busyId === m.id"
            @click="confirmDelete(m.id)"
          >
            Delete
          </button>
          <button
            :data-testid="`model-cancel-${m.id}`"
            type="button"
            class="rounded-md bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10"
            @click="confirmingId = null"
          >
            Cancel
          </button>
        </div>
        <button
          v-else-if="m.present"
          :data-testid="`model-delete-${m.id}`"
          type="button"
          class="rounded-md bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10 disabled:opacity-50"
          :disabled="busyId !== null"
          @click="confirmingId = m.id"
        >
          Delete
        </button>
      </div>
      <p
        v-if="error"
        data-testid="models-error"
        class="text-xs text-rose-400"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
```

(Exact Tailwind classes may be adjusted to taste, but the shell, testids, copy, and disabled/confirm mechanics are contract.) Mount: in `BuddySettings.vue`'s `<template #integrations>` block (currently `McpSettings` → `DocumentImportSettings` → `TranscriptionAppSettings`), add `<TranscriptionModelsCard />` directly after `<TranscriptionAppSettings />`, with the matching import.

- [ ] **Step 4: Verify + commit**

Run: `npx vitest run && npm run build && npm run lint && npm run check:loc`
Expected: green (extend any BuddySettings mount test additively if it inventories the tab).

```bash
git add src/components/TranscriptionModelsCard.vue src/components/BuddySettings.vue src/types.ts tests/transcription-models-card.test.ts
git commit -m "feat(ui): transcription models card — cache visibility + guarded delete

Every artifact with its real size or approximate download cost; delete
behind an in-panel confirm naming the re-download price; errors inline.
The user-facing face of the delete-to-redownload self-heal (GAP-14)."
```

---

### Task H7: documentation

**Files:**
- Modify: `AGENTS.md` — IPC table gains a `model_commands.rs` row (`list_transcription_models`, `delete_transcription_model` *(async — the delete's bounded retry must not sit on the main thread)*); the lead count sentence 61 → 63; the transcription-domain section gains a short paragraph (detected-language semantics: auto-only, honest labeling, `detected-language` frontmatter; the models card + purge/delete guard) and the whisper-rs upgrade tracked-trigger note (0.16.0 is the newest release — verified on crates.io 2026-07-16; a git pin violates deny.toml's `unknown-git = "deny"`; upgrade when 0.17 ships, with the hand-wired trampoline regression tests as the acceptance gate).
- Modify: `docs/DEVELOPMENT.md` — the models-on-disk section notes the in-app card (list + delete-to-redownload) and the `detected-language` sidecar field.
- Modify: `docs/Gaps.md` — GAP-14 gains one sentence: the Transcription models card (Buddy settings → Integrations) is the user-facing remedy for a suspect cached model.

Every claim must be verified against the shipped code before writing (command names, sync/async markers, frontmatter key, card heading). Verify the true `generate_handler!` count before writing "63".

- [ ] Commit:

```bash
git add AGENTS.md docs/DEVELOPMENT.md docs/Gaps.md
git commit -m "docs: detected language, models card, and the whisper-rs upgrade trigger"
```

---

### Task H8: full gates

Run ALL, in order (fix-per-policy on failure, never lower a floor):

1. `cd src-tauri && cargo fmt --check`
2. `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test`
3. `cd src-tauri/transcribe && cargo clippy --all-targets -- -D warnings && cargo test && cargo test --features whisper`
4. `cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib && cargo machete .`
5. `npm run lint && npm run check:loc && rm -rf coverage && npm run check:quality && npm run test:coverage && npm run build`

(`cargo deny` not required: no Cargo manifest changes this increment — flag if that assumption breaks.) Do NOT push (controller pushes, updates the PR body, and runs the final increment review).
