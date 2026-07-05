# Recordings Enhancements Design — Record-as-view, back button, re-transcribe

- **Date:** 2026-07-05
- **Status:** Approved
- **Source:** Follow-up polish on the just-shipped recordings feature. Three
  asks: (1) make the Start Recording modal a proper panel view; (2) replace the
  misleading "cog" header button with a real back button; (3) add a
  re-transcribe button to each recording row.

## Goal

Three independent enhancements to the recordings/record-mode UI, plus the one
backend capability the third requires (an explicit, forced re-transcription that
can regenerate an already-finished transcript on demand). No change to the
never-clobber contract for the audio or companion note; the only forced write is
the transcript sidecar the user explicitly asks to regenerate.

---

## Part 1 — Record chooser becomes a panel view

`RecordModeDialog.vue` (a modal overlay) becomes `RecordMode.vue`, a panel view
rendered in the body like `Recordings`/`CaptureSettings`.

- **Store (`vaults.ts`):** `view` gains `'recordMode'`; add `recordModeVaultId:
  string | null` and `openRecordMode(vaultId)`; `showList()` clears it.
- **Entry:** the vault-row **capture button** calls `store.openRecordMode(vaultId)`
  — the component-local `recordRequest` modal state in `ActionPanel` is removed
  entirely.
- **`RecordMode.vue`:** on mount, fetch `get_capture_config` for the vault's
  default mode (on error, fall back to `meeting` — preserving today's "a config
  read never blocks recording" rule). Render **Meeting / Voice Note** (the default
  highlighted) and a **Browse recordings** row. Selecting a mode →
  `capture.start(vaultId, mode)` then `store.showList()` (returns to the list,
  where the recording bar appears). Browse → `store.openRecordings(vaultId)`.
- Drops the modal chrome: backdrop, ✕ button, `aria-modal`, escape-to-cancel.
  The panel header (owned by `ActionPanel`) supplies the title ("Record") and the
  back button.

## Part 2 — Proper back button (← replaces the cog)

The header today has one button that renders a cog glyph but acts as "back" in
every non-list view — the misleading part.

- **List view:** header keeps the **cog** → buddy settings (its real purpose).
- **Non-list views:** header shows a **← back** button. Its target is the view's
  *parent* (each view has exactly one):

  | View | ← back goes to |
  | --- | --- |
  | `recordings` | **Record view** (its only entry point) |
  | `recordMode` | Vaults |
  | `captureSettings` | Vaults |
  | `settings` | Vaults |

- **Implementation:** a `store.back()` action — `recordings` →
  `openRecordMode(recordingsVaultId)`, everything else → `showList()`. One fixed
  parent per view; no history stack. The header title switch gains "Record".

## Part 3 — Re-transcribe button per recording

### Backend

- **`core::transcript`:**
  - `TranscriptStatus { Missing, Pending, Failed, Complete }` + `transcript_status(mp3)
    -> TranscriptStatus` reading the `<base>.transcript.md` sidecar marker
    (`Missing` = no file; `Pending`/`Failed` = our regenerable markers; anything
    else, incl. a finished transcript or a user's hand-edit → `Complete`, so the
    confirm fires on it).
  - `force_write_sidecar(path, content)` — a **forced** atomic overwrite
    (owned temp + fsync + REPLACING rename) of the transcript sidecar, used by
    the re-transcribe path for both the "transcribing…" placeholder overwrite and
    the final transcript write. The auto/recovery paths keep the existing
    idempotent `write_placeholder` + never-clobber `replace_if_ours`.
- **`core::recordings`:** `RecordingEntry` gains `transcript_status`; the scan
  reads each recording's sidecar (one extra file read per recording, beside the
  companion-note read it already does).
- **Shell (`capture_commands.rs`):**
  - `TranscriptionJob` gains `force: bool`.
  - New command **`retranscribe(path)`** — enqueues a job with `force = true`.
    A forced job (a) **bypasses the vault's `transcribe` setting** (an explicit
    per-recording opt-in, independent of the automatic default) and (b) uses
    `force_write_sidecar` for the placeholder and final write, so it regenerates
    even a `complete` sidecar. The model downloads on demand via the existing
    `capture:modelDownload` progress path. Model tier + language come from the
    vault config as usual.
  - `transcribe_recording_now` (the failed-retry path) is unchanged: `force =
    false`, still gate-respecting and regenerable-only.
  - `RecordingDto` gains `transcriptStatus` (lowercased string).
  - Register `retranscribe`.
- **`transcribe` crate:** `transcribe_recording(..., force: bool)` — on `force`,
  the final write is `force_write_sidecar`; otherwise `replace_if_ours`
  (unchanged behavior/signature-compatible via the new param).

### Frontend (`Recordings.vue`)

- `Recording` type gains `transcriptStatus: "none" | "pending" | "failed" |
  "complete"`.
- Each row gains a small **transcript-status indicator** (transcribed ✓ /
  failed / — ) and a **re-transcribe icon button**.
- Click behaviour: `transcriptStatus === 'complete'` → an inline **"Replace the
  current transcript?"** confirm → `invoke('retranscribe', { path })`; otherwise
  (`none`/`failed`) → `retranscribe` immediately.
- The row reflects a transient **transcribing…** state and settles to
  complete/failed, driven by the existing `capture:transcribing / transcribed /
  transcribeFailed` events (each carries the `mp3`, so the matching row updates
  in place). The view sets up those listeners for its lifetime.

## Architecture (files)

| Path | Change |
| --- | --- |
| `core/src/transcript.rs` | `TranscriptStatus` + `transcript_status`; `force_write_sidecar` |
| `core/src/recordings.rs` | `RecordingEntry.transcript_status`; scan reads the sidecar |
| `transcribe/src/lib.rs` | `transcribe_recording(..., force)` |
| `src/capture_commands.rs` | `TranscriptionJob.force`; `retranscribe` command; force branch in `process_transcription`; `RecordingDto.transcriptStatus`; register |
| `src/lib.rs` | register `retranscribe` |
| `src/types.ts` | `Recording.transcriptStatus` |
| `src/stores/vaults.ts` | `recordMode` view + `recordModeVaultId` + `openRecordMode` + `back()` |
| `src/components/RecordMode.vue` | new (from `RecordModeDialog.vue`) |
| `src/components/ActionPanel.vue` | header back button; `recordMode` slot/title; capture button → `openRecordMode`; remove modal |
| `src/components/Recordings.vue` | per-row status + re-transcribe button + confirm + transient status |

## Invariants preserved

- **Vault-write surface does not grow.** Re-transcribe reuses the transcript
  sidecar's atomic temp+fsync+rename; a forced write overwrites **only** the
  `<base>.transcript.md` the user explicitly asked to regenerate (confirmed when
  it is finished), never the audio (`.mp3`) or companion note (`.md`).
- **Auto/recovery transcription is unchanged** — still idempotent placeholder +
  never-clobber `replace_if_ours`; only the new explicit `retranscribe` command
  forces.
- **Read-only recordings list otherwise** — opening a row still hands off to
  Obsidian via `obsidian://` (`open_recording`), no write.
- **Config-read-never-blocks-recording** — `RecordMode.vue` falls back to
  `meeting` on a config read error, as the modal did.

## Edge cases

- **Re-transcribe with the vault's transcription off** — works (explicit opt-in);
  downloads the model on demand with the existing progress UI; uses the vault's
  configured tier/language (defaults if never set).
- **Re-transcribe a `failed` or missing transcript** — no confirm; runs
  immediately.
- **Re-transcribe a hand-edited transcript** — classified `Complete`, so the
  confirm fires ("Replace the current transcript?") before any overwrite.
- **Navigating away mid-transcribe** — the worker keeps running (backend queue);
  re-entering the recordings list reflects the settled status from the fresh
  `list_recordings` scan.

## Testing

- **Core:** `transcript_status` classification (missing/pending/failed/complete)
  + `force_write_sidecar` overwrites a complete sidecar (and round-trips);
  `list_recordings` reports `transcript_status` per recording (tempdir tree with
  a complete/failed/absent sidecar).
- **Transcribe:** `transcribe_recording(force=true)` overwrites a complete
  sidecar; `force=false` keeps the existing never-clobber behavior (extend the
  existing tests).
- **Shell:** unit-testable pieces stay in core; the `retranscribe` command +
  force branch are Windows-compiled (CI gate).
- **Frontend (Vitest):** `RecordMode.vue` (fetches default, emits start/browse);
  `vaults` store (`openRecordMode`, `back()` targets incl. recordings→recordMode);
  `ActionPanel` (back button per view, capture→recordMode, record slot);
  `Recordings.vue` (status indicator, re-transcribe invoke, confirm-on-complete,
  transient status via events).

## Non-goals / scope guards

- Re-transcribe is a **per-row icon button**, not hidden behind a row expansion;
  the status indicator is **minimal** (a small icon/label, not a badge system).
- **No history stack** — one fixed parent per view.
- **No new vault-write path** — the forced write stays within the existing
  transcript-sidecar surface.
- No batch/select-all re-transcribe; no per-recording model/language override
  (uses the vault config).

## Addendum — 2026-07-05 (post-review data-safety hardening)

The final whole-branch review surfaced two narrow corners in the Part 3 design;
both are refined here (the approved body above is left intact as the review
trail — this addendum is the correction of record).

- **Forced placeholder must not clobber a finished transcript up-front.** Part 3
  had the forced job stamp the "transcribing…" placeholder via
  `force_write_sidecar` *before* the regenerated transcript exists. For a
  `Complete`/hand-edited sidecar that destroys the original immediately, so a
  forced re-transcribe that then fails (model download, model load, or decode
  error) leaves a `failed` placeholder and the user's transcript gone — a
  never-clobber violation, since the user asked to *replace* the transcript, not
  discard it on failure. **Refinement:** the up-front placeholder is written only
  when the sidecar is **not** already `Complete`. A `Complete`/hand-edited sidecar
  is left in place; on success `transcribe_recording`'s `force_write_sidecar`
  swaps in the finished transcript, and on failure `fail_transcription`'s
  `replace_if_ours` skips the non-regenerable original, so it survives intact.
  `force_write_sidecar` is thus the **final** transcript write; the forced
  placeholder covers only missing/`pending`/`failed` sidecars. The row still
  reflects the in-flight "transcribing…" state via `capture:transcribing`.
- **Re-transcribe button must not strand a stuck `pending` recording.** The
  button's `disabled` guard included `transcriptStatus === 'pending'`, which
  permanently disables re-transcribe for a sidecar stuck at `pending` (a crash
  left a placeholder, no job running) — unrecoverable. **Refinement:** the button
  is gated **only** on this session's transient in-flight set
  (`transcribingMp3.has(mp3)`), never on the persisted `pending` status.
