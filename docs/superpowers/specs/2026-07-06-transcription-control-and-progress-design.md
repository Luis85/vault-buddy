# Increment 4 Design — "See and steer your transcriptions"

- **Date:** 2026-07-06
- **Status:** Approved
- **Source:** Follow-up to
  [Increment 3](2026-07-04-increment-3-local-speech-to-text-design.md)
  (local speech-to-text). Increment 3 shipped auto-transcription, the
  embedded sidecar, per-vault settings, and a per-row re-transcribe in the
  Recordings list. Production use surfaced four gaps: no way to **cancel** a
  running transcription, a **stale re-transcribe** state in the Recordings
  list, transcription settings **buried** in vault settings, and a
  **download row that never clears** when inference starts. A real report
  (below) showed the deeper problem underneath all of them: transcription
  runs with **no visible feedback**, so a normally-slow job is
  indistinguishable from a stuck one.

## The report that anchors this work

A user on a fresh install, with auto-transcribe **off**, recorded a meeting
and then triggered re-transcribe from the Recordings list. The log:

```
09:48:04  transcribe: queued …2026-07-06 0930 Meeting.mp3
09:48:04  transcribe: downloading model small
09:49:09  symphonia … estimating duration from bitrate, may be inaccurate for vbr files
10:15:55  clean shutdown (quit)
```

**What it proves.** The `symphonia` line is the first step *inside*
`transcribe_recording` (decode → 16 kHz PCM), which runs only after the model
has downloaded **and** loaded. So by 09:49:09 the model was ready and the
recording was decoding, then handed to whisper inference. `clean shutdown
(quit)` means the **main thread was responsive** — the app was not frozen.
Only the background transcription worker was busy: whisper inference on the
`small` model, CPU-only, **with no feedback in the UI or the log** for ~27
minutes, at which point the user reasonably gave up and quit.

**What it does not prove.** Whether inference was advancing or genuinely
wedged at 0%. Today nothing can distinguish the two — no UI progress, and
whisper's own logging is suppressed (`set_print_progress(false)`). That
ambiguity *is* the gap this increment closes.

## Goal

Make background transcription **observable and steerable**: the user can see
what is running, how far along it is, whether it failed, and can cancel it —
and the settings that govern it are reachable where a recording is started.
No change to the transcription engine, the write path, or the never-clobber
contract.

## Scope

### In scope

1. **Cancel a running transcription** — from the main-panel summary and from
   the Recordings list. Cancelling aborts in-flight whisper inference (via
   whisper-rs's abort callback) and drops a not-yet-started job from the
   queue. Cancellation is **sticky**: the recording is marked `cancelled`
   and is **not** auto-retried on the next launch, but stays manually
   re-transcribable.
2. **A Transcriptions progress view** — a dedicated, navigable panel view
   listing the live queue (the active job with a real progress bar, queued
   jobs, and a "waiting for recording to finish" state) plus this session's
   finished/failed/cancelled jobs, each with the right action
   (cancel / re-transcribe / open). Reached from a **compact summary
   indicator** on the main panel that replaces today's single-line status.
3. **Real progress + an explicit backend phase model** — the worker owns an
   observable phase per in-flight job
   (`downloading → preparing → transcribing`) and emits every transition,
   including a clean **model-download → inference handover** so the download
   row is replaced the instant the download finishes. Inference exposes a
   **real 0–100% progress** via whisper-rs `set_progress_callback_safe`.
4. **Transcription settings on the Record view** — the transcribe
   on/off + model + language + timestamps controls appear on the
   "Start Recording" view, editing the **vault's** per-vault config (one
   source of truth), extracted into a shared component reused by the vault
   settings screen.
5. **Honest inference logging** — the worker logs inference start (audio
   length, thread count), periodic progress, and completion/elapsed, so the
   log never goes dark mid-inference again.

### Out of scope (deferred)

- **Changing the default model** (`small`). The Record-view settings let a
  user pick `base` for speed; the default stays as decided in increment 3.
- **Persisting the progress view across app restarts.** The Transcriptions
  view reflects the current session (live queue + this session's outcomes);
  prior-session failures remain visible per-vault in the Recordings list.
- **Parallel transcription workers / GPU.** Still one worker, one job at a
  time (inference is CPU-bound; parallelism would starve it).
- **A global "cancel all" button.** Per-job cancel only; a bulk control can
  be added later if the queue routinely grows.
- **Pausing/resuming an in-flight transcription.** whisper has no
  checkpoint; cancel restarts from scratch. Not offered.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| Cancel in-flight inference | whisper-rs `FullParams::set_abort_callback_safe(FnMut() -> bool)` polling an `Arc<AtomicBool>` cancel token | The only way to interrupt the blocking multi-minute `whisper.full()` FFI call. An aborted `full()` returns `Err`, which the worker distinguishes from a real failure via the token. Verified against the pinned whisper-rs 0.16 source. |
| Cancel semantics | **Sticky** — a new `cancelled` sidecar state that the startup scan does **not** re-queue | Matches the verb "cancel." A `failed`/`pending` sidecar is re-scanned on next launch; a deliberately cancelled one should not silently run again. Still force-re-transcribable on demand. |
| Live progress source of truth | A backend `transcription_status()` command + per-transition events; the frontend keeps a per-job **map** keyed by mp3 | The current singular store fields cannot represent "1 running + N queued," and a component-local `ref` is lost on panel remount (the Recordings stale-state bug). One backend-derived model fixes both. |
| Inference progress | `set_progress_callback_safe(FnMut(i32))` (0–100), stored lock-free in an `AtomicU8` on the active job | The callback fires on the whisper FFI thread; an atomic avoids taking the queue mutex per tick. Turns "is it stuck?" into an observable fact. |
| Download → inference handover | A dedicated `capture:modelReady` event fired the instant `ensure_model` returns (downloaded **or** already present) | The download row clearing only on the terminal event is the reported "stuck at 100%" bug; an explicit ready signal replaces it with "Preparing…". |
| Record-view settings persistence | Edit and **save the vault config** (same store as the gear-icon settings) | Consistent with how model/language already work; one source of truth. A per-recording override would be a new concept and inconsistent (model/language would still come from config). |
| Progress view placement | A dedicated **navigable view**, reached from a compact summary indicator | Keeps the 360×340 buddy panel uncluttered while giving room for a real multi-job list; mirrors the existing Recordings view pattern. |

## Architecture

Honors the repo's "what compiles where" split: pure logic in
`vault_buddy_core` (Linux-tested), engine wiring behind the `whisper`
feature (Windows CI gate), thin shell wiring, Vue frontend (Vitest).

### `vault_buddy_core` — new `cancelled` transcript state

`transcript.rs` gains a fourth marker mirroring the existing three:

- `MARKER_CANCELLED = "vault-buddy-transcript: cancelled"`.
- `render_cancelled(mp3_file_name) -> String` — a `[!note]` callout
  ("Transcription cancelled — re-transcribe to run it again"), same
  frontmatter/`yaml_quote` discipline as `render_error`.
- `TranscriptStatus::Cancelled` + `as_dto_str()` → `"cancelled"`;
  `transcript_status()` classifies the marker.
- **`is_regenerable()` deliberately does NOT match `cancelled`.** This is the
  crux of sticky-cancel: `needs_transcription()` (hence
  `pending_transcriptions()`, the startup scan) skips a cancelled sidecar, so
  it is never auto-re-queued. Force re-transcribe still overwrites it via
  `force_write_sidecar` (which ignores the regenerable guard).

All unit-testable on Linux. Existing `pending`/`failed`/`complete` behavior
is untouched.

### `vault_buddy_transcribe` — cancellation + progress plumbing

- **`CancelToken`** — a thin newtype over `Arc<AtomicBool>` with
  `cancel()` / `is_cancelled()`, `Clone`. Lives in the crate so both the
  engine and the shell share one type.
- **`Transcriber` trait** gains cancellation + progress hooks:
  `transcribe(&self, samples, language, cancel: &CancelToken, on_progress: Box<dyn FnMut(i32) + Send>)`.
  Both whisper callbacks must be `'static`, so: `WhisperTranscriber` **clones**
  the token's `Arc<AtomicBool>` into
  `params.set_abort_callback_safe(move || cancel.is_cancelled())` (an owned
  clone, hence `'static`), and **moves** the owned `on_progress` box into
  `params.set_progress_callback_safe` (a borrowed sink couldn't satisfy
  `'static`, which is why progress is passed by value, not by `&mut`).
  - The Linux test fakes honor the token (a pre-cancelled token returns an
    "aborted" `Err` without producing segments) and can invoke `on_progress`.
- **`transcribe_recording`** threads the `CancelToken` through and checks it
  **right after decode** (decode is fast, but a cancel during it should bail
  before inference); on a cancelled token it returns a typed
  `Cancelled`-style error so the shell can tell cancel from failure. It also
  accepts the progress forwarder.

Engine changes sit behind the `whisper` feature (Windows CI gate); the
orchestration + token + fake are testable on Linux.

### Tauri shell (`capture_commands.rs`) — phase-aware worker

**Active-job state.** `TranscriptionQueue` gains
`active: Option<ActiveJob>` where:

```rust
struct ActiveJob {
    mp3: PathBuf,
    vault_id: String,
    cancel: CancelToken,
    started_at_ms: u64,
    phase: Phase,                 // Downloading{recv,total} | Preparing | Transcribing
    progress: Arc<AtomicU8>,      // 0..100 inference %, updated lock-free from the callback
}
enum Phase { Downloading { received: u64, total: Option<u64> }, Preparing, Transcribing }
```

The worker sets `active` under the queue mutex **after** it commits to a job
(post `is_recording` gate, post `pop_front`), and clears it when done — never
holding the mutex across inference. The whisper progress callback writes only
the `AtomicU8` (lock-free); coarse phase changes take the mutex briefly.

**Phase transitions (each observable).** In `process_transcription`:

1. Emit `capture:transcribing { mp3, vaultId }` → set active, phase
   `Preparing`.
2. `ensure_model`: if a download is needed, phase `Downloading`; the download
   callback updates `{received,total}` and emits throttled
   `capture:modelDownload { mp3, model, received, total }` (now carries
   `mp3`).
3. **Handover:** the instant `ensure_model` returns Ok (downloaded **or**
   already present), emit `capture:modelReady { mp3 }` and set phase
   `Preparing`. This replaces the download row immediately — *before* the
   (possibly multi-second) model load, so it can never stick at "100%".
4. The model loads (only when the tier changed; a cached model skips it);
   inference then begins → phase `Transcribing`, seeded with an initial
   `capture:transcribeProgress { mp3, progress: 0 }`. The progress callback
   updates `progress` and emits throttled `capture:transcribeProgress` (~every
   5%). Log inference start/periodic/%/done.
5. Terminal: `capture:transcribed` / `capture:transcribeFailed` /
   **`capture:transcribeCancelled { mp3 }`** (new). On a cancel-token abort,
   the worker writes the `cancelled` sidecar via `force_write_sidecar`
   (replacing our own placeholder — never a `complete`/user file) and emits
   `transcribeCancelled` **without** the scary failure toast.

**New commands:**

- `cancel_transcription(path)` — under the queue mutex: if `path` is the
  active job, `active.cancel.cancel()` (aborts inference; the worker finishes
  the cancelled bookkeeping); if it's a pending job, remove it from `pending`
  + `known` and emit `capture:transcribeCancelled` + write the `cancelled`
  sidecar directly. Unknown path → typed error.
- `transcription_status()` → the live picture for a freshly-opened view:

  ```jsonc
  {
    "active": { "mp3", "vaultId", "phase", "progress", "received", "total", "startedAtMs" } | null,
    "queued": [ { "mp3", "vaultId" } ],
    "waitingForRecording": bool   // worker postponed because a recording is live
  }
  ```

**Hygiene, in scope:** name the transcription worker thread
(`transcribe-worker`) — `run_transcription` currently uses a bare
`std::thread::spawn`, violating the "every spawned thread is named"
diagnostics invariant.

### Vue frontend

**`capture` store — a per-job transcription model.** Replace the scattered
singular fields (`transcribing`, `modelDownload`, `transcriptError`,
`transcriptFailedMp3`, `transcribingVaultId`, `lastTranscribed`) with one
reactive map:

```ts
type Phase = "queued" | "downloading" | "preparing" | "transcribing"
           | "done" | "failed" | "cancelled";
interface TranscriptionJob {
  mp3: string; vaultId: string; name: string;
  phase: Phase; progress: number | null;   // 0..1 for downloading/transcribing
  model: string | null; error: string | null; startedAtMs: number | null;
}
// state: transcriptions: Record<string, TranscriptionJob>, waitingForRecording: boolean
```

- `init()` seeds it from `transcription_status()` and installs listeners for
  every `capture:transcrib*` / `modelDownload` / `modelReady` /
  `transcribeProgress` event, each upserting the keyed job. `modelReady`
  clears the download progress and moves the job to `preparing` (covering the
  model load); the first `transcribeProgress` moves it to `transcribing`
  (fixes issue 4).
- Derived getters keep existing surfaces thin: `activeTranscription`
  (phase ∈ downloading/preparing/transcribing), `queuedTranscriptions`,
  `finishedTranscriptions` (done/failed/cancelled this session, bounded,
  newest first), and `transcribingVaultId` (= active job's vault, for the
  vault-row dot).
- Actions: `cancelTranscription(mp3)`, `retranscribe(mp3)` (force),
  `openTranscript(mp3)`.

**New `Transcriptions.vue` view** (registered in the `vaults` store `view`
union + `ActionPanel`, same pattern as `Recordings`):

- **Active** (0–1): name · vault · phase label · **progress bar**
  (download % or inference %) · elapsed · **Cancel**. When
  `waitingForRecording`, the label reads "Waiting for the recording to
  finish…". A **"taking longer than expected"** hint appears if a
  `transcribing` job's % has not advanced for ~2 minutes (tracked from
  `transcribeProgress` timestamps) — the honest "might be stuck" signal.
- **Queued** (N): name · "Waiting" · **Cancel**.
- **Finished this session** (bounded): ✓ done → **Open in Obsidian**;
  ⚠ failed → error + **Re-transcribe**; ⦸ cancelled → **Re-transcribe**.

**Compact summary indicator** (replaces the single-line `TranscriptionStatus`
on the main panel list view): `⟳ Transcribing "X" — 42% · +2 queued`, or
`⚠ 1 transcription failed`, or nothing when idle — clickable to open the
Transcriptions view. The Recordings view also links to it.

**`Recordings.vue` fixes (issues 2 + 1).** The row's "is it transcribing?"
state derives from the store's live job map (backend-seeded), not a local
`ref` — so it survives the remount that caused the stale re-transcribe bug:

- Re-transcribe button **disabled** when the mp3's job phase is active
  (queued/downloading/preparing/transcribing). A `pending` sidecar on disk
  that is **not** in the live map (crash-stuck) stays re-transcribable —
  preserving the intent of the earlier "keep re-transcribe available for a
  stuck pending recording" fix.
- An animated **spinner** on actively-transcribing rows (replaces the static
  `…` glyph for those rows).
- A **Cancel** button on actively-transcribing/queued rows.

**Record-view settings (issue 3).** Extract the transcribe block
(toggle + model + language + timestamps) from `CaptureSettings.vue` into a
**controlled** `TranscriptionSettings.vue` (v-model'd object; no persistence
of its own). `RecordMode.vue` — which already loads the vault `CaptureConfig`
— renders it above the Meeting/Voice buttons and **saves the full vault
config** on change (updating only the transcription fields). `CaptureSettings`
uses the same component inside its existing form/Save button. The recording a
user starts immediately after picks up the change (config is re-read at
finalize).

## Data flow (a re-transcribe with a missing model, happy path)

1. User clicks Re-transcribe in the Recordings list → `retranscribe(mp3)`
   (force) → enqueued; row shows a spinner + Cancel; the main-panel summary
   appears.
2. Worker commits: `active` set, `capture:transcribing` → store job phase
   `preparing`.
3. Model absent → phase `downloading`; `capture:modelDownload` progress
   drives the bar in the Transcriptions view.
4. Download completes → `capture:modelReady` → phase `preparing`, download
   bar replaced (issue 4 fixed); the model loads; inference begins → phase
   `transcribing`.
5. Inference runs; `capture:transcribeProgress` ticks the bar 0→100%; the log
   records start/periodic/done. If the user hits **Cancel**, the abort
   callback trips, `full()` returns `Err`, the worker writes the `cancelled`
   sidecar and emits `capture:transcribeCancelled`; the view moves the job to
   "cancelled" with a Re-transcribe action, and nothing re-queues it on next
   launch.
6. On success → `capture:transcribed`; the job moves to "done ✓" with
   Open in Obsidian; the Recordings row flips to `complete`.

## Error handling / invariants (unchanged contract)

- **Best-effort, never harms audio/note.** Cancel and every new path leave
  the MP3 and companion note untouched; only the sidecar changes, always via
  the atomic collision-safe writer, and cancel writes only over our own
  placeholder (never a `complete` or user-edited file).
- **No new deadlock surface.** The queue mutex is never held across
  inference; inference progress is lock-free (`AtomicU8`); `cancel` and
  `transcription_status` take the mutex only briefly. The whisper progress
  callback touches no Tauri window state.
- **Cancel can't wedge shutdown.** Quit does not wait on the transcription
  worker (transcription is best-effort); a cancelled or killed job leaves a
  sidecar the next launch handles per its marker (`cancelled` → left alone;
  `pending`/`failed` → re-queued).
- **No hidden processing.** Every phase transition is logged (queue,
  download, ready, inference start/%/done, success/fail/cancel), satisfying
  the auditability rule the report exposed.
- **Config parity.** The Record-view settings write the same vault config the
  gear-icon screen does; `transcribe` still gates both the live and recovered
  paths.

## Testing

Same split as increments 1–3.

- **`vault_buddy_core` (Linux CI):** `cancelled` marker round-trip
  (`render_cancelled`, `transcript_status`, `as_dto_str`), `is_regenerable`
  excludes `cancelled`, `pending_transcriptions` skips a cancelled sidecar
  (the no-auto-retry guarantee).
- **`vault_buddy_transcribe` (Linux CI, no whisper):** `CancelToken` behavior;
  `transcribe_recording` with a pre-cancelled token writes no `complete`
  sidecar and returns the cancelled error; the progress forwarder is invoked;
  the fake honors both hooks.
- **Vitest:** the store job-map reducer (each event → correct phase/progress;
  `modelReady` clears download and moves to `transcribing`);
  `transcription_status()` seeding; `Transcriptions.vue` rendering (active bar
  for both download and inference, queued + cancel, waiting-for-recording,
  finished/failed/cancelled with actions, the stuck hint); the summary
  indicator (counts, click-through); `Recordings.vue` (disabled re-transcribe
  from live map, spinner, cancel; stuck-pending stays enabled);
  `TranscriptionSettings.vue` (controlled) and `RecordMode.vue` persistence.
- **Shell (Windows CI gate, compile + manual):** the two new commands, worker
  phase transitions + active-job tracking + cancel handling, the engine abort
  + progress callbacks, `modelReady`/`transcribeProgress` emits — mirrored
  from existing patterns, verified against pinned whisper-rs 0.16.
- **Manual Windows checklist:** re-transcribe a recording, watch the download
  bar hand off to a moving inference bar; cancel mid-inference and confirm the
  audio/note survive, the row reads "cancelled," and a relaunch does **not**
  re-transcribe it; toggle transcription from the Record view and confirm the
  next recording respects it; leave and re-enter the Recordings list mid-run
  and confirm the re-transcribe button stays disabled with a spinner.

## Success criteria

1. A running transcription can be cancelled from both the main-panel summary
   and the Recordings list; cancelling stops inference promptly, leaves the
   audio + note intact, marks the recording `cancelled`, and does not
   auto-re-transcribe on the next launch.
2. The Recordings list shows a live spinner on the transcribing row and
   disables its re-transcribe button for the duration — surviving leaving and
   re-entering the view — while a genuinely stuck `pending` recording stays
   re-transcribable.
3. The Transcriptions view shows the active job with a real progress bar
   (download %, then inference %), the queue, and this session's outcomes,
   each with the right action; a stalled job is visibly flagged.
4. When a model must be downloaded first, the download row is replaced by the
   transcribing state the moment the download completes.
5. Transcription settings (on/off, model, language, timestamps) are available
   on the Record view and persist to the vault config.
6. The app log records inference start, progress, and completion — no silent
   multi-minute gaps.
7. `vault_buddy_core` and `vault_buddy_transcribe` unit tests and the Vitest
   suite pass in CI (Linux); the static-linked whisper.cpp build passes in the
   `windows-app` job.
