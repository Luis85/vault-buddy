# Transcription Housekeeping Design — detected language + model management

- **Date:** 2026-07-16
- **Status:** Approved (Increment A of the whisper-integration program; user
  directed it onto PR #61)
- **Source:** After the accuracy & speed and GPU increments, two small,
  independent, user-visible gaps remain from the program decomposition:
  auto-detect vaults never learn what whisper detected, and the models dir
  (now up to ~2.7 GB across four tiers + silero) has no in-app visibility
  or remedy. The program's other items: sidecar inference (Increment B),
  chunked long recordings (Increment C, built on B), and the whisper-rs
  upgrade — **re-scoped to a tracked trigger**: 0.16.0 is the newest
  published release (verified on crates.io), a git pin would violate
  `deny.toml`'s `unknown-git = "deny"` supply-chain gate, and a fork is a
  standing maintenance burden that doesn't fit this app. Upgrade when 0.17
  ships; the hand-wired trampoline regression tests are the acceptance
  gate (AGENTS.md note lands with this increment's docs).

## Goal

Auto-detect vaults see what whisper actually detected, honestly labeled;
users see and manage the multi-gigabyte model cache in-app — including the
delete-to-redownload remedy that makes GAP-14's accepted trust posture
user-reachable. No engine-behavior change for pinned-language vaults; no
new vault-write capability.

## Scope

### In scope

1. **Detected-language reporting.** When (and only when) the vault's
   transcription language is auto, the engine reports whisper's detected
   language; the transcript's stats row reads
   `| Language | auto (detected: de) |` and the sidecar frontmatter gains
   `detected-language: "de"` (Dataview-queryable). Pinned-language vaults,
   degraded runs, and the all-silence short-circuit report nothing.
2. **`EngineOutput` struct.** The `Transcriber` trait's return
   `(Vec<Segment>, bool)` becomes a named struct — third signature change
   on this branch; widening tuples stops here:

   ```rust
   pub struct EngineOutput {
       pub segments: Vec<Segment>,
       /// Whether this run actually filtered non-speech via VAD
       /// (the EFFECTIVE state — unchanged semantics).
       pub vad_engaged: bool,
       /// Whisper's detected language code (e.g. "de"), only when the
       /// job ran on auto AND inference actually ran. None on a pinned
       /// language, a degraded run, or the all-silence short-circuit.
       pub detected_language: Option<String>,
   }
   ```

3. **Model management.** Two IPC commands and a Buddy-settings card:
   - `list_transcription_models` (sync): every registry artifact — the
     four speech tiers plus the silero VAD model — with `present`, the
     real on-disk `sizeBytes` when downloaded, and an approximate
     download size when not.
   - `delete_transcription_model` (**async**): guarded delete of a cached
     model file so the next VAD-enabled/tier-matching job re-downloads
     (SHA-verified) — the user-facing GAP-14 remedy.
   - UI: a **Transcription models** card beside the GPU card — one row
     per artifact, size or "not downloaded", Delete with an in-panel
     confirm (re-downloading Medium costs ~1.5 GB), per-row busy/error.

### Out of scope

- Sidecar process, chunked inference (Increments B/C), the whisper-rs
  upgrade (tracked trigger, above).
- An "active tier" badge on the models card (YAGNI — the delete guard
  handles the only real interaction).
- Custom/side-loaded models; changing which tiers exist.
- Re-verifying cached model hashes in place (delete-to-redownload is the
  remedy; GAP-14's residual stands).

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Detection surfaced | Stats row `auto (detected: xx)` + `detected-language` frontmatter, only when detected | Honest labeling (whisper's first-window classification, not a guarantee); queryable in Obsidian; the `language:` field keeps recording the SETTING, wire-stable |
| Detection scope | Auto-language jobs with real inference only | A pinned language makes detection meaningless; all-silence/degraded runs never ran whisper |
| Engine return | `EngineOutput` struct | Three tuple-widenings on one branch is the signal; future fields stop rippling through every implementor |
| Delete vs the worker's mmap | Purge-request + bounded retry (see below) | Windows can't unlink a file the idle worker's cached `WhisperContext` still maps; "in use" while idle is baffling UX; delete-at-next-launch is invisible |
| Delete command | Async | The bounded retry sleeps; sync commands must never block the main thread |
| Confirm on delete | Yes, in-panel | Re-download costs up to ~1.5 GB; reuses the re-transcribe confirm idiom |
| whisper-rs upgrade | Tracked trigger, not scheduled | No published target past 0.16.0; git pin violates deny.toml; fork is a misfit burden |

## Design

### Detected language (engine → sidecar)

- **Engine** (`transcribe/src/engine.rs`): after a successful
  `state.full()`, when `opts.language.is_none()`, read
  `state.full_lang_id_from_state()` and map via
  `whisper_rs::get_lang_str(id)` (verified: crate-level fn, "2 → de");
  an error or unknown id degrades to `None` (never a job failure). The
  all-silence short-circuit and the `vad_model: None` path are untouched
  except for returning the struct.
- **Pipeline** (`transcribe/src/lib.rs`): `EngineOutput` replaces the
  tuple; `transcribe_recording` threads
  `detected_language` into `TranscriptMeta` (new
  `detected_language: Option<String>` field). Test fakes and
  `examples/transcribe_file.rs` adjust.
- **Rendering** (`core/src/transcript.rs`): frontmatter emits
  `detected-language: "de"` (yaml-quoted) only when present; the stats
  Language row becomes `auto (detected: de)` when present, unchanged
  otherwise. A pinned-language transcript is byte-identical to today.

### Model management (backend)

- **Registry surface** (`transcribe/src/model.rs`): a
  `ModelArtifact { id, file_name, approx_download_bytes }` enumeration
  covering the four tiers + the VAD model (ids: the tier keys plus
  `"vad"`), plus a listing helper that stats the models dir — pure
  enough to test against a temp dir. `.part` files are ignored (a
  half-download is the janitor's business, not this card's).
- **Purge request** (`src-tauri/src/transcription.rs`): a shared slot
  beside the queue state (same mutex/cv discipline): the delete command
  posts `PurgeCachedModel(tier-or-vad)` and notifies; the worker loop
  handles it at the top of its wait/claim cycle by dropping its cached
  `(tier, use_gpu, transcriber)` when the tier matches (the VAD model is
  not cached by the worker today — its purge entry is a no-op accepted
  for symmetry). Queue-logic only; unit-testable like the existing
  enqueue/cancel methods.
- **Commands** (beside the existing config commands, LOC permitting):
  - `list_transcription_models` (sync): registry × fs metadata →
    `[{ id, fileName, present, sizeBytes?, approxDownloadBytes }]`.
  - `delete_transcription_model(id)` (async, `spawn_blocking`): refuse
    while ANY transcription job is active (`"A transcription is
    running — try again when it finishes."`); otherwise post the purge
    request, notify the worker, then attempt `remove_file` with a short
    bounded retry (~2 s total, stepped) to ride out the worker's drop;
    a still-locked file returns an honest error naming the restart
    fallback. An already-absent file is success (the remove_model
    "path is clear" contract).
- **Never blocks transcription:** a deleted model re-downloads on the
  next job via the existing `ensure_model`/`ensure_vad_model` flow —
  this card is the user-visible face of that self-heal.

### Model management (UI)

`TranscriptionModelsCard.vue` (self-contained, the settings-card idiom),
mounted in Buddy settings → Integrations beside the GPU card: loads the
list on mount; rows show the artifact name (Base / Small / Medium /
Turbo / VAD silence filter), the real size (`formatBytes`) or
"not downloaded (~574 MB)"; Delete appears on present rows, opens an
in-panel confirm naming the re-download cost, disables the row while the
async delete runs, re-lists on success, and surfaces the command's error
inline on failure. No new store; no new events.

### Error handling

- Detection failure → `None` (warn-level log only) — reporting is
  best-effort garnish, never a job outcome.
- Delete refusals are typed messages the card renders verbatim
  (active-job, still-locked); a locked-file error leaves everything
  consistent (the file only vanishes when the unlink succeeds).
- The purge slot follows the queue's existing poison-recovery discipline.

## Testing

- **core:** render tests — frontmatter + stats row with/without
  `detected_language`; pinned-language byte-identity.
- **transcribe:** `EngineOutput` threading (fake returns a detection →
  meta carries it; pinned language → fake asserts `opts.language`
  present and pipeline records nothing); registry listing against a
  temp dir (present/absent/sizes/.part-ignored); id↔artifact mapping.
- **shell:** purge-request queue handling (posted → worker drop path
  taken; tier mismatch → cache retained); delete-command guards
  (active-job refusal) at the queue-logic level.
- **engine (`--features whisper`):** compile gate; the `#[ignore]`
  real-model test asserts a `Some` detected code on an auto-language run
  with real speech (env-gated, manual).
- **frontend:** card tests — list render (present + absent rows),
  confirm flow (cancel = no IPC call; confirm = delete + re-list),
  error revert, busy serialization.

## Documentation updates

- AGENTS.md: IPC table (+2 commands, delete marked *(async)*),
  transcription-domain paragraph (detected-language semantics; the
  models card + purge/delete guard), and the whisper-rs upgrade
  tracked-trigger note (0.16.0 is current; upgrade when 0.17 ships; the
  trampoline regression tests are the acceptance gate).
- docs/DEVELOPMENT.md: models-on-disk section gains the card's existence
  and the delete-to-redownload remedy.
- docs/Gaps.md: GAP-14 gains one line — the models card is the
  user-facing remedy for a suspect cached model.

## Increment B preview (so this spec's seams point the right way)

The sidecar increment will move decode + VAD + inference into a separate
process; this increment deliberately keeps its additions on the seams
that survive that move — `EngineOutput` is the natural wire payload
shape, and the purge request stays queue-side (the sidecar's model cache
will be process-lifetime, purged by process recycle).
