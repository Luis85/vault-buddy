# Transcription Accuracy & Speed Design — "More of what whisper offers"

- **Date:** 2026-07-16
- **Status:** Approved
- **Source:** Follow-up to
  [Increment 3](2026-07-04-increment-3-local-speech-to-text-design.md)
  (local speech-to-text) and its polish increments. Increment 3 shipped the
  most conservative possible whisper configuration: greedy decoding, one
  full-file pass, three model tiers, a language pin, segment timestamps.
  The pinned `whisper-rs` 0.16 exposes substantially more. This increment
  adopts the three features with the best value-per-risk for meeting
  capture — vocabulary priming, a modern model tier, and built-in silence
  skipping — chosen from a brainstorm that weighed accuracy, speed, richer
  output, and live feedback (the user picked **accuracy + speed**).

## Goal

Better words on the page, sooner, with zero new architecture: prime whisper
with the vault's own vocabulary and the recording's title, add the modern
CPU sweet-spot model, and stop transcribing silence. Every change rides an
existing seam (per-vault config fields, the model registry, the
`TranscribeOptions` plumbing, one settings component). The engine remains
in-process, batch, local-only; the never-clobber transcript contract is
untouched.

## Scope

### In scope

1. **Custom vocabulary → `initial_prompt`.** A per-vault free-text
   *Custom vocabulary* field (names, acronyms, project terms) plus the
   recording's current title (its file-name stem minus the
   `YYYY-MM-DD HHmm ` capture prefix), composed into whisper's
   `initial_prompt` so domain terms transcribe correctly. A recording
   renamed before its job runs ("Budget review with Anna Kowalska") primes
   the model for free.
2. **A Turbo model tier.** `ggml-large-v3-turbo-q5_0` (~574 MB) — smaller
   than Medium (~1.5 GB), more accurate, and faster on CPU. One new
   dropdown entry; the existing three tiers are untouched.
3. **Silero VAD silence skipping.** whisper-rs 0.16's built-in VAD
   (`enable_vad` + silero model), behind a per-vault **Skip silence**
   toggle, **default on**: meetings transcribe noticeably faster and
   whisper stops hallucinating phrases into silent stretches. Needs one
   tiny (~1 MB) extra model download under the same pinned-SHA discipline.

### Out of scope

- **GPU (Vulkan) acceleration** — approved as the **named next
  increment**, not this one. It is build-system + CI work (Vulkan SDK on
  the Windows jobs, a `whisper-vulkan` cargo feature enabled only for
  Windows builds), plus an app-global *Use GPU* escape-hatch toggle and a
  `(tier, gpu)` model-cache key. Kept separate so the CI/build risk ships
  and reverts independently of this increment's pure Rust/config work.
- **Beam search** — considered and rejected for this increment: Turbo +
  VAD + vocabulary already move accuracy and speed; beam adds 1.5–2×
  inference time for incremental gain.
- **Richer output** (speaker turns via tinydiarize, word-level timestamps,
  detected-language reporting, translate-to-English) and **live segment
  streaming** — surveyed, deliberately deferred.
- Anti-hallucination threshold tuning (`no_speech_thold`, entropy/logprob
  thresholds, `suppress_nst`) — VAD removes the dominant hallucination
  source (silence); the thresholds stay at whisper defaults rather than
  silently changing existing vaults' output.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Prompt sources | Per-vault field **+ recording title** | The title is the one piece of per-recording context we already have; a renamed recording carries the meeting's subject and attendee names |
| Prompt order | Title first, **vocabulary last** | whisper truncates a too-long prompt from the *front* (it keeps the last `n_text_ctx/2` tokens), so the user's explicit vocabulary is what must survive |
| Model lineup | Add **Turbo only** (`large-v3-turbo-q5_0`) | Modern sweet spot; quantization also serves the speed goal; dropdown stays at 4 entries; existing tiers' files unchanged |
| VAD posture | Per-vault toggle, **default on** | Speed + fewer phantom phrases out of the box; escape hatch if VAD ever clips soft speech on someone's recordings |
| VAD model download failure | **Degrade to no-VAD**, warn, still transcribe | The user's intent is "transcribe my meeting"; a ~1 MB optional accelerant must not fail the job. Self-heals on a later job |
| Beam search | Skipped | See out-of-scope |
| GPU | Next increment | See out-of-scope |

## Design

### Config & settings surface

Two new per-vault fields in `VaultCaptureConfig`
(`core/src/vault_config.rs`), following the existing field discipline:

- `transcription_vocabulary: Option<String>` — wire key
  `transcriptionVocabulary`. Parsed defensively (non-string → default),
  trimmed, empty-after-trim → `None`; serialized only when present (the
  `transcriptionLanguage` pattern, keeping hand-authored configs minimal).
- `transcription_vad: bool` — wire key `transcriptionVad`, default
  **true**; per-field defensive bool parse like every sibling.

`transcription_model` (existing string) accepts a fourth value, `"turbo"`.
`ModelTier::from_str` maps it; unknown values still default to `small`, so
a config written by a newer app version degrades safely on an older one.

`serialize_vault_entry` round-trips both fields. The settings DTO in
`capture_config_commands.rs` and `src/types.ts` gain the same two fields.
`TranscriptionSettings.vue` (rendered only while *Transcribe recordings*
is on) gains a **Custom vocabulary** textarea ("Names, acronyms, project
terms…" — helper text explains it primes the model), a **Skip silence**
toggle (helper: faster meetings, fewer phantom phrases in silent
stretches), and **Turbo** in the model dropdown. No new IPC commands, no
new events.

### Model registry: Turbo + the silero artifact

- `ModelTier::Turbo`: `as_str` `"turbo"`, label `whisper-turbo`, file
  `ggml-large-v3-turbo-q5_0.bin` from the same pinned Hugging Face repo
  (`ggerganov/whisper.cpp`), SHA-256 pinned at implementation time,
  `min_size` floor 500 MB (file is ~574 MB). Inherits `.part`-then-rename,
  checksum verification, and the corrupt-model self-heal untouched.
- The silero VAD model is deliberately **not** a `ModelTier` (it is not a
  speech model; the enum stays honest). `model.rs` gains `vad_model_path()`
  (same models dir) and `download_vad_model(cancel, on_progress)` reusing
  the existing `download_stream` — file `ggml-silero-v5.1.2.bin` (~1 MB)
  from whisper.cpp's official VAD model source (the `ggml-org/whisper-vad`
  Hugging Face repo), URL + SHA-256 pinned at implementation time, a
  500 KB size floor (the file is ~1 MB), same `.part`/verify/rename
  discipline.

### Prompt composition (pure, Linux-tested)

- `core::capture_paths::capture_title(base) -> &str` strips the
  `YYYY-MM-DD HHmm ` prefix from a capture base name, reusing the prefix
  grammar `is_capture_base`/`rename_plan` already encode — one source of
  truth for the capture-name shape.
- `transcribe::compose_initial_prompt(title, vocabulary) -> Option<String>`
  joins the non-empty parts, **title first, vocabulary last** (see key
  decisions), `None` when both are empty. The composed prompt is capped by
  whisper itself (last-half-context tokens); no client-side cap.

### Engine

The `Transcriber` trait's `language: Option<&str>` parameter widens into a
small borrowed struct so the trait does not grow positional arguments:

```rust
pub struct EngineOptions<'a> {
    pub language: Option<&'a str>,
    pub initial_prompt: Option<&'a str>,
    pub vad_model: Option<&'a Path>, // Some = enable VAD with this silero file
}
```

`WhisperTranscriber::transcribe` sets `set_initial_prompt` when present.
If `set_initial_prompt` shares `set_language`'s bounded few-bytes-per-job
CString leak, it gets the same documented-and-accepted comment.

**VAD mechanism (AMENDED after the whole-branch review).** The first cut
used `FullParams`' `enable_vad`/`set_vad_model_path`/`set_vad_params` —
which turned out to be dead on our call path: whisper-rs routes inference
through `whisper_full_with_state`, and the bundled whisper.cpp implements
VAD filtering only in `whisper_full`/`whisper_full_parallel` (unreachable
from whisper-rs's no-state contexts). The corrected design uses
whisper-rs's standalone VAD API, with the filtering and remapping done in
Rust where it is unit-testable:

- FFI (feature-gated): `WhisperVadContext::new(model, params)` +
  `segments_from_samples(WhisperVadParams::default(), samples)` yields
  speech segments in **centiseconds** (the C side applies threshold /
  min-speech / min-silence / pad / overlap internally).
- Pure logic (`transcribe/src/vad.rs`, Linux-tested): convert centiseconds
  → sample spans (16 kHz: 1 cs = 160 samples) with clamp/merge; build a
  concatenated speech-only buffer plus a span map; remap whisper's
  segment timestamps from filtered time back to the original timeline via
  that map (positions falling in a collapsed gap clamp to the owning
  span's edge).
- The engine runs whisper on the FILTERED buffer, then remaps. All-silence
  short-circuits to zero segments without running whisper (the existing
  "No speech detected" rendering). A VAD context/detect failure degrades
  to an unfiltered run with a warning — never a job failure.
- `Transcriber::transcribe` returns the segments plus a `vad_engaged`
  flag, and `TranscriptMeta.vad` records that flag — the stats row
  reports what actually happened, not the setting (an engine-level
  degrade in a VAD-enabled vault honestly reads `off`).

`TranscribeOptions` (the crate-level options) gains `initial_prompt:
Option<String>` and `vad_model: Option<PathBuf>`; `transcribe_recording`
threads them into the engine call. The test fakes adjust to the new trait
signature.

### Shell orchestration (`transcription.rs`)

In `process_transcription`, after `ensure_model`:

- Compose the prompt from the vault's `transcription_vocabulary` and
  `capture_title` of the job's mp3 file name.
- When the vault's `transcription_vad` is on, ensure the silero model:
  present → use it; absent → download it, progress riding the existing
  `capture:modelDownload` event (`model: "vad"`). A **failed** download
  logs a warning and proceeds with `vad_model: None` (degrade — the job
  still transcribes); a **cancel** during it still cancels the job
  (`emit_cancelled`), exactly like a cancel during the main download.
- Pass both through `TranscribeOptions`. No queue, dedup, cancel, or
  rename-retarget semantics change.

### Transcript output

`TranscriptMeta` gains `vad: bool`; the `## Statistics` footer gains one
row — `| Silence skipping (VAD) | on/off |` — so a transcript is
self-explaining when quiet asides are absent. The `model` frontmatter
value shows `whisper-turbo` automatically via the existing label plumbing.
The composed prompt is deliberately **not** recorded in the transcript:
vocabulary can contain sensitive names, and transcripts get shared.

### Error handling

- VAD download failure → warn + degrade (see key decisions).
- A silero file that corrupts on disk *after* a verified download is the
  same accepted class as GAP-14 (cached models trusted without
  re-verification); the Gaps entry is extended to name the VAD artifact.
- Everything else inherits the existing paths: inference errors → the
  retryable `failed` sidecar via `inference_failure_message`, cancel
  semantics and the never-clobber transcript contract unchanged.

## Testing

TDD throughout, per repo convention; everything below runs on Linux except
the env-gated real-model check.

- **core:** parse/default/serialize round-trips for
  `transcriptionVocabulary` (including trim-to-None) and
  `transcriptionVad` (default-true, malformed-value fallback);
  `capture_title` round-trips against the capture-name grammar (and
  refuses/passes through non-capture names sensibly).
- **transcribe:** `compose_initial_prompt` ordering, emptiness, and
  whitespace handling; Turbo registry shape (file/url/label, 64-char
  lowercase-hex sha, floor, `from_str("turbo")`, unknown → Small);
  VAD artifact shape (path under the models dir, url/sha format).
  `download_stream` behavior is already pinned by its existing suite.
- **engine (`--features whisper`, Linux `rust-core` CI job):** compiles
  with the new setters; the `#[ignore]` real-model test gains env-gated
  prompt/VAD paths (`VB_TEST_VOCAB`, VAD on when the silero model is
  present) asserting a VAD run still yields ordered, original-timeline
  timestamps — a Windows dev runs it with `-- --ignored`.
- **frontend:** `TranscriptionSettings` coverage for the textarea, the
  toggle, and the Turbo option (emit shape + default rendering), in the
  existing Vitest suite.

## Documentation updates

- **AGENTS.md** — transcription domain section (new fields, Turbo tier,
  VAD + degrade posture), the config-key documentation pointer.
- **docs/DEVELOPMENT.md** — capture config reference gains the two keys
  and the `"turbo"` model value; the models-on-disk table gains the silero
  file.
- **docs/Gaps.md** — extend GAP-14 to cover the VAD artifact.
- **CONTEXT.md** — no new terms coined ("vocabulary" and "VAD" are plain
  language); no change expected.

## Follow-up (named next increment)

**GPU (Vulkan) acceleration:** whisper-rs `vulkan` feature behind a
`whisper-vulkan` cargo feature enabled only for Windows builds; Vulkan SDK
in the Windows CI/release jobs; an app-global *Use GPU* toggle (default
on, the escape hatch for buggy drivers) read at model load with the worker
model cache keyed on `(tier, gpu)`; stats row naming the device. Honest
caveat to carry into that design: the engine is in-process, so a GPU
driver fault crashes the app — the sidecar-process architecture remains
the documented future fix if that bites in practice.
