# Error feedback, propagation & open-findings cleanup — design

Status: draft for review
Date: 2026-07-06
Branch: `claude/vault-buddy-local-stt-rktgl2` (follows increment 4 — transcription control & progress)

## Why

An audit of the transcription + capture failure surface found that the
**backend already carries failure reasons** — `capture:failed`,
`capture:transcribeFailed`, and `capture:warning` all include a
human-readable `message`, and they reach the store (`capture.error`,
`capture.warning`, per-job `job.error`). The user is left in the dark because
of the **frontend**, plus three backend swallows:

1. **Errors are gated to one view.** The only error/warning banners live in
   `ActionPanel` and render `view === 'list'` (error) or `list && idle`
   (warning). Any failure while the user is on Recordings, Transcriptions,
   Settings, or Record-mode is invisible — including the action they just took
   *on that view*.
2. **Scattered swallows.** `cancelTranscription`, `retranscribe` (from the
   Transcriptions view — the store action has **no `try/catch`** → unhandled
   rejection), `openTranscript`, and the Record-view transcription-settings
   save all `logWarning` only.
3. **The reason is hidden even when we have it.** The Recordings list and the
   summary show a generic "Transcript failed"; the real `job.error` reason is
   rendered in exactly one place (the Transcriptions view).
4. **Three genuine backend swallows** that actively mislead: a never-clobber
   `SkippedForeign` (we kept an existing/hand-edited transcript) is reported as
   **success**; a **mid-recording disk failure** ends the recording early but
   the reason is dropped from `capture:saved`; and a **companion-note write
   failure** after a good audio save is `log::warn!`-only.

## Goals

- One **view-independent** surface so failures are visible wherever the user
  is, next to the action that triggered them.
- Every scattered/swallowed frontend error path reaches the user.
- The real **reason** is shown, not a generic "failed".
- The three backend swallows become honest signals.
- All 11 tracked open findings from the increment-4 review are resolved.

## Non-goals

- No change to the never-clobber write contract, the transcription engine, or
  the recording pipeline's behavior (only what it *reports*).
- No global telemetry / error-reporting service. This is local, in-app feedback.
- No persistence of a failed transcript's reason into `list_recordings`
  (the reason lives in the failed sidecar body; a *live* failure this session
  carries `job.error`, and every live failure also raises a notification).

---

## Design

### 1. Notification store — the single transient-message surface

New `src/stores/notifications.ts` (Pinia):

```ts
type NotifyKind = "error" | "warning" | "success" | "info";
interface Notification { id: number; kind: NotifyKind; message: string; }

// state
items: Notification[]            // newest last; capped at MAX_ITEMS (5), oldest dropped

// actions
notify(kind, message, opts?: { ttlMs?: number | null }): number   // returns id
error(message): number     // ttl null (sticky)
warning(message): number   // ttl null (sticky)
success(message): number   // ttl 4000 (auto-dismiss)
info(message): number      // ttl 4000
dismiss(id): void
clear(): void
```

- **Auto-dismiss** is scheduled inside `notify` via `setTimeout(dismiss, ttlMs)`
  when `ttlMs != null`. Errors/warnings are sticky (`null`) — a problem should
  not vanish before it's read. Successes/info auto-dismiss.
- **Dedupe:** if the newest un-dismissed item has the same `kind` + `message`,
  `notify` is a no-op returning that id (prevents a retried command spamming the
  same line). Ids are a monotonic counter (no `Date.now`/`Math.random` needed).
- **Cap** at `MAX_ITEMS` so a burst can't grow unbounded.

New `src/components/NotificationHost.vue`:

- Renders the stack, kind-colored (error red / warning amber / success emerald /
  info slate), each with the message and a dismiss `✕`.
- `role="alert"` + `aria-live="assertive"` for errors; `role="status"` +
  `aria-live="polite"` otherwise.
- Positioned as an overlay pinned to the **bottom of the panel**, above content,
  so it shows in **every** view. Mounted **once** in `ActionPanel.vue`.
- The panel window is only hidden/shown (never unmounted), so the host and its
  timers persist across opens — consistent with the existing store lifetimes.

### 2. Routing rules — what surfaces where

- **Removed:** the list-view-only `capture.error` / `capture.warning` banners in
  `ActionPanel`. Their content now flows through the notification host (all views).
- **`capture:failed{message}`** → `notifications.error(message)`.
- **`capture:transcribeFailed{mp3, message}`** → keep setting per-job `job.error`
  (drives inline row/summary display) **and** raise
  `notifications.error("Transcription failed: " + message)` so it's seen off the
  Transcriptions view too.
- **Command rejections** now route to the notification surface instead of
  `logWarning`-only: `cancelTranscription`, `retranscribe` (add the missing
  `try/catch` in the store action), `openTranscript`, `open_recording`, and the
  Record-view `set_capture_config` save (`RecordMode.vue`). `logWarning` stays
  as the file-log breadcrumb; the notification is the user-facing half.
- **Live recording warnings** (`capture:warning` while a recording is active)
  stay inline in `RecordingBar` (contextual, live). Warnings that arrive outside
  an active recording also raise a notification.
- **Store fields `capture.error` / `capture.warning` are retained as signals**
  (not display): they are still set by the event handlers so the buddy and
  `RecordingBar` can watch them, but their **only display** is now the
  notification host — the `ActionPanel` banners are removed. Setting either also
  raises the matching notification. Keeping the fields (vs routing purely through
  the notifications store) is the deliberate lower-churn choice: the buddy and
  RecordingBar watchers stay put, and existing tests that assert on
  `capture.error`/`warning` keep meaning what they meant.
- **Successes** are intentionally quiet — the buddy bubble already confirms
  "Transcript ready!" / "Saved". The success/info kinds exist for the rare case
  with no existing confirmation (e.g. the "kept existing transcript" notice
  below is a `warning`, not a silent success).
- **Buddy signal:** `useBuddyAnnouncements` speaks the **reason** — a short,
  truncated form of the retained `capture.error` message (and of a failed job's
  `job.error`) — instead of the generic "Hmm, that didn't work 😕".

### 3. Inline reasons where state is persistent

- **`Recordings.vue`:** a failed row keeps the "Transcript failed" label but its
  `title` shows `job.error` when a live job carries one (hover-to-see-why). A
  fetched historical failure keeps the generic label (reason is in the sidecar).
- **`TranscriptionSummary.vue`:** the "N failed" chip's `title` shows the newest
  failure reason; clicking still opens the Transcriptions view (where the full
  reason renders). No layout change.

### 4. Backend honesty fixes (Rust; Windows-shell parts are compile-bracketed)

- **`SkippedForeign` is not success.** `transcribe_recording` returns a small
  outcome enum distinguishing `Written(path)` from `SkippedForeign(path)` (a
  Complete/hand-edited sidecar we refused to overwrite). The shell emits a new
  **`capture:transcribeSkipped{mp3, message}`** for the skip (message e.g.
  "kept your existing transcript — not overwritten") and keeps `capture:transcribed`
  only for a real write. Store: sets the job phase to `done` (a complete
  transcript **does** exist) and raises `notifications.warning(...)` so the user
  knows their file was preserved, not regenerated. `is_regenerable` and the
  never-clobber guards are unchanged.
- **`capture:saved` carries the early-stop reason.** `emit_saved` includes
  `endedEarly: bool` and `warning: string | null` (from the existing
  `Outcome.warning`). Store reads them: on `endedEarly` or a present `warning`,
  raise `notifications.warning("Recording ended early: " + reason)` instead of a
  reason-free OS toast only.
- **Companion-note write failure is surfaced.** When the audio saved but the
  companion note failed (`session.rs`), record the reason into `Outcome.warning`
  (today it's `log::warn!`-only). It then rides the same `capture:saved.warning`
  channel → `notifications.warning("Saved the recording, but the note couldn't be
  written: " + reason)`.

These change `transcribe_recording`'s return type and the `capture:saved` payload
→ they break the Windows-only shell until it's updated, so they are **held and
pushed as one compile-bracket** (same discipline as increments 2–4); the
`windows-app` CI job is the gate. The `transcribe`/`capture`/`core` crate parts
are Linux-testable.

### 5. Open findings folded in

Robustness (shell, bracketed):
- **A1** — the two new `TranscriptionState` commands use `lock_ignoring_poison`
  (not `.lock().unwrap()`), matching the file's no-abort-across-FFI discipline.
- **A2** — `transcription_queue_status` snapshots `is_recording` **before** taking
  the `TranscriptionState` lock (drop-before-check, matching `run_recovery`).
- **C1** — a `set_phase()` helper replaces the ~3× repeated set-phase-under-mutex
  block.
- **D1** — fix the stale `TranscriptionStatus` reference in `open_transcript`'s doc.

Frontend UX:
- **B1** — Transcriptions active/queued rows show the vault **name** (via
  `useVaultsStore`), not the raw hex id.
- **B2** — a `downloading` phase with unknown total shows the spinner +
  "Downloading model…", not a bogus "…0%".
- **B3** — `waitingForRecording` re-syncs from capture state (derive it or update
  it on the relevant capture event) rather than only seeding once.
- **B4** — the "taking longer than expected" stuck timer lives in the store so it
  survives the view remount (today it's component-local and resets on nav).
- **C2** — the mm:ss `elapsed` formatter is extracted to one shared util (3rd
  consumer: `RecordingBar`, `Transcriptions`, and the new surface).
- **C3** — `TranscriptionSettings` DOM ids are scoped (id prefix) so two
  instances can't collide.

Tests:
- **E1** — a dedicated injection test for `render_cancelled`.
- **E2** — a `FakeOk`-based precancelled test that isolates the after-decode
  early-bail (FakeOk ignores the token, so it only passes if the bail fires).
- **E3** — assert `on_progress` is actually invoked (counter), not just smoke-run.
- **E4** — cover `activeSeedProgress`'s downloading branch; assert the
  `transcriptions` map is bounded.
- **E5** — cover the `TranscriptionSummary` a11y (role/tabindex/keydown) additions.

---

## Invariants preserved

- **Never-clobber** is untouched: `SkippedForeign` reporting only *changes what we
  say*, not what we write; guarded `replace_if_ours` vs sanctioned
  `force_write_sidecar` usage is unchanged.
- **No swallowed error** (diagnostics invariant) is now enforced end-to-end: a
  caught error still logs (`log::*` / `logWarning`) **and** now reaches the user
  through the notification surface or an inline reason. `logWarning` is the
  breadcrumb; the notification is the user half.
- **Main-thread / lock discipline** unchanged; A1/A2 only make the new commands
  *more* consistent with it. No new lock is taken across inference or held across
  an fsync.
- **Compile-bracket**: backend signature/payload changes are held and pushed
  together; `windows-app` never sees a broken intermediate.

## Testing

- **Frontend (Vitest, Linux):** the `notifications` store (queue, ttl with fake
  timers, dedupe, cap); the host renders/dismisses per kind; each rewired error
  path raises a notification (cancel, retranscribe-from-Transcriptions,
  openTranscript, RecordMode save); Transcriptions vault-name + "0%" fix; the
  buddy speaks the reason; store handling of `capture:transcribeSkipped` and the
  new `capture:saved` fields.
- **Rust (Linux):** `transcribe_recording` returns `SkippedForeign` vs `Written`
  (`core`/`transcribe` unit tests); `Outcome.warning` carries the note-write
  reason; E1–E3 above.
- **Windows (CI `windows-app`):** the shell compiles with the new return
  type/payload, the `set_phase` helper, the poison-safe locks, and the
  `transcribeSkipped`/`saved` emits.

## Rollout / compile-bracket

- Frontend-only tasks push independently (frontend CI gates each).
- The `transcribe` return-type change, the `capture` `Outcome.warning`/note-write
  change, and every shell edit (`emit_saved` payload, `transcribeSkipped` emit,
  A1/A2, `set_phase`, D1) are **held and pushed as one bracket**; the
  `windows-app` job confirms the Windows/MSVC build before the branch is green.
