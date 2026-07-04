# Increment 2 Design — "Buddy records your meeting"

- **Date:** 2026-07-04
- **Status:** Approved
- **Source:** First increment cut from [docs/PRD-knowledge-intake.md](../../PRD-knowledge-intake.md) (Knowledge Intake, Version 1 — Audio Recording)

## Goal

Ship the first Knowledge Intake slice: one click on 🎙 Capture records
microphone + desktop audio (the Teams-meeting use case) into a single mixed
stereo MP3 stored inside the chosen vault, with the buddy itself acting as
the always-visible recording indicator.

This is the app's **first write path into a vault**. Increment 1's rule was
"Vault Buddy never writes into a vault"; Knowledge Intake deliberately
changes that. The design compensates with atomic writes, collision-safe
naming, crash recovery, and full audit logging.

## Scope

### In scope

1. **Capture action** — each vault row in the existing action panel gains
   🎙 **Capture Knowledge**. Clicking it starts recording immediately using
   the system default microphone and default output device (WASAPI
   loopback). No dialogs.
2. **Recording state UX** — the buddy gets a `recording` character state
   (pulsing red mic badge); the tray icon switches to a recording variant;
   the panel shows a recording bar with elapsed time and **Stop**. Other
   vaults' Capture buttons are disabled while a recording is active (one
   recording at a time).
3. **Audio pipeline** — cpal microphone stream + WASAPI loopback stream →
   resample/mix to 44.1 kHz stereo with soft-clip limiting → stream-encode
   128 kbps MP3 (LAME) into a hidden `.part` file inside the target folder,
   flushed ~every second, atomically renamed on stop.
4. **Storage layout** — `Meetings/YYYY/MM/YYYY-MM-DD HHmm Meeting.mp3`;
   name collisions get a ` (2)`, ` (3)`, … suffix. The collision check
   treats the recording and its markdown note as a **pair**: a base name
   is only used if both the `.mp3` and the `.md` paths are free, so a
   pre-existing user note is never overwritten. Folders are auto-created.
5. **Companion markdown note** (no AI) — a same-named `.md` beside the MP3
   containing frontmatter metadata (date, duration, vault, recording type,
   devices, created timestamp) and an `![[…]]` embed of the recording.
   Config-toggleable, default on. Delivers the PRD's Metadata requirement
   and makes captures instantly visible in Obsidian. The note is written
   with exclusive-create (never overwrite); if the path is somehow taken
   despite the pairwise collision check, the note gets its own suffix
   rather than replacing existing content.
6. **Per-vault config file** — app-side at
   `%APPDATA%\vault-buddy\config.json`, keyed by vault ID. No config is
   written into user vaults (a vault synced to another machine must not
   carry this machine's device assumptions). Establishes the settings
   schema for later increments: recording mode (`meeting` default;
   `voice-note` = mic-only), recording folder, bitrate, note on/off.
   Hand-editable and documented; a settings UI comes in a later increment.
7. **Notifications** — buddy/panel states cover started/stopped; an OS
   toast (Tauri notification plugin) reports **saved** (with filename) and
   **failed**.
8. **Crash recovery** — on startup, orphaned `.part` files in configured
   recording folders are finalized as `… (recovered).mp3` (plus note and
   toast). Streaming MP3 encoding means partial files are playable.
   Recovery must never touch a live recording: the app enforces **single
   instance** (`tauri-plugin-single-instance` — a second launch focuses
   the running buddy instead of starting a new process), and as a second
   guard the scan only recovers `.part` files whose modification time is
   stale (no writes for ≥60 s; an active session flushes every ~1 s).

### Out of scope (deferred)

Pause/resume, settings UI, device pickers, naming templates, all other
capture providers (screenshot, clipboard, …), AI pipeline (transcription,
summaries), non-Windows capture, multiple simultaneous recordings.

### Deliberate PRD deviation

The PRD's example filename (`2026-07-04 Sprint Planning.mp3`) contains a
human title. One-click capture cannot know a title, so this slice
auto-names with timestamp + type; users rename in Obsidian (its link
updating keeps the note's embed working). Title prompts arrive with naming
templates later.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| First Knowledge Intake slice | Meeting recording (mic + desktop audio) | Hits the flagship Teams use case; front-loads the hardest audio engineering deliberately |
| Track layout | Single mixed stereo MP3 | Matches PRD file layout, simplest to store/embed, what transcription expects |
| Recording indicator | The buddy itself + tray variant | Zero new windows; builds on the existing character-state system; satisfies the visible-indicator security requirement |
| Settings | Defaults + hand-editable config file, no UI | Keeps the slice thin while establishing the schema a future UI reads/writes |
| Capture stack | Rust-native: cpal (WASAPI loopback) + LAME streaming encode | In-process, no bundled binaries, testable pure core; streaming encode gives instant save and playable partials. Webview MediaRecorder breaks one-click for desktop audio; ffmpeg lacks built-in WASAPI loopback |
| Temp-file strategy | Dot-prefixed `.part` file in the target folder | Same-directory rename is atomic on every filesystem/drive; dotfiles are hidden in Obsidian; crash leaves a recoverable file in a known location |
| Config location | App-side, keyed by vault ID | Vaults stay unpolluted; device/machine-specific settings don't sync with vault contents |

## Architecture

### Rust — new workspace crate `src-tauri/capture` (`vault_buddy_capture`)

| Module | Responsibility |
| --- | --- |
| `session.rs` | Recording state machine (`Idle → Recording → Finalizing`); owns the worker thread; exposes `start`/`stop`/`status`. The only stateful piece. |
| `devices.rs` | Open cpal streams: default microphone (input) and default output in WASAPI loopback mode. Loopback is `#[cfg(windows)]`; mic-only elsewhere so the crate compiles and tests on Linux CI. |
| `mixer.rs` | **Pure:** resample both streams to 44.1 kHz stereo, sum with soft-clip limiting, silence-fill on underrun, drop-with-log on overflow (this doubles as basic clock-drift handling). Fully unit-testable. |
| `encoder.rs` | LAME wrapper: PCM in → MP3 frames out, flushed ~every second so the `.part` file on disk is always near-complete. |

### Pure logic in `vault_buddy_core`

Output path/filename generation, collision suffixing, config parsing with
defaults, frontmatter/note rendering. No I/O; all unit-testable — matching
the existing `discovery`/`daily_notes`/`uri` style.

### Tauri layer (`src-tauri/src`)

- Commands: `start_capture(vault_id)`, `stop_capture()`, `capture_status()`.
- Managed state: `CaptureState` wrapping the session.
- Events to the frontend: `capture:started`, `capture:saved {mp3, note}`,
  `capture:failed {message}`, `capture:warning` (e.g. one source died).
- Elapsed time is computed frontend-side from the started timestamp — no
  tick IPC. Tray icon swaps on start/stop.

### Vue (`src/`)

- New Pinia store `capture` — state: `idle` / `recording` / `saving`,
  `activeVaultId`, `startedAt`, last error/warning.
- `CompanionCharacter.vue` gains the `recording` state.
- `ActionPanel.vue` / `VaultList.vue` render the recording bar (elapsed +
  Stop) and disable other Capture buttons while active.

## Data flow (happy path)

1. `start_capture(vault_id)` → load config (defaults if absent) → verify
   the vault path is writable → create `Meetings/YYYY/MM/` → open
   `.YYYY-MM-DD HHmm Meeting.mp3.part` (dot-prefixed → hidden in Obsidian)
   in the target folder.
2. Open the loopback stream, then the mic stream → worker thread pulls both
   ring buffers → mixer → encoder → file, flushed every second.
3. `stop_capture()` → flush encoder → fsync → rename to the final name →
   write the `.md` note → emit `capture:saved` + toast. Streaming encode
   makes stop near-instant regardless of meeting length.

## Performance targets

- Recording startup < 2 s (stream opening is the only real cost).
- Stop → saved < 5 s even for multi-hour recordings (streaming encode; the
  finalize is flush + rename + small note write).
- Recording adds one worker thread; MP3 encoding is far below one core.

## Error handling

Guiding rule: **never lose captured audio.**

- **Start failures fail fast, before any file exists** — vault path
  missing/unwritable, no default microphone, loopback unavailable →
  `capture:failed` with a human-readable message in the panel; buddy stays
  idle. (In `voice-note` mode, absent loopback is fine by definition.)
- **One source dies mid-recording** (headset unplugged, device switch) —
  recording continues with the surviving stream; the mixer already
  silence-fills a starved side. A `capture:warning` shows in the panel and
  the event is recorded in the note metadata.
- **Both sources die** — finalize immediately (flush, rename, write note)
  and report "recording ended early" with the saved file.
- **Disk full / write error mid-recording** — stop streams, attempt to
  finalize what is flushed, surface the error.
- **App crash / power loss** — the `.part` file contains valid MP3 frames
  up to the last flush. The startup recovery scan finalizes orphans as
  `… (recovered).mp3` (+ note + toast). Recovery only ever renames — it
  never deletes — and only acts on `.part` files with a stale mtime
  (≥60 s), backed by single-instance enforcement, so it can never grab a
  recording that another live session is still writing.
- **Concurrency** — `start_capture` during an active session returns a
  typed error; the UI prevents it by disabling other Capture buttons.
- **Auditability** — every start/stop/save/recovery is app-logged with
  vault and path. Combined with the buddy/tray indicator: no hidden
  recordings (PRD security requirement).

## Testing

Same split as increment 1 — pure logic in CI, native behavior verified on
Windows (development environment is Linux).

- **Rust unit tests (CI, no devices):** mixer math — mixing, soft-clip,
  underrun silence-fill, overflow drop; resampler correctness on synthetic
  sines; filename/path generation including pairwise mp3+md collision
  suffixing; recovery staleness decision (fresh vs. stale mtime); config
  parsing with missing/partial/garbage input; frontmatter and note
  rendering.
- **Rust integration test (CI):** synthesized PCM through
  mixer → encoder → file; assert a decodable MP3 with expected duration
  (± tolerance); simulate mid-stream truncation and assert recovery
  produces a playable file.
- **Vitest:** capture store transitions (idle → recording → saving → idle,
  failure paths), recording bar rendering, Capture buttons disabled while
  recording, character `recording` state.
- **Manual Windows checklist** (verification doc like increment 1's): real
  Teams call with both sides audible in the MP3; device unplug
  mid-meeting; kill the app mid-recording and verify recovery; toasts;
  tray/buddy indicators.
- **CI workflow update (required):** the `rust-core` job currently runs
  clippy and `cargo test` only from `src-tauri/core`
  (`.github/workflows/ci.yml`). This increment must extend that job to
  also cover the new `capture` crate (e.g. run clippy/tests per crate or
  workspace-wide from `src-tauri`, excluding the Tauri shell crate if its
  system dependencies are unavailable on the runner) — otherwise the
  capture tests above would exist but never execute in CI. Because cpal's
  Linux backend links against ALSA, the job must also install
  `libasound2-dev` (one apt step) before building the capture crate;
  without it the extended job fails at compile time before any test runs.

## Known limitations (accepted for this increment)

1. **Clock drift** between mic and loopback is handled only by
   buffer-occupancy (silence-fill/drop); adaptive resampling is deferred.
   Worst case: a slowly growing offset of a few hundred milliseconds in
   very long meetings.
2. **Loopback captures all desktop audio**, not just the meeting app —
   inherent to WASAPI loopback; documented for users.
3. **Windows-only capture.** The crates compile and unit-test
   cross-platform, but loopback and desktop verification target Windows.
4. **System default devices only.** Wrong-device situations are resolved in
   Windows sound settings until the device-picker increment.

## Success criteria

Increment 2 is done when, on a Windows machine:

1. Clicking 🎙 Capture on a vault starts recording within 2 seconds; buddy
   and tray visibly enter the recording state.
2. During a Teams call, both the user's voice and other participants are
   audible in the resulting MP3.
3. Stop produces, within 5 seconds, a correctly named MP3 in
   `Meetings/YYYY/MM/` inside the vault plus a markdown note with metadata
   and a working audio embed in Obsidian.
4. Filename collisions and missing folders are handled automatically.
5. Killing the app mid-recording leaves a playable `.part` file that the
   next launch finalizes as `… (recovered).mp3`.
6. Saved and failed outcomes raise OS toasts; every recording action
   appears in the app log.
7. All Rust and Vitest tests pass in CI (Linux), with the CI workflow
   updated so the capture crate's clippy and tests actually run there.
