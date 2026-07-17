# Transcription Accuracy & Speed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Custom vocabulary + recording title primed into whisper's `initial_prompt`, a Turbo model tier (`large-v3-turbo-q5_0`), and default-on Silero VAD silence skipping — per spec `docs/superpowers/specs/2026-07-16-transcription-accuracy-and-speed-design.md`.

**Architecture:** Three thin verticals through existing seams. Two new per-vault config fields flow `config.json → VaultCaptureConfig → CaptureConfigDto → TranscriptionSettings.vue`. The engine's `Transcriber` trait widens its `language` parameter into a borrowed `EngineOptions` struct carrying language + initial prompt + VAD model path. The Turbo tier and the silero artifact ride the existing pinned-SHA download machinery unchanged. The shell composes the prompt and resolves the VAD model per job, degrading to no-VAD (warn, still transcribe) when the silero download fails.

**Tech Stack:** Rust (workspace crates `vault_buddy_core`, `vault_buddy_transcribe`, shell `vault-buddy`), whisper-rs 0.16 (pinned — do NOT bump), Vue 3 + Vitest, serde_json hand-rolled config parse/serialize.

## Global Constraints

- whisper-rs stays pinned at `0.16` — no Cargo version changes.
- Config parse is per-field defensive: a malformed value defaults only itself, never the entry or file.
- Optional config fields serialize only when non-default (`transcriptionVocabulary` only when Some, `transcriptionVad` only when `false`) — the hand-editable file stays minimal.
- Wire keys are camelCase: `transcriptionVocabulary`, `transcriptionVad`; model value string is `"turbo"`.
- Turbo pin: file `ggml-large-v3-turbo-q5_0.bin`, SHA-256 `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2`, real size 574,041,195 bytes, floor 500_000_000. (Verified against the Hugging Face LFS pointer at `https://huggingface.co/ggerganov/whisper.cpp/raw/main/ggml-large-v3-turbo-q5_0.bin`.)
- Silero pin: file `ggml-silero-v5.1.2.bin`, URL `https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin`, SHA-256 `29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf`, real size 885,098 bytes, floor 500_000. (Verified against `https://huggingface.co/ggml-org/whisper-vad/raw/main/ggml-silero-v5.1.2.bin`.)
- Prompt order: **title first, vocabulary last** (whisper keeps the trailing `n_text_ctx/2` tokens of an over-long prompt — the user's explicit vocabulary must survive truncation).
- VAD model download failure degrades to a no-VAD run with `log::warn!` — it must NOT fail the job. A cancel during that download is still a cancel.
- The composed prompt is never written into the transcript sidecar.
- Every new spawned thread would need a name — this plan spawns none.
- Commit style: Conventional Commits (`feat(core): …`, `feat(transcribe): …`, `feat(shell): …`, `feat(ui): …`, `docs: …`), imperative subject, body explains why.
- Rust gates per crate: `cargo fmt --check` (workspace), `cargo clippy --all-targets -- -D warnings`, `cargo test`. Frontend gates: `npm run lint`, `npx vitest run`, `npm run build` (vue-tsc).
- Committer identity must already be `git config user.email noreply@anthropic.com` / `git config user.name Claude` (set in this session; verify with `git config user.email` before the first commit).

---

### Task 1: Per-vault config fields (`transcription_vocabulary`, `transcription_vad`)

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs` (struct ~line 61-86, Default ~line 88-111, `vault_entry` ~line 209-266, `serialize_vault_entry` ~line 289-320, tests)

**Interfaces:**
- Consumes: nothing new.
- Produces: `VaultCaptureConfig.transcription_vocabulary: Option<String>` (trimmed, non-empty when Some) and `VaultCaptureConfig.transcription_vad: bool` (default `true`) — read by Tasks 8 and 9.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/core/src/vault_config.rs`:

```rust
    #[test]
    fn transcription_vocabulary_and_vad_parse_and_default() {
        // Defaults: no vocabulary, VAD on.
        let d = VaultCaptureConfig::default();
        assert_eq!(d.transcription_vocabulary, None);
        assert!(d.transcription_vad, "VAD defaults on");
        // Parse both; vocabulary is trimmed, whitespace-only reads as None.
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "transcriptionVocabulary": "  Kubernetes, Anna Kowalska  ",
                "transcriptionVad": false
            }, "b": { "transcriptionVocabulary": "   " } } }"#,
        );
        let a = vault_config(&cfg, "a");
        assert_eq!(
            a.transcription_vocabulary.as_deref(),
            Some("Kubernetes, Anna Kowalska")
        );
        assert!(!a.transcription_vad);
        assert_eq!(vault_config(&cfg, "b").transcription_vocabulary, None);
        // Malformed values default only themselves (the file is hand-edited).
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "transcriptionVocabulary": 7,
                "transcriptionVad": "no",
                "mode": "voice-note"
            } } }"#,
        );
        let v = vault_config(&cfg, "a");
        assert_eq!(v.transcription_vocabulary, None);
        assert!(v.transcription_vad, "malformed bool falls back to on");
        assert_eq!(v.mode, RecordingMode::VoiceNote);
    }

    #[test]
    fn transcription_vocabulary_and_vad_round_trip_and_stay_minimal() {
        let mut cfg = AppConfig::default();
        cfg.vaults.insert(
            "a".to_string(),
            VaultCaptureConfig {
                transcription_vocabulary: Some("ggml, rmcp".to_string()),
                transcription_vad: false,
                ..VaultCaptureConfig::default()
            },
        );
        let json = serialize_config(&cfg);
        let parsed = parse_config(&json);
        assert_eq!(
            parsed.vaults["a"].transcription_vocabulary.as_deref(),
            Some("ggml, rmcp")
        );
        assert!(!parsed.vaults["a"].transcription_vad);
        // Defaults are omitted — the hand-editable file stays minimal.
        let mut cfg2 = AppConfig::default();
        cfg2.vaults
            .insert("b".to_string(), VaultCaptureConfig::default());
        let json2 = serialize_config(&cfg2);
        assert!(!json2.contains("transcriptionVocabulary"), "got: {json2}");
        assert!(!json2.contains("transcriptionVad"), "got: {json2}");
    }
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cd src-tauri/core && cargo test transcription_vocabulary`
Expected: COMPILE ERROR — `struct VaultCaptureConfig has no field named transcription_vocabulary`.

- [ ] **Step 3: Add the fields**

In the `VaultCaptureConfig` struct, directly after `pub transcript_timestamps: bool,`:

```rust
    /// Free-text vocabulary (names, acronyms, project terms) composed into
    /// whisper's initial prompt. None → no priming. Stored trimmed and
    /// non-empty only — a whitespace value reads back as None.
    pub transcription_vocabulary: Option<String>,
    /// Skip non-speech via Silero VAD before inference. Default on: meetings
    /// transcribe faster and whisper stops hallucinating into silence.
    pub transcription_vad: bool,
```

In `impl Default`, after `transcript_timestamps: true,`:

```rust
            transcription_vocabulary: None,
            transcription_vad: true,
```

In `vault_entry`, after the `transcript_timestamps` field:

```rust
        transcription_vocabulary: entry
            .get("transcriptionVocabulary")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        transcription_vad: entry
            .get("transcriptionVad")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.transcription_vad),
```

In `serialize_vault_entry`, after the `transcriptTimestamps` insert:

```rust
    if let Some(vocabulary) = &v.transcription_vocabulary {
        entry.insert(
            "transcriptionVocabulary".to_string(),
            json!(vocabulary),
        );
    }
    if !v.transcription_vad {
        entry.insert("transcriptionVad".to_string(), json!(false));
    }
```

Also extend the exhaustive struct literal in the existing test
`config_round_trips_through_serialize_and_parse` (it lists every field), after
`transcript_timestamps: false,`:

```rust
                transcription_vocabulary: Some("Vault Buddy".to_string()),
                transcription_vad: false,
```

- [ ] **Step 4: Run the core suite**

Run: `cd src-tauri/core && cargo test`
Expected: PASS (all tests, including the two new ones).

- [ ] **Step 5: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/core/src/vault_config.rs
git commit -m "feat(core): per-vault transcription vocabulary + VAD config fields

transcriptionVocabulary (trimmed, absent when empty) and transcriptionVad
(default on) parse per-field defensively and serialize only when
non-default, per the accuracy & speed spec."
```

---

### Task 2: `capture_title` — strip the capture prefix from a base name

**Files:**
- Modify: `src-tauri/core/src/capture_paths.rs` (add function near `is_capture_base` ~line 55; tests at the bottom)

**Interfaces:**
- Consumes: `is_capture_base(base: &str) -> bool`, `const CAPTURE_PREFIX_CHARS: usize = 16` (both already in this module).
- Produces: `pub fn capture_title(base: &str) -> &str` — used by Task 8's `initial_prompt_for`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/core/src/capture_paths.rs` (a `fn date()` helper already exists there):

```rust
    #[test]
    fn capture_title_strips_the_prefix_and_keeps_suffixes() {
        assert_eq!(
            capture_title("2026-07-16 0930 Budget review with Anna"),
            "Budget review with Anna"
        );
        assert_eq!(capture_title("2026-07-16 0930 Meeting (2)"), "Meeting (2)");
        // Round-trip with the generator every capture name comes from.
        let base = base_name(date(), 14, 5, "Standup");
        assert_eq!(capture_title(&base), "Standup");
        // Unicode after the ASCII prefix must not panic or split a char.
        assert_eq!(capture_title("2026-07-16 0930 Café müsli"), "Café müsli");
    }

    #[test]
    fn capture_title_passes_a_non_capture_name_through() {
        assert_eq!(capture_title("download"), "download");
        assert_eq!(capture_title(""), "");
    }
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cd src-tauri/core && cargo test capture_title`
Expected: COMPILE ERROR — `cannot find function capture_title`.

- [ ] **Step 3: Implement**

Add after `is_capture_base` (below line 55):

```rust
/// The human title of a capture base name: everything after the
/// `YYYY-MM-DD HHmm ` prefix. `is_capture_base` guarantees the first
/// `CAPTURE_PREFIX_CHARS` characters are ASCII (digits/dash/space), so the
/// byte slice below can never split a UTF-8 sequence. A non-capture name
/// passes through unchanged (defensive — callers only hand in capture
/// names, and the title's use is prompt priming, where the whole name is
/// still a harmless prompt).
pub fn capture_title(base: &str) -> &str {
    if is_capture_base(base) {
        &base[CAPTURE_PREFIX_CHARS..]
    } else {
        base
    }
}
```

Note: `CAPTURE_PREFIX_CHARS` is declared further down the file (~line 79) as a
private const — same module, so the reference compiles; no visibility change
needed.

- [ ] **Step 4: Run the core suite**

Run: `cd src-tauri/core && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/capture_paths.rs
git commit -m "feat(core): capture_title extracts the human title from a capture base

The transcription prompt needs the recording's title; the 16-char prefix
is ASCII by is_capture_base's own validation, so the slice is safe."
```

---

### Task 3: `compose_initial_prompt` (transcribe crate)

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (add public fn near `inference_failure_message` ~line 89; tests in `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn compose_initial_prompt(title: &str, vocabulary: Option<&str>) -> Option<String>` — used by Task 8's `initial_prompt_for`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/transcribe/src/lib.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cd src-tauri/transcribe && cargo test compose_initial_prompt`
Expected: COMPILE ERROR — `cannot find function compose_initial_prompt`.

- [ ] **Step 3: Implement**

Add after `inference_failure_message` (before `WHISPER_MAX_THREADS`):

```rust
/// Compose whisper's `initial_prompt` from the recording's title and the
/// vault's custom vocabulary. Title FIRST, vocabulary LAST: whisper keeps
/// only the trailing `n_text_ctx/2` tokens of an over-long prompt (it
/// truncates from the front), so the user's explicit vocabulary is the part
/// that must survive. `None` when there is nothing to prime with — whisper
/// then behaves exactly as it did before this feature existed.
pub fn compose_initial_prompt(title: &str, vocabulary: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = [Some(title), vocabulary]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(". "))
    }
}
```

- [ ] **Step 4: Run the transcribe suite**

Run: `cd src-tauri/transcribe && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): compose_initial_prompt joins title + vocabulary

Title first, vocabulary last — whisper truncates over-long prompts from
the front, and the user's explicit vocabulary must survive."
```

---

### Task 4: Turbo model tier

**Files:**
- Modify: `src-tauri/transcribe/src/model.rs` (enum ~line 12, every `match` arm, tests)

**Interfaces:**
- Consumes: existing tier machinery.
- Produces: `ModelTier::Turbo` (`from_str("turbo")`, `as_str() == "turbo"`, `label() == "whisper-turbo"`, `file_name() == "ggml-large-v3-turbo-q5_0.bin"`) — reachable from config in Task 8, validated in Task 9, offered by UI in Task 11.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/transcribe/src/model.rs` tests, extend the three existing tests:

In `tier_from_str_defaults_to_small` add:

```rust
        assert_eq!(ModelTier::from_str("turbo"), ModelTier::Turbo);
```

In `tier_files_urls_and_labels` add:

```rust
        assert_eq!(ModelTier::Turbo.file_name(), "ggml-large-v3-turbo-q5_0.bin");
        assert!(ModelTier::Turbo
            .url()
            .ends_with("/ggml-large-v3-turbo-q5_0.bin"));
        assert_eq!(ModelTier::Turbo.label(), "whisper-turbo");
        assert_eq!(ModelTier::Turbo.as_str(), "turbo");
```

In `tier_sha256_values_are_lowercase_hex_of_expected_length` change the array to:

```rust
        for t in [
            ModelTier::Base,
            ModelTier::Small,
            ModelTier::Medium,
            ModelTier::Turbo,
        ] {
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cd src-tauri/transcribe && cargo test tier_`
Expected: COMPILE ERROR — `no variant named Turbo`.

- [ ] **Step 3: Add the variant and arms**

Enum:

```rust
pub enum ModelTier {
    Base,
    Small,
    Medium,
    Turbo,
}
```

`from_str` gains (before the default arm):

```rust
            "turbo" => ModelTier::Turbo,
```

`as_str` gains:

```rust
            ModelTier::Turbo => "turbo",
```

`file_name` gains:

```rust
            ModelTier::Turbo => "ggml-large-v3-turbo-q5_0.bin",
```

`sha256` gains (comment stays accurate — same HF repo):

```rust
            ModelTier::Turbo => "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
```

`min_size` gains:

```rust
            ModelTier::Turbo => 500_000_000,    // ~574 MB (574,041,195 bytes)
```

- [ ] **Step 4: Run the transcribe suite**

Run: `cd src-tauri/transcribe && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/transcribe/src/model.rs
git commit -m "feat(transcribe): Turbo model tier (large-v3-turbo-q5_0)

Smaller than Medium (~574 MB vs ~1.5 GB), more accurate, faster on CPU.
SHA-256 pinned from the Hugging Face LFS pointer; same download/verify
machinery as the existing tiers."
```

---

### Task 5: Silero VAD model artifact

**Files:**
- Modify: `src-tauri/transcribe/src/model.rs` (new consts + two fns after `model_path` ~line 86; tests)

**Interfaces:**
- Consumes: `model_dir()`, `model_download_agent()`, `download_stream(...)` (all already in this module).
- Produces: `pub fn vad_model_path() -> Option<PathBuf>`, `pub fn download_vad_model(cancel: &CancelToken, on_progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf, String>`, `pub const VAD_MODEL_FILE: &str` — used by Task 8's `ensure_vad_model` and Task 6's ignored real-model test.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/transcribe/src/model.rs`:

```rust
    #[test]
    fn vad_model_lives_in_the_models_dir_with_a_pinned_hash() {
        if let (Some(vad), Some(dir)) = (vad_model_path(), model_dir()) {
            assert_eq!(vad.parent(), Some(dir.as_path()));
            assert_eq!(
                vad.file_name().unwrap().to_string_lossy(),
                "ggml-silero-v5.1.2.bin"
            );
        }
        // Same hex discipline as the tier hashes.
        assert_eq!(VAD_MODEL_SHA256.len(), 64);
        assert!(VAD_MODEL_SHA256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(VAD_MODEL_URL.starts_with("https://huggingface.co/ggml-org/whisper-vad"));
        assert!(VAD_MODEL_URL.ends_with("/ggml-silero-v5.1.2.bin"));
    }

    #[test]
    fn precancelled_vad_download_bails_without_touching_the_network() {
        // Mirrors precancelled_download_bails_without_touching_the_network:
        // hermetic in CI precisely because the abort happens before any
        // request is made.
        let cancel = crate::CancelToken::new();
        cancel.cancel();
        let mut progress = |_received: u64, _total: Option<u64>| {};
        assert!(
            download_vad_model(&cancel, &mut progress).is_err(),
            "a pre-cancelled VAD download must not proceed to the network"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cd src-tauri/transcribe && cargo test vad_`
Expected: COMPILE ERROR — `cannot find function vad_model_path` / missing consts.

- [ ] **Step 3: Implement**

Add after `model_path` (~line 86):

```rust
/// The Silero VAD model whisper.cpp uses to skip non-speech before
/// inference. Deliberately NOT a `ModelTier` — it is not a speech model the
/// user picks; it rides along whenever a vault's "Skip silence" is on. Same
/// pinned-URL + SHA-256 + `.part`-then-rename discipline as the tiers.
pub const VAD_MODEL_FILE: &str = "ggml-silero-v5.1.2.bin";
const VAD_MODEL_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin";
/// Canonical SHA-256 of the file above (ggml-org/whisper-vad on Hugging Face).
const VAD_MODEL_SHA256: &str = "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf";
/// Sanity floor, not a checksum (the real file is 885,098 bytes).
const VAD_MODEL_MIN_SIZE: u64 = 500_000;

pub fn vad_model_path() -> Option<PathBuf> {
    model_dir().map(|d| d.join(VAD_MODEL_FILE))
}

/// Download the Silero VAD model with progress — `download_model`'s exact
/// contract at ~1 MB scale: skips if present, cancellable per chunk,
/// `.part`-then-rename, checksum-verified.
pub fn download_vad_model(
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    // Already cancelled: do no work and open no connection.
    if cancel.is_cancelled() {
        return Err("cancelled".to_string());
    }
    let dir = model_dir().ok_or("cannot resolve model directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create model dir: {e}"))?;
    let dest = dir.join(VAD_MODEL_FILE);
    if dest.exists() {
        return Ok(dest);
    }
    let agent = model_download_agent();
    download_stream(
        &agent,
        VAD_MODEL_URL,
        &dir,
        VAD_MODEL_FILE,
        VAD_MODEL_MIN_SIZE,
        VAD_MODEL_SHA256,
        cancel,
        on_progress,
        std::time::Duration::from_secs(60),
    )
}
```

- [ ] **Step 4: Run the transcribe suite**

Run: `cd src-tauri/transcribe && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/transcribe/src/model.rs
git commit -m "feat(transcribe): pinned Silero VAD model artifact + download

Not a ModelTier (it is not a user-picked speech model) — a sibling
artifact in the models dir reusing download_stream's .part/SHA-256
discipline. ~1 MB from ggml-org/whisper-vad."
```

---

### Task 6: `EngineOptions` — widen the `Transcriber` trait and wire the engine

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (trait ~line 45-53, `TranscribeOptions` ~line 55-59, `transcribe_recording` engine call ~line 158-169, test fakes + `opts()` fixture)
- Modify: `src-tauri/transcribe/src/engine.rs` (`transcribe` impl ~line 190-234, ignored real-model test ~line 352-388)

**Interfaces:**
- Consumes: Task 5's `vad_model_path` (test only), `WhisperVadParams` from whisper-rs.
- Produces: the new trait signature every implementor must match —

```rust
pub struct EngineOptions<'a> {
    pub language: Option<&'a str>,
    pub initial_prompt: Option<&'a str>,
    pub vad_model: Option<&'a Path>,
}
pub trait Transcriber {
    fn transcribe(
        &self,
        samples: &[f32],
        opts: &EngineOptions,
        cancel: &CancelToken,
        on_progress: Box<dyn FnMut(i32) + Send>,
    ) -> Result<Vec<Segment>, String>;
}
```

and `TranscribeOptions` gaining `pub initial_prompt: Option<String>` + `pub vad_model: Option<PathBuf>` — constructed by Task 8.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src-tauri/transcribe/src/lib.rs`:

```rust
    #[test]
    fn engine_options_reach_the_transcriber() {
        // The prompt/VAD knobs must actually arrive at the engine — a
        // TranscribeOptions field nobody forwards would silently do nothing.
        use std::sync::Mutex;
        struct FakeSeen(Arc<Mutex<Option<(Option<String>, Option<PathBuf>)>>>);
        impl Transcriber for FakeSeen {
            fn transcribe(
                &self,
                _s: &[f32],
                opts: &EngineOptions,
                _c: &CancelToken,
                _p: Box<dyn FnMut(i32) + Send>,
            ) -> Result<Vec<Segment>, String> {
                *self.0.lock().unwrap() = Some((
                    opts.initial_prompt.map(str::to_string),
                    opts.vad_model.map(Path::to_path_buf),
                ));
                Ok(vec![])
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
                Some("Standup. cpal".to_string()),
                Some(PathBuf::from("/models/silero.bin"))
            ))
        );
    }
```

- [ ] **Step 2: Run to verify it fails to compile**

Run: `cd src-tauri/transcribe && cargo test engine_options_reach`
Expected: COMPILE ERROR — `cannot find type EngineOptions` / no field `initial_prompt`.

- [ ] **Step 3: Implement in lib.rs**

Above the `Transcriber` trait, add (and change the trait as shown in
**Interfaces** above — replace the `language: Option<&str>` parameter with
`opts: &EngineOptions`):

```rust
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
```

Update the trait doc comment: replace the sentence about `language` with
"`opts` carries the language plus the prompt/VAD knobs (see `EngineOptions`)".

`TranscribeOptions` becomes:

```rust
pub struct TranscribeOptions {
    pub language: Option<String>,
    pub timestamps: bool,
    pub model_label: String,
    /// Composed title+vocabulary priming (see `compose_initial_prompt`).
    pub initial_prompt: Option<String>,
    /// Resolved Silero model path when this job runs with VAD.
    pub vad_model: Option<PathBuf>,
}
```

In `transcribe_recording`, replace the engine call with:

```rust
    let engine_opts = EngineOptions {
        language: opts.language.as_deref(),
        initial_prompt: opts.initial_prompt.as_deref(),
        vad_model: opts.vad_model.as_deref(),
    };
    let segments = match transcriber.transcribe(&samples, &engine_opts, cancel, on_progress) {
```

Update the four existing fakes' signatures (`FakeOk`, `FakeEmpty`, `FakeErr`
in lib.rs tests — `_l: Option<&str>` becomes `_o: &EngineOptions`; `FakeErr`
keeps its `cancel` parameter) and the `opts()` fixture:

```rust
    fn opts() -> TranscribeOptions {
        TranscribeOptions {
            language: Some("en".into()),
            timestamps: true,
            model_label: "whisper-small".into(),
            initial_prompt: None,
            vad_model: None,
        }
    }
```

- [ ] **Step 4: Implement in engine.rs**

Change the impl signature:

```rust
impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &self,
        samples: &[f32],
        opts: &crate::EngineOptions,
        cancel: &CancelToken,
        on_progress: Box<dyn FnMut(i32) + Send>,
    ) -> Result<Vec<Segment>, String> {
```

Replace `if let Some(lang) = language {` with `if let Some(lang) = opts.language {`
(the leak NOTE comment above it stays). Directly after that `set_language`
block, add:

```rust
        if let Some(prompt) = opts.initial_prompt {
            // NOTE: same bounded upstream leak class as `set_language` above —
            // whisper-rs `CString::into_raw()`s the prompt and `FullParams`
            // has no `Drop` reclaiming it. A prompt is a title plus a short
            // vocabulary line, a few hundred bytes per job at most; accepted
            // for the same pub(crate) reason documented on set_language.
            params.set_initial_prompt(prompt);
        }
        if let Some(vad_model) = opts.vad_model {
            // Skip non-speech before inference. whisper.cpp maps segment
            // timestamps back to the ORIGINAL audio timeline after VAD
            // filtering, so sidecar timestamps are unaffected. Params are
            // whisper.cpp's own defaults (threshold 0.5, min speech 250 ms,
            // min silence 100 ms, pad 30 ms) — deliberately no user knobs.
            params.enable_vad(true);
            params.set_vad_model_path(Some(vad_model.to_string_lossy().as_ref()));
            params.set_vad_params(whisper_rs::WhisperVadParams::default());
        }
```

(If the whisper-rs 0.16 signatures differ slightly — e.g. `set_vad_model_path`
taking `&str` instead of `Option<&str>`, or `WhisperVadParams::new()` instead
of `default()` — follow the compiler; the docs.rs listing for 0.16.0 shows
`enable_vad(bool)`, `set_vad_model_path(Option<&str>)`,
`set_vad_params(WhisperVadParams)` with `Default` implemented.)

In the `#[ignore]` test `real_model_transcribes_without_spurious_abort`,
replace the `t.transcribe(...)` call with:

```rust
        // Optional priming/VAD paths for a manual (Windows dev / local) run:
        // VB_TEST_VOCAB primes the prompt; VAD engages when the silero model
        // is already cached. Both default off so the test's original -6
        // regression coverage is unchanged.
        let vocab = std::env::var("VB_TEST_VOCAB").ok();
        let vad = crate::model::vad_model_path().filter(|p| p.exists());
        let opts = crate::EngineOptions {
            language: None,
            initial_prompt: vocab.as_deref(),
            vad_model: vad.as_deref(),
        };
        let out = t.transcribe(&samples, &opts, &cancel, Box::new(|_| {}));
```

and extend the `VB_TEST_AUDIO` branch at the end:

```rust
        if std::env::var("VB_TEST_AUDIO").is_ok() {
            let segments = out.unwrap();
            assert!(
                !segments.is_empty(),
                "a real speech clip must yield at least one segment"
            );
            // With or without VAD, timestamps must stay on the original
            // timeline: monotonically non-decreasing starts, end >= start.
            for w in segments.windows(2) {
                assert!(w[0].start_ms <= w[1].start_ms, "segment starts out of order");
            }
            assert!(segments.iter().all(|s| s.end_ms >= s.start_ms));
        }
```

- [ ] **Step 5: Run the transcribe suite (default features)**

Run: `cd src-tauri/transcribe && cargo test`
Expected: PASS (engine.rs is feature-gated off here; the lib fakes prove the trait).

- [ ] **Step 6: Compile-gate the engine with the whisper feature**

Run: `cd src-tauri/transcribe && cargo test --features whisper`
Expected: compiles and PASSES (the two wired-callback regression tests run; the real-model test stays ignored). This is the step that catches any whisper-rs signature drift in the VAD/prompt setters.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/transcribe/src/lib.rs src-tauri/transcribe/src/engine.rs
git commit -m "feat(transcribe): EngineOptions carries initial_prompt + VAD into whisper

Widens Transcriber's language parameter into a borrowed options struct so
the next knob doesn't ripple through every implementor. Engine sets
set_initial_prompt (same bounded leak class as set_language, documented)
and enable_vad + silero path + whisper.cpp-default VAD params."
```

---

### Task 7: `TranscriptMeta.vad` + Statistics row

**Files:**
- Modify: `src-tauri/core/src/transcript.rs` (`TranscriptMeta` ~line 24-32, `render_stats` ~line 130-165, `meta()` fixture + new test)
- Modify: `src-tauri/transcribe/src/lib.rs` (`TranscriptMeta` construction ~line 192-200)

**Interfaces:**
- Consumes: Task 6's `TranscribeOptions.vad_model`.
- Produces: `TranscriptMeta.vad: bool`; stats row `| Silence skipping (VAD) | on/off |`.

- [ ] **Step 1: Write the failing test**

In `src-tauri/core/src/transcript.rs` tests, append:

```rust
    #[test]
    fn stats_report_vad_on_or_off() {
        // The one visible trace of silence skipping: a transcript missing a
        // quiet aside must be self-explaining ("VAD was on"), and a degraded
        // (no-VAD) run must honestly say off even in a VAD-enabled vault.
        let mut m = meta();
        m.vad = true;
        assert!(render_transcript(&m, &[]).contains("| Silence skipping (VAD) | on |"));
        m.vad = false;
        assert!(render_transcript(&m, &[]).contains("| Silence skipping (VAD) | off |"));
    }
```

- [ ] **Step 2: Run to verify it fails to compile**

Run: `cd src-tauri/core && cargo test stats_report_vad`
Expected: COMPILE ERROR — no field `vad` on `TranscriptMeta`.

- [ ] **Step 3: Implement**

`TranscriptMeta` gains, after `pub processing_secs: u64,`:

```rust
    /// Whether this run skipped silence via Silero VAD (the effective state:
    /// a degraded run in a VAD-enabled vault reports false).
    pub vad: bool,
```

In `render_stats`, add one row after the `| Language | {language} |` line —
change the format string block to include:

```rust
         | Language | {language} |\n\
         | Silence skipping (VAD) | {vad} |\n\
```

binding, next to the existing `let language = ...`:

```rust
    let vad = if meta.vad { "on" } else { "off" };
```

Update the core `meta()` fixture (add `vad: true,`), and in
`src-tauri/transcribe/src/lib.rs`:
- the `TranscriptMeta` construction in `transcribe_recording` gains
  `vad: opts.vad_model.is_some(),`
- the tests' `opts()` fixture already has `vad_model: None` (Task 6), so no
  transcribe-side test change is needed.

- [ ] **Step 4: Run both crates' suites**

Run: `cd src-tauri/core && cargo test && cd ../transcribe && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/transcript.rs src-tauri/transcribe/src/lib.rs
git commit -m "feat(core): transcript stats report silence skipping (VAD) on/off

The effective state, not the setting: a degraded no-VAD run in a
VAD-enabled vault honestly reports off."
```

---

### Task 8: Shell orchestration — compose the prompt, resolve the VAD model

**Files:**
- Modify: `src-tauri/src/transcription.rs` (imports ~line 19-22, `process_transcription` ~line 427-478, new helpers + tests)

**Interfaces:**
- Consumes: `capture_title` (Task 2), `compose_initial_prompt` (Task 3), `vad_model_path`/`download_vad_model` (Task 5), `TranscribeOptions` fields (Task 6), config fields (Task 1).
- Produces: fully wired `process_transcription`; helper `fn initial_prompt_for(mp3: &Path, vocabulary: Option<&str>) -> Option<String>` (private, unit-tested).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/src/transcription.rs`:

```rust
    #[test]
    fn initial_prompt_for_composes_title_and_vocabulary() {
        let mp3 = Path::new("/v/Meetings/2026/07/2026-07-16 0930 Budget review.mp3");
        assert_eq!(
            initial_prompt_for(mp3, Some("Kubernetes, rmcp")),
            Some("Budget review. Kubernetes, rmcp".to_string())
        );
        assert_eq!(
            initial_prompt_for(mp3, None),
            Some("Budget review".to_string())
        );
    }

    #[test]
    fn initial_prompt_for_is_none_when_there_is_nothing_to_prime_with() {
        // A non-capture stem passes through capture_title unchanged and still
        // primes (harmless), but an empty stem + no vocabulary must be None so
        // whisper runs exactly as before this feature.
        assert_eq!(initial_prompt_for(Path::new(""), None), None);
        assert_eq!(
            initial_prompt_for(Path::new("/x/download.mp3"), None),
            Some("download".to_string())
        );
    }
```

- [ ] **Step 2: Run to verify they fail to compile**

Run: `cd src-tauri && cargo test -p vault-buddy --lib initial_prompt_for`
Expected: COMPILE ERROR — `cannot find function initial_prompt_for`.
(Note: the shell crate's tests need the GUI libs and a built `../dist` — in this
environment run `npm run setup:linux` once and `npm run build` first if not
already done. If the full shell build is too heavy at this point, defer the
run to Step 5's suite run — the compile error is equally visible there.)

- [ ] **Step 3: Implement the helpers**

Update the model import (~line 19):

```rust
use vault_buddy_transcribe::model::{
    download_model, download_vad_model, model_path, vad_model_path, ModelTier,
};
```

Add above `process_transcription`:

```rust
/// The composed `initial_prompt` for a job: the recording's current title
/// (its stem minus the `YYYY-MM-DD HHmm ` capture prefix) plus the vault's
/// custom vocabulary. Pure so it's unit-testable on Linux;
/// `process_transcription` feeds it the live config.
fn initial_prompt_for(mp3: &Path, vocabulary: Option<&str>) -> Option<String> {
    let stem = mp3
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = vault_buddy_core::capture_paths::capture_title(&stem);
    vault_buddy_transcribe::compose_initial_prompt(title, vocabulary)
}

/// Ensure the Silero VAD model is on disk, downloading (progress rides the
/// existing `capture:modelDownload` event with model:"vad") if missing.
/// Failure is NOT a job failure — the caller degrades to a no-VAD run: the
/// user's intent is "transcribe my meeting", and a ~1 MB optional accelerant
/// must never block that. A cancel mid-download is still a real cancel (the
/// caller consults the token, exactly like the main-model path).
fn ensure_vad_model(app: &AppHandle, mp3: &Path, cancel: &CancelToken) -> Result<PathBuf, String> {
    if let Some(p) = vad_model_path() {
        if p.exists() {
            return Ok(p);
        }
    }
    log::info!("transcribe: downloading the silero VAD model");
    let app = app.clone();
    let mp3 = mp3.to_path_buf();
    // ~885 KB file: a 200 KB emit step yields a handful of updates.
    let mut throttle = EmitThrottle::new(200_000);
    download_vad_model(cancel, &mut |received, total| {
        if throttle.should_emit(received, Some(received) == total) {
            set_phase(&app, Phase::Downloading { received, total });
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({
                    "mp3": mp3.to_string_lossy(),
                    "model": "vad",
                    "received": received,
                    "total": total,
                }),
            );
        }
    })
}
```

- [ ] **Step 4: Wire `process_transcription`**

Directly after the `let model = match ensure_model(...) { ... };` block (and
BEFORE the existing `set_phase(app, Phase::Preparing);` / `capture:modelReady`
emit, so the download phases stay contiguous), insert:

```rust
    // Resolve the Silero model only for VAD-enabled vaults. A download
    // failure DEGRADES — the job still transcribes, just without silence
    // skipping (the warning below and the stats row's "off" are the traces) —
    // unless the error was actually our own cancel.
    let vad_model = if cfg.transcription_vad {
        match ensure_vad_model(app, &job.mp3, &cancel) {
            Ok(p) => Some(p),
            Err(e) => {
                if cancel.is_cancelled() {
                    return emit_cancelled(app, &job.mp3);
                }
                log::warn!(
                    "transcribe: VAD model unavailable, transcribing {} without silence skipping: {e}",
                    job.mp3.display()
                );
                None
            }
        }
    } else {
        None
    };
```

Replace the `TranscribeOptions` construction with:

```rust
    let opts = TranscribeOptions {
        language: cfg.transcription_language.clone(),
        timestamps: cfg.transcript_timestamps,
        model_label: tier.label(),
        initial_prompt: initial_prompt_for(&job.mp3, cfg.transcription_vocabulary.as_deref()),
        vad_model,
    };
```

- [ ] **Step 5: Build the shell (Linux compile gate) and run its tests**

Run (once per container): `npm run setup:linux && npm run build`
Then: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: PASS including the two new `initial_prompt_for` tests.
Then: `cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/transcription.rs
git commit -m "feat(shell): compose the whisper prompt and resolve the VAD model per job

Title + per-vault vocabulary via initial_prompt_for (pure, unit-tested);
ensure_vad_model reuses the capture:modelDownload event (model:'vad') and
degrades to a no-VAD run on failure — a ~1 MB accelerant must never fail
the transcription. Cancel during the VAD download stays a real cancel."
```

---

### Task 9: DTO + model validation gate

**Files:**
- Modify: `src-tauri/src/capture_config_commands.rs` (const ~line 15, DTO ~line 17-55, `set_capture_config` ~line 108-138, tests)

**Interfaces:**
- Consumes: Task 1's config fields.
- Produces: `CaptureConfigDto.transcription_vocabulary: Option<String>` / `transcription_vad: bool` (wire: `transcriptionVocabulary` / `transcriptionVad` via the existing `rename_all = "camelCase"`); `TRANSCRIPTION_MODELS` accepting `"turbo"` — consumed by the frontend in Tasks 10-11.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests`:

```rust
    #[test]
    fn transcription_models_gate_includes_every_tier_the_ui_offers() {
        // The settings dropdown (TranscriptionSettings.vue MODELS) and this
        // validation gate must agree — a tier the UI offers that this array
        // misses would fail every save with "Unknown transcription model".
        for m in ["base", "small", "medium", "turbo"] {
            assert!(
                TRANSCRIPTION_MODELS.contains(&m),
                "{m} missing from the gate"
            );
        }
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test -p vault-buddy --lib transcription_models_gate`
Expected: FAIL — `turbo missing from the gate`.

- [ ] **Step 3: Implement**

```rust
pub const TRANSCRIPTION_MODELS: [&str; 4] = ["base", "small", "medium", "turbo"];
```

`CaptureConfigDto` gains, after `pub transcript_timestamps: bool,`:

```rust
    /// Free-text vocabulary primed into whisper's initial prompt; None/blank
    /// = no priming.
    pub transcription_vocabulary: Option<String>,
    /// Skip silence via Silero VAD before inference (default on).
    pub transcription_vad: bool,
```

`from_config` gains:

```rust
            transcription_vocabulary: v.transcription_vocabulary.clone(),
            transcription_vad: v.transcription_vad,
```

`set_capture_config`'s `VaultCaptureConfig` construction gains, after
`transcript_timestamps: cfg.transcript_timestamps,`:

```rust
        // Blank/whitespace vocabulary collapses to None (no priming), the
        // same treatment transcription_language gets one line up.
        transcription_vocabulary: cfg
            .transcription_vocabulary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        transcription_vad: cfg.transcription_vad,
```

- [ ] **Step 4: Run the shell tests**

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/capture_config_commands.rs
git commit -m "feat(shell): capture-config DTO carries vocabulary + VAD; turbo passes the model gate"
```

---

### Task 10: Frontend types + config owners (compile-through, no UI yet)

**Files:**
- Modify: `src/types.ts` (`CaptureConfig` ~line 99-116, `RecordingSettingsValue` ~line 131-144)
- Modify: `src/components/RecordingConfigTab.vue` (seed ~line 25-38, TEXT_KEYS ~line 12, save mapping ~line 51-58, load mapping ~line 101-108)
- Modify: `src/components/RecordMode.vue` (default config literal ~line 44-56)
- Modify: any test literal the typecheck flags (known: `tests/record-mode.test.ts`, `tests/recording-config-tab.test.ts`, `tests/recording-settings.test.ts`, `tests/capture-settings.test.ts`)

**Interfaces:**
- Consumes: Task 9's wire fields.
- Produces: `CaptureConfig.transcriptionVocabulary: string | null` + `transcriptionVad: boolean`; `RecordingSettingsValue.transcriptionVocabulary: string` + `transcriptionVad: boolean` — consumed by Task 11's UI.

- [ ] **Step 1: Add the type fields**

`src/types.ts`, `CaptureConfig` after `transcriptTimestamps: boolean;`:

```ts
  /** Free-text vocabulary (names, acronyms, jargon) primed into whisper's
   * initial prompt. null = none. */
  transcriptionVocabulary: string | null;
  /** Skip silence via Silero VAD before inference (default on). */
  transcriptionVad: boolean;
```

`RecordingSettingsValue` after `transcriptTimestamps: boolean;`:

```ts
  transcriptionVocabulary: string;
  transcriptionVad: boolean;
```

- [ ] **Step 2: Run the typecheck to enumerate every construction site**

Run: `npm run build`
Expected: FAIL with TS2739/TS2345 errors at each object literal missing the
new fields — this is the authoritative list for Step 3 (it must include
RecordingConfigTab.vue's seed + load/save mappings and RecordMode.vue's
default config literal).

- [ ] **Step 3: Update the owners**

`RecordingConfigTab.vue`:
- seed `rec` literal gains: `transcriptionVocabulary: "",` and `transcriptionVad: true,`
- `TEXT_KEYS` (free-text fields debounce instead of saving per keystroke):

```ts
const TEXT_KEYS = new Set<keyof RecordingSettingsValue>([
  "meetingFolder",
  "voiceNoteFolder",
  "transcriptionVocabulary",
]);
```

- save mapping (inside the `cfg:` object sent to `set_capture_config`), after
  `transcriptTimestamps: r.transcriptTimestamps,`:

```ts
        transcriptionVocabulary: r.transcriptionVocabulary.trim() || null,
        transcriptionVad: r.transcriptionVad,
```

- load mapping (from `get_capture_config`), after
  `transcriptTimestamps: cfg.transcriptTimestamps,`:

```ts
      transcriptionVocabulary: cfg.transcriptionVocabulary ?? "",
      transcriptionVad: cfg.transcriptionVad,
```

`RecordMode.vue` default `config` literal gains:

```ts
  transcriptionVocabulary: null,
  transcriptionVad: true,
```

- [ ] **Step 4: Update test literals flagged by vitest/tsc**

Run: `npm run build && npx vitest run`
For each flagged literal in `tests/record-mode.test.ts`,
`tests/recording-config-tab.test.ts`, `tests/recording-settings.test.ts`,
`tests/capture-settings.test.ts`: add `transcriptionVocabulary: null,` (for
`CaptureConfig`-shaped mocks) or `transcriptionVocabulary: "",` (for
`RecordingSettingsValue`-shaped literals) plus `transcriptionVad: true,`.
Expected after fixes: build PASS, vitest PASS (no behavior change yet — the
fields only flow through).

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/components/RecordingConfigTab.vue src/components/RecordMode.vue tests/
git commit -m "feat(ui): thread transcriptionVocabulary + transcriptionVad through config types and owners

Vocabulary joins TEXT_KEYS so typing debounces instead of saving per
keystroke. No visible UI yet — the fields round-trip load/save unchanged."
```

---

### Task 11: TranscriptionSettings UI — vocabulary, Skip silence, Turbo

**Files:**
- Modify: `src/components/TranscriptionSettings.vue` (interface ~line 6-11, computeds ~line 44-59, `MODELS` ~line 61, template)
- Modify: `src/components/RecordingSettings.vue` (bundle computed ~line 59-71)
- Modify: `src/components/RecordMode.vue` (`transcription` computed ~line 106-125)
- Test: `tests/transcription-settings.test.ts`

**Interfaces:**
- Consumes: Task 10's `RecordingSettingsValue`/`CaptureConfig` fields.
- Produces: `TranscriptionSettingsValue` gains `transcriptionVocabulary: string` + `transcriptionVad: boolean`; test ids `transcription-vocabulary-input`, `transcription-vad-toggle`, option `transcription-model-select-option-turbo`; ids `capture-transcription-vocabulary`, `capture-transcription-vad-toggle` (idPrefix-scoped like the rest).

- [ ] **Step 1: Write the failing tests**

In `tests/transcription-settings.test.ts`, extend `baseValue`:

```ts
const baseValue = {
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
  transcriptionVocabulary: "",
  transcriptionVad: true,
};
```

Extend the hidden-while-off test with two lines:

```ts
    expect(wrapper.find('[data-testid="transcription-vocabulary-input"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="transcription-vad-toggle"]').exists()).toBe(false);
```

Append new tests:

```ts
  it("renders vocabulary and VAD from modelValue once transcribe is on", () => {
    const wrapper = mountWith({
      ...baseValue,
      transcribe: true,
      transcriptionVocabulary: "Kubernetes, rmcp",
      transcriptionVad: false,
    });
    expect(
      wrapper.get<HTMLTextAreaElement>('[data-testid="transcription-vocabulary-input"]').element
        .value,
    ).toBe("Kubernetes, rmcp");
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="transcription-vad-toggle"]').element.checked,
    ).toBe(false);
  });

  it("editing the vocabulary emits update:modelValue with only that field changed", async () => {
    const modelValue = { ...baseValue, transcribe: true };
    const wrapper = mountWith(modelValue);
    await wrapper
      .get('[data-testid="transcription-vocabulary-input"]')
      .setValue("Anna Kowalska, ggml");
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...modelValue, transcriptionVocabulary: "Anna Kowalska, ggml" }],
    ]);
  });

  it("toggling Skip silence emits update:modelValue with only transcriptionVad changed", async () => {
    const modelValue = { ...baseValue, transcribe: true, transcriptionVad: true };
    const wrapper = mountWith(modelValue);
    await wrapper.get('[data-testid="transcription-vad-toggle"]').setValue(false);
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...modelValue, transcriptionVad: false }],
    ]);
  });

  it("offers Turbo in the model dropdown and emits it when picked", async () => {
    const modelValue = { ...baseValue, transcribe: true };
    const wrapper = mountWith(modelValue);
    await pickOption(wrapper, "transcription-model-select", "turbo");
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...modelValue, transcriptionModel: "turbo" }],
    ]);
  });

  it("scopes the vocabulary/VAD id-for pairs with idPrefix too", () => {
    active = mount(TranscriptionSettings, {
      props: { modelValue: { ...baseValue, transcribe: true }, idPrefix: "record-" },
      attachTo: document.body,
    });
    const wrapper = active;
    const vocab = wrapper.get('[data-testid="transcription-vocabulary-input"]');
    expect(vocab.attributes("id")).toBe("record-capture-transcription-vocabulary");
    const vad = wrapper.get('[data-testid="transcription-vad-toggle"]');
    expect(vad.attributes("id")).toBe("record-capture-transcription-vad-toggle");
    wrapper.get(`label[for="${vocab.attributes("id")}"]`);
    wrapper.get(`label[for="${vad.attributes("id")}"]`);
  });
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/transcription-settings.test.ts`
Expected: FAIL — the new testids don't exist; `turbo` option missing.
(TS may also flag the two RecordingSettings/RecordMode bundle producers now
that `TranscriptionSettingsValue` is about to widen — fix them in Step 3.)

- [ ] **Step 3: Implement**

`TranscriptionSettings.vue` — interface:

```ts
interface TranscriptionSettingsValue {
  transcribe: boolean;
  transcriptionModel: string;
  transcriptionLanguage: string; // "" = auto-detect
  transcriptTimestamps: boolean;
  transcriptionVocabulary: string; // "" = none
  transcriptionVad: boolean;
}
```

Computeds, after `transcriptTimestamps`:

```ts
const transcriptionVocabulary = computed({
  get: () => props.modelValue.transcriptionVocabulary,
  set: (v: string) => patch({ transcriptionVocabulary: v }),
});
const transcriptionVad = computed({
  get: () => props.modelValue.transcriptionVad,
  set: (v: boolean) => patch({ transcriptionVad: v }),
});
```

Models:

```ts
const MODELS = ["base", "small", "medium", "turbo"] as const;
```

Template — inside the `v-if="transcribe"` block, after the Language section
and before the Timestamps section:

```html
    <section>
      <label
        :for="scopedId('capture-transcription-vocabulary')"
        class="mb-1 block text-sm text-slate-200"
      >
        Custom vocabulary
        <span class="block text-xs text-slate-500">Names, acronyms, project terms — primes the model</span>
      </label>
      <textarea
        :id="scopedId('capture-transcription-vocabulary')"
        v-model="transcriptionVocabulary"
        data-testid="transcription-vocabulary-input"
        rows="2"
        placeholder="Anna Kowalska, Kubernetes, Vault Buddy…"
        class="w-full resize-none rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      />
    </section>
    <section class="flex items-center justify-between">
      <label
        :for="scopedId('capture-transcription-vad-toggle')"
        class="text-sm text-slate-200"
      >
        Skip silence
        <span class="block text-xs text-slate-500">Faster meetings, fewer phantom phrases in silent stretches</span>
      </label>
      <input
        :id="scopedId('capture-transcription-vad-toggle')"
        v-model="transcriptionVad"
        data-testid="transcription-vad-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
      >
    </section>
```

`RecordingSettings.vue` — the `transcriptionBundle` computed gains both fields
in `get` and its `set` type:

```ts
const transcriptionBundle = computed({
  get: () => ({
    transcribe: props.modelValue.transcribe,
    transcriptionModel: props.modelValue.transcriptionModel,
    transcriptionLanguage: props.modelValue.transcriptionLanguage,
    transcriptTimestamps: props.modelValue.transcriptTimestamps,
    transcriptionVocabulary: props.modelValue.transcriptionVocabulary,
    transcriptionVad: props.modelValue.transcriptionVad,
  }),
  set: (v: {
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
    transcriptionVocabulary: string;
    transcriptionVad: boolean;
  }) => patch(v),
});
```

`RecordMode.vue` — the `transcription` computed:

```ts
const transcription = computed({
  get: () => ({
    transcribe: config.value.transcribe,
    transcriptionModel: config.value.transcriptionModel,
    transcriptionLanguage: config.value.transcriptionLanguage ?? "",
    transcriptTimestamps: config.value.transcriptTimestamps,
    transcriptionVocabulary: config.value.transcriptionVocabulary ?? "",
    transcriptionVad: config.value.transcriptionVad,
  }),
  set: (v: {
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
    transcriptionVocabulary: string;
    transcriptionVad: boolean;
  }) => {
    config.value = {
      ...config.value,
      transcribe: v.transcribe,
      transcriptionModel: v.transcriptionModel,
      transcriptionLanguage: v.transcriptionLanguage.trim() || null,
      transcriptTimestamps: v.transcriptTimestamps,
      transcriptionVocabulary: v.transcriptionVocabulary.trim() || null,
      transcriptionVad: v.transcriptionVad,
    };
```

(keep the rest of the setter body — the `loaded` gate + `persist()` call —
unchanged).

- [ ] **Step 4: Run the suite + typecheck**

Run: `npx vitest run && npm run build`
Expected: PASS / clean. If `tests/record-mode.test.ts` asserts the emitted
`set_capture_config` payload shape, extend its expected object with
`transcriptionVocabulary: null, transcriptionVad: true` (the defaults flowing
through).

- [ ] **Step 5: Commit**

```bash
git add src/components/TranscriptionSettings.vue src/components/RecordingSettings.vue src/components/RecordMode.vue tests/transcription-settings.test.ts tests/record-mode.test.ts
git commit -m "feat(ui): custom vocabulary, Skip silence toggle, and Turbo model option

Vocabulary primes whisper's initial prompt (textarea, debounced upstream);
Skip silence surfaces the per-vault VAD default-on toggle; Turbo joins the
model dropdown. Same controlled-component idiom, ids idPrefix-scoped."
```

---

### Task 12: Documentation

**Files:**
- Modify: `AGENTS.md` (transcription domain section; the `Where state lives on disk` models row)
- Modify: `docs/DEVELOPMENT.md` (capture config reference)
- Modify: `docs/Gaps.md` (GAP-14 entry)

**Interfaces:** none (prose).

- [ ] **Step 1: AGENTS.md**

In the transcription domain section
(`## The transcription & recordings domains`), after the sentence about tier +
language coming from the vault config, add:

```markdown
Three per-vault knobs joined in the accuracy & speed increment
(spec: `docs/superpowers/specs/2026-07-16-transcription-accuracy-and-speed-design.md`):
`transcriptionVocabulary` + the recording's title compose whisper's
`initial_prompt` (title first, vocabulary LAST — whisper truncates over-long
prompts from the front and the user's explicit vocabulary must survive; the
prompt is never written into the transcript); the `turbo` model tier
(`ggml-large-v3-turbo-q5_0`, ~574 MB, pinned SHA like the others); and
`transcriptionVad` (default ON) — Silero VAD silence skipping via a separate
pinned ~1 MB `ggml-silero-v5.1.2.bin` in the same models dir. The VAD model
downloads on first VAD-enabled job (progress on `capture:modelDownload`,
`model:"vad"`); a FAILED download degrades that job to a no-VAD run with a
warning (never a job failure — the stats footer's
`Silence skipping (VAD)` row reports the EFFECTIVE state), while a cancel
during it is still a cancel.
```

In `### Where state lives on disk`, change the Whisper models row to:

```markdown
| Whisper models | `%APPDATA%\vault-buddy\models\ggml-<tier>.bin` + `ggml-silero-v5.1.2.bin` (pinned Hugging Face URLs + SHA-256) |
```

- [ ] **Step 2: docs/DEVELOPMENT.md**

Find the capture config reference (search `transcriptionLanguage`) and add the
two keys beside their siblings, matching the surrounding table/list format:

```markdown
- `transcriptionVocabulary` (string, optional) — free-text names/acronyms/jargon
  primed into whisper's initial prompt together with the recording's title;
  absent = no priming.
- `transcriptionVad` (bool, default `true`) — skip silence via Silero VAD
  (`models\ggml-silero-v5.1.2.bin`, downloaded on first use). If the model
  can't be fetched the job still transcribes without VAD.
```

and extend the documented `transcriptionModel` values with `"turbo"`.

- [ ] **Step 3: docs/Gaps.md — extend GAP-14**

In the GAP-14 entry (`Cached whisper models are trusted without
re-verification; torn finalize is permanent`), append to its prose:

```markdown
Also covers the Silero VAD artifact (`ggml-silero-v5.1.2.bin`,
`transcribe/src/model.rs::download_vad_model`) since the accuracy & speed
increment: verified at download, trusted from disk thereafter — same class,
same accepted posture. A corrupt cached VAD file surfaces as an inference
failure (`failed` sidecar), not a silent wrong transcript.
```

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md docs/DEVELOPMENT.md docs/Gaps.md
git commit -m "docs: transcription vocabulary/turbo/VAD — agent guide, config reference, GAP-14 scope"
```

---

### Task 13: Full verification gates

**Files:** none (verification only; baseline files only if a gate demands a ratchet update — see Step 3).

- [ ] **Step 1: Rust gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test
cd src-tauri/transcribe && cargo clippy --all-targets -- -D warnings && cargo test && cargo test --features whisper
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri && cargo test -p vault-buddy --lib
cd src-tauri && cargo machete .
```

Expected: all clean/PASS. (`--features whisper` compiles whisper.cpp — needs
cmake/g++, present in this container. The workspace clippy + shell tests need
`npm run setup:linux` + a built `dist/` from Task 8.)

- [ ] **Step 2: Frontend gates, in CI's order**

```bash
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
npm run build
```

Expected: all PASS. `check:quality` must run with NO `coverage/` dir present —
if a previous `test:coverage` left one, `rm -rf coverage` first, then run the
sequence in the order above.

- [ ] **Step 3: Gate contingencies (only if a gate fails)**

- LOC guard failure (a file grew past its baseline): prefer extracting/shrinking;
  if the growth is legitimate and modest, run `npm run check:loc -- --update`
  and commit the changed `scripts/loc-baseline.json` in the same PR with a
  one-line justification in the PR body.
- Coverage floor failure: add the missing test rather than lowering any floor.
- Quality ratchet: same policy — `npm run check:quality -- --update` only when
  the metric legitimately moved, committed with justification.

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin claude/whisper-transcription-features-0njd72
```

Then create a ready-for-review PR titled
`feat: transcription accuracy & speed — vocabulary priming, Turbo tier, Silero VAD`
whose body summarizes the three verticals, links the spec
(`docs/superpowers/specs/2026-07-16-transcription-accuracy-and-speed-design.md`),
and lists the verification gates that ran. Subscribe to PR activity after
creating it.
