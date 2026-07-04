# Increment 3 Design — "Buddy transcribes your recording"

- **Date:** 2026-07-04
- **Status:** Approved
- **Source:** Next Knowledge Intake slice, building directly on
  [Increment 2](2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md)
  (meeting/voice recording → MP3). Delivers the PRD's transcription
  requirement — **locally, with no cloud and no API**.

## Goal

Turn each recording into text **on-device**. When a capture finalizes, a
background worker transcribes the MP3 with a local Whisper model and drops
the result beside the recording as an embedded transcript, so opening the
meeting note in Obsidian shows the audio player and the transcript inline,
in one view.

Hard constraint: **no network at transcription time** — no cloud service, no
API key, no telemetry. The single unavoidable download is the speech model
itself (one time, on first enable), fetched from Hugging Face and cached
app-side. Everything after that runs offline on the user's CPU.

This extends the increment-2 write path rather than inventing a new one: the
transcript is a vault file, so it inherits the same non-negotiable rules —
atomic writes, collision-safe naming, never clobber a user's file, recovery
that touches only our own artifacts, full audit logging.

## Scope

### In scope

1. **Auto-transcribe on stop** — transcription is a **batch** step that runs
   after a recording finalizes, on a background worker, fully off the event
   loop. It never delays the increment-2 `stop → saved < 5 s` guarantee; the
   save fires first and transcription proceeds independently.
2. **In-process engine** — a new workspace crate `vault_buddy_transcribe`
   wraps **whisper.cpp** via `whisper-rs`, **statically linked** (no runtime
   DLL to ship). The engine sits behind a `Transcriber` trait so the
   orchestration around it is testable without a real model.
3. **Local Whisper model, downloaded on first enable** — the multilingual
   **`small`** model (≈466 MB) is the default, fetched on demand from
   Hugging Face `ggerganov/whisper.cpp` (ggml `.bin`) into
   `%APPDATA%\vault-buddy\models\`. Models are **not bundled** (the installer
   stays lean). Tier is per-vault configurable (`base` / `small` / `medium`).
4. **Embedded transcript sidecar** — the transcript is its own vault file,
   `<base>.transcript.md`, whose name is **reserved pairwise** alongside the
   `.mp3`/`.md`/`.mp3.part` so collision suffixing stays in lockstep. The
   meeting note embeds it (`![[…transcript]]`) under a `## Transcript`
   heading, so Obsidian renders everything inline. The note is still written
   **exactly once at finalize** and **never reopened** — the transcript
   arrives as a separate file, so there is no read-modify-write race against
   a user editing the note.
5. **Placeholder → real content** — because transcription outlives finalize,
   the sidecar is created **at finalize** with a `*Transcribing…*`
   placeholder (carrying an owned marker), so the embed never renders
   "file not found." When transcription completes, the worker **atomically
   replaces** the placeholder with the real transcript; on failure it
   replaces it with a retryable error note.
6. **Per-vault config** — new fields on `VaultCaptureConfig`
   (`%APPDATA%\vault-buddy\config.json`), parsed with the same per-field
   defensive discipline (one bad value defaults only itself, never flipping
   another field):
   - `transcribe: bool` — **default `false`** (opt-in).
   - `transcriptionModel: string` — `"base"` | `"small"` | `"medium"`,
     **default `"small"`**.
   - `transcriptionLanguage: string | null` — **default `null`**
     (auto-detect; fine for mixed European), or a pinned code like `"es"`.
   - `transcriptTimestamps: bool` — **default `true`**.
7. **Background transcription worker** — bounded and resilient, modeled on
   the existing `run_recovery` worker: runs on its own thread, **postpones
   while a recording is active** (inference must never steal CPU from live
   capture), retries while work is pending, and **re-scans on startup** so a
   quit mid-transcription resumes. **Recovered** recordings are transcribed
   too, for parity with the live path.
8. **Events / commands / minimal UI** — new events
   `capture:transcribing`, `capture:transcribed`, `capture:transcribeFailed`;
   a model-download progress surface (the one piece that genuinely needs UI);
   a retry affordance. Enabling transcription itself stays config-file driven
   (a settings UI remains deferred, consistent with increment 2).

### Out of scope (deferred)

- **Live / streaming captions** — batch only. Streaming would need a
  different engine (Vosk/sherpa) and live-caption UI; revisit if desired.
- **Sidecar-server architecture** — every mature app (Vibe→`sona`,
  Hyprnote→`owhisper`, Buzz) runs whisper.cpp as a separate HTTP process for
  crash isolation. Real, but more machinery than a batch MVP in a
  minimalist "tiny buddy" warrants. In-process keeps one process and no
  extra binary; we can graft on a sidecar later if a need appears.
- **GPU acceleration** — CPU-only to start. The documented upgrade is a
  single **Vulkan** build (one binary covers NVIDIA/AMD/Intel on Windows).
- **Speaker diarization** (who spoke when) — future work via `parakeet-rs`
  (NVIDIA Sortformer) or `pyannote-rs`, or whisper's own tinydiarize.
- **LLM summarization**, transcribing arbitrary non-capture files,
  custom-model side-loading, and a full transcription settings UI.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Engine | In-process **whisper.cpp** via `whisper-rs`, static-linked | Self-contained, **no runtime DLL**, lean installer, CPU-friendly, most-precedented for Rust/Tauri; multilingual out of the box. whisper.cpp recently added Parakeet support, so the speed door stays open without changing engines. |
| Trigger | **Auto batch** after finalize | Highest-value, zero extra clicks; drops cleanly into the existing monitor-thread `saved` seam. Batch doesn't need the streaming/reuse machinery a sidecar buys. |
| Transcript location | **Embedded sidecar** `<base>.transcript.md` | Inline reading UX of an in-note transcript **plus** clean writes — the audio note is written once and never reopened, so it can never clobber the user's edits. |
| Default model | Multilingual **`small`** (≈466 MB) | Best accuracy/size/speed balance for mixed European meeting audio on a CPU laptop; markedly better than `base` on accents and proper nouns. Configurable per vault. |
| Enablement | **Opt-in per vault, default off** | Transcription is heavyweight (large download, sustained CPU); users shouldn't be surprised by it. Mirrors how capture config already works. |
| Model distribution | **Download on first enable**, not bundled | Models are 100s of MB; bundling bloats the installer. HF `ggerganov/whisper.cpp` ggml `.bin`, cached in `%APPDATA%`. |
| Audio decode | **`symphonia` + `rubato`** (pure Rust) | Decode our own MP3 → 16 kHz mono f32 PCM. Avoids bundling an ffmpeg binary (which the reference apps ship); keeps the app self-contained. |
| whisper.cpp libraries | **Prebuilt static libs pinned by commit, built in CI** (the `sona` recipe), fallback to source build on the native MSVC runner | Sidesteps the known MSVC + bindgen build failure and long from-source builds; reproducible; no C++ toolchain needed on the app build. |
| Placeholder sidecar | Write `*Transcribing…*` at finalize | Embed always resolves; the marker doubles as the "still needs transcription" signal that makes the startup re-scan trivial. |

## Architecture

Three layers, honoring the repo's "what compiles where" split.

### Rust — new workspace crate `src-tauri/transcribe` (`vault_buddy_transcribe`)

Added to `src-tauri/Cargo.toml` `members`. Portable C++ (whisper.cpp) so it
compiles and tests cross-platform, the same way `capture` does with cpal.

| Module | Responsibility |
| --- | --- |
| `engine.rs` | `Transcriber` trait + a `WhisperTranscriber` impl over `whisper-rs`: load model, run `full()`, collect segments `{start, end, text}`. The trait lets the worker be tested with a fake — no model needed. The real `whisper-rs` binding sits behind a Cargo feature so cheap CI can build the crate without compiling whisper.cpp. |
| `decode.rs` | **Pure:** `symphonia` MP3 → PCM, `rubato` resample to **16 kHz mono f32** (whisper's required input). Unit-testable on synthesized audio; reuses the downmix idea already in `capture::mixer`. |
| `model.rs` | Model registry (`base`/`small`/`medium` → filename, URL, checksum, size) and on-disk path resolution under `%APPDATA%\vault-buddy\models\`. Pure. |

### Pure logic in `vault_buddy_core` — new `transcript.rs` module

Beside `capture_note.rs`, mirroring its style exactly:

- **Render** segments → transcript markdown (frontmatter + `[HH:MM:SS]`
  timestamped body), all strings through `yaml_quote` for injection safety.
- **Derive** the sidecar name from the recording `base` via the shared
  `candidate()` suffix scheme (`capture_paths.rs:65-71`).
- **Write** via an atomic, collision-safe writer that mirrors
  `write_note_atomic` (`capture_note.rs:89-139`): dot-prefixed owned temp →
  `flush`+`fsync` → `rename_noreplace`.
- **Placeholder / marker** helpers: render the placeholder, and detect
  whether an existing sidecar is still our placeholder (so the worker only
  ever replaces content it owns, never a user-edited transcript).

Fully unit-testable on Linux — no Tauri, no audio, no model.

### Tauri shell (`src-tauri/src/`, Windows-only gate)

Thin wiring only:

- **Kick-off seam:** in the monitor thread, right after `emit_saved`
  (`capture_commands.rs:321`) — already off the event loop and holding the
  finalized `Outcome` (`session.rs:43-49`, gives `mp3`, `note`,
  `duration_secs`). Enqueue the recording if the vault has `transcribe` on.
- **Transcription worker:** a bounded background thread modeled on
  `run_recovery` (`capture_commands.rs:420-513`) — postpones while a
  recording is active, retries while work is pending, and on startup scans
  the same `YYYY/MM` layout for capture MP3s whose sidecar is missing or
  still a placeholder (crash-resilience for free; also covers recovered
  captures).
- **Model download:** when a job runs and the model file is absent, download
  it first (progress surfaced, checksum-verified, `.part`-then-rename),
  reusing the updater's download/progress patterns, then transcribe.
- **Commands:** `transcribe_recording(path)` (retry / on-demand),
  `transcription_status()`; **events:** `capture:transcribing {mp3}`,
  `capture:transcribed {mp3, transcript}`, `capture:transcribeFailed {mp3,
  message}`, plus model-download progress events.
- **Managed state** for the worker queue and model-download state.

### Vue (`src/`)

- The `capture` store gains a `transcribing` signal and the transcript path
  on `saved`; a light indicator on the just-saved capture, model-download
  progress, and a retry button on failure. Deliberately minimal — enabling
  transcription is a config-file edit, as in increment 2.

## Data flow (happy path)

1. **Recording finalizes** (increment-2 path, unchanged). If the vault has
   `transcribe = true`, a **placeholder sidecar** `<base>.transcript.md` is
   created at finalize with the same atomic/exclusive-create writer as the
   note:

   ```markdown
   ---
   transcript-of: "2026-07-04 1405 Meeting.mp3"
   status: "transcribing"
   created-by: Vault Buddy
   ---

   *Transcribing…*
   ```

   The transcript name was **reserved pairwise** at start (extend
   `reserve_names`, `capture_paths.rs:73-89`), so it never collides.
   Independently, if `createNote = true`, the companion note written at
   finalize now includes, after the `![[…mp3]]` embed:

   ```markdown
   ## Transcript

   ![[2026-07-04 1405 Meeting.transcript]]
   ```

   (`transcribe` and `createNote` are independent: with the note off, the
   transcript sidecar is still produced — it just stands alone beside the
   MP3 with nothing embedding it. Implementation note: verify Obsidian
   resolves the dotted embed `![[<base>.transcript]]` to
   `<base>.transcript.md`; fall back to a `<base> transcript.md` /
   `![[<base> transcript]]` form if the dotted name is treated as an
   extension.)
2. **Monitor thread** emits `capture:saved` (as today), then enqueues the
   MP3 to the transcription worker and emits `capture:transcribing`.
3. **Worker** waits until no recording is active, ensures the model is
   present (download-on-demand if not), then: `decode.rs` → 16 kHz mono f32 →
   `engine.rs` `full()` → segments.
4. **Write** the real transcript: render segments → markdown, then
   **atomically replace** the placeholder sidecar (only if it still bears our
   marker — otherwise the user has taken it over and we leave it alone,
   logging the skip). Emit `capture:transcribed`.

   ```markdown
   ---
   transcript-of: "2026-07-04 1405 Meeting.mp3"
   model: "whisper-small"
   language: "es"
   duration: "1:02:03"
   generated: "2026-07-04T15:10:00+02:00"
   created-by: Vault Buddy
   ---

   [00:00:00] First segment of speech…
   [00:00:12] Next segment…
   ```

   Obsidian hides an embedded note's frontmatter, so the inline view under
   `## Transcript` is just the timestamped text.
5. **Restart resilience:** if the app quits between steps 1 and 4, the
   startup scan finds the placeholder (or missing) sidecar and re-runs from
   step 3. Recovered recordings (increment-2 crash recovery) enter the same
   queue.

## Model management

- **Location:** `%APPDATA%\vault-buddy\models\ggml-<tier>.bin` (app-side,
  never inside a vault — a model must not sync with vault contents).
- **Source:** `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-<tier>.bin`.
- **Download:** streamed to a `.part`, checksum-verified, atomically renamed;
  progress surfaced via events (reuse the updater's UX). One model at a time;
  a second job waits.
- **On-demand:** the first transcription job for a tier triggers the download
  transparently, so no settings UI is required to obtain a model. Failure to
  download surfaces as a transcription failure (retryable), audio untouched.

## Performance targets

- **`stop → saved` unchanged** (< 5 s): transcription is fully async and off
  the critical path; the save event fires before transcription starts.
- **Transcription throughput:** `small` runs roughly around real-time on a
  modern multi-core CPU; a 1-hour meeting completes in minutes in the
  background. `medium` is materially slower (documented). The worker yields
  to active recordings, so live capture is never degraded.
- **Footprint:** static-linked whisper.cpp adds single-digit MB to the
  binary and **no runtime DLL**; models live app-side and are downloaded, not
  bundled. One extra background thread, active only when work is pending.

## Error handling

Guiding rule: **transcription is best-effort and must never jeopardize the
saved audio or note.** Every failure degrades to *"audio saved, transcript
missing (retryable)."*

- **Model download fails** (offline, 404, checksum mismatch) → placeholder
  replaced with a retryable error note; `capture:transcribeFailed` toast;
  audio + note intact.
- **Decode / inference fails** → same degradation; the MP3 is never touched.
- **Placeholder was edited by the user** → detected via the missing marker;
  the worker **does not overwrite**, logs the skip. Never clobber user
  content.
- **Crash mid-transcription** → the placeholder (or absent) sidecar is picked
  up by the startup scan and retried; bounded retry count prevents infinite
  loops on a permanently bad file.
- **All writes** honor the increment-2 contract: exclusive-create owned
  temps, `rename_noreplace` (never `std::fs::rename`), owned markers so
  recovery only ever deletes our temps, writes stay inside the validated
  vault root, the audio note is never reopened, and every start / finish /
  failure / skip is `log::info!`-logged (auditability — no hidden
  processing).
- **Config parity:** `transcribe` gates both the live path and the recovered
  path, exactly as `createNote` already gates the note in both.

## Testing

Same split as increments 1–2: pure logic in CI, native behavior verified on
Windows.

- **`vault_buddy_core` unit tests (CI, Linux):** transcript rendering
  (timestamps, frontmatter, empty/one/many segments), `yaml_quote` injection
  safety, sidecar name derivation + pairwise collision suffixing,
  placeholder rendering, marker round-trip (`is our placeholder` true/false),
  atomic-replace-only-if-owned.
- **`vault_buddy_transcribe` unit tests (CI, Linux):** `decode.rs` MP3 → 16
  kHz mono f32 on synthesized audio (rate, channel, length assertions);
  worker orchestration driven by a **`Transcriber` fake** (success, failure,
  slow); model registry path/URL/checksum resolution. The real `whisper-rs`
  build sits behind a Cargo feature; a genuine end-to-end run against a
  downloaded `tiny` model is an **opt-in, non-default** test (keeps CI fast
  and hermetic).
- **Vitest:** capture store gains `transcribing`; transcript-pending
  indicator, model-download progress, and retry-on-failure rendering.
- **CI update:** extend the `rust-core` job to build and test the new
  `transcribe` crate (feature-gated so whisper.cpp isn't compiled on the
  cheap Linux job). The **`windows-app` job remains the full compile gate**
  for the static-linked whisper.cpp build — this is where the known
  MSVC/bindgen risk is validated; if the from-source build proves flaky,
  switch to the pinned prebuilt-static-libs fetch (the `sona` recipe).
- **Manual Windows checklist** (verification doc, like prior increments):
  enable `transcribe`, record a short Spanish/English clip, confirm the model
  downloads once with progress, the transcript sidecar appears and renders
  inline under `## Transcript`, timestamps line up with the audio, a
  mid-transcription quit resumes on relaunch, and a forced failure shows the
  retryable error without harming the audio.

## Known limitations (accepted for this increment)

1. **CPU-only.** No GPU acceleration yet; `medium` on long meetings is slow.
   Vulkan is the planned upgrade.
2. **No speaker labels.** Overlapping speakers in a meeting are transcribed
   as one stream; diarization is deferred.
3. **Accuracy varies** with audio quality, accents, and jargon; `small`
   is a balance, not the ceiling (`medium` or a future GPU + larger model
   improves it).
4. **One model download required** — the only non-offline moment, one time,
   from Hugging Face. After that, fully local.
5. **Loopback captures all desktop audio** (inherited from increment 2), so
   non-meeting audio during a recording is transcribed too.

## Success criteria

Increment 3 is done when, on a Windows machine with `transcribe` enabled for
a vault:

1. Recording still saves within 5 s of Stop (unchanged); a
   `<base>.transcript.md` appears shortly after and renders **inline** in the
   meeting note under `## Transcript`.
2. The first transcription downloads the `small` model once, with visible
   progress; later recordings reuse the cached model with **no network
   access**.
3. The transcript is a reasonable, timestamped rendering of European-language
   speech.
4. Quitting mid-transcription resumes on the next launch; crash-recovered
   recordings are transcribed too.
5. A transcription or download failure leaves the audio and note intact,
   shows a retryable error in place of the transcript, and can be retried.
6. No user file is ever clobbered; all transcript writes are atomic and
   collision-safe; every transcription action is in the app log.
7. `vault_buddy_core` and `vault_buddy_transcribe` unit tests and the Vitest
   suite pass in CI (Linux), and the static-linked whisper.cpp build passes
   in the `windows-app` job.
