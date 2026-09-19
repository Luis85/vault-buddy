# Native communication contracts

## Status and naming

The following `editor_*` API is a **proposed implementation contract**. It is not claimed to exist in the checked repository. The existing capture API and its exact payloads are retained. TypeScript `invoke<T>` is compile-time assistance, not runtime validation of unknown JSON; decode responses and validate requests natively. Generate synchronized Rust/TypeScript types from one controlled source during implementation, or use fixture tests that detect drift. [Source evidence and official Tauri guidance](REFERENCES.md).

## Existing capture API — reuse

| Command | Input | Result / behavior |
|---|---|---|
| `list_capture_sources` | None | Existing available capture-source descriptors. |
| `start_screen_capture` | `id` (vault), `sourceId`, `inputs[]`, `outputs[]` | Authoritative capture status; input/output names preserve device identity. |
| `pause_screen_capture` | None | Request pause; event/status reflects native clock. |
| `resume_screen_capture` | None | Resume; native accumulated pause time remains authoritative. |
| `stop_screen_capture` | None | `{stillSaving:boolean}`; terminal result arrives separately. |
| `screen_capture_status` | None | Current capture truth for resynchronization. |

Existing `screen:stopped` is `{base,path,durationMs,sourceTitle,width,height}`. Existing `screen:failed` carries `{message,retainedPath}`. Started/paused/resumed/warning/frames messages remain capture-owned. Do not reuse these names for editor render progress. Do not parse retained file locations from display prose.

## Proposed editor API

| Command | Core input | Core output and rules |
|---|---|---|
| `editor_open_staged` | `stagedBase` | `EditorSnapshot`; native lookup of sidecar and destination; idempotent per staged source. |
| `editor_open_project` | Authorized `projectFileId` | Snapshot plus missing-media report; validate before replacing active document. |
| `editor_get_snapshot` | `sessionId`, optional known revision | Current summary and graph/delta; never install another session's result. |
| `editor_execute` | `sessionId,expectedRevision,commandId,command` | Acknowledged snapshot/delta; semantic validation, serialized transactions, idempotent command ID. |
| `editor_import_media` | `sessionId`, native dialog grants/file IDs | Job ID; batch progress + retained successes + per-file errors; cancellation stops future imports. |
| `editor_relink_media` | `sessionId`, asset IDs, authorized candidates | Match/probe report; no ambiguous auto-match, no silent source replacement. |
| `editor_save_project` | `sessionId,expectedRevision`, resolved save target/options | Save receipt **after commit**. Portable export includes historical dependencies. |
| `editor_start_render` | Render request + progress channel | Accepted job ID, exact source revision, output preset/range; completion is a separate terminal record. |
| `editor_cancel_job` | `sessionId,jobId` | Cancellation request acknowledged; job may already be publishing/completed. |
| `editor_get_jobs` | `sessionId` | Authoritative current/terminal job states after reload, missed event or reconnect. |
| `editor_get_products` | `sessionId` | Immutable product descriptors and file availability. |
| `editor_restore_product` | `sessionId,expectedRevision,productId,commandId` | New current revision from retained edit snapshot; does not modify encoded product. |
| `editor_get_workspace` / `editor_save_workspace` | Session ID, bounded view DTO | UI preferences only, never a domain edit. |
| `editor_get_guide_progress` / `editor_save_guide_progress` | Validated guide DTO/content revision | Separate application preference receipt. No media paths or content. |
| `editor_close_session` | Session ID, explicit disposition | Detach/keep staged/discard only after jobs/resource policy; never generic recursive deletion. |

The small starter implements the TypeScript adapter for open/execute/save/render only. Remaining rows are specified, not implemented examples.

## Summary DTO and update envelope

```ts
interface EditorSnapshot {
  sessionId: string;
  projectId: string;
  revision: number;
  persistedRevision: number | null;
  title: string;
  durationMs: number;
  canUndo: boolean;
  canRedo: boolean;
}
interface ExecuteRequest {
  sessionId: string;
  expectedRevision: number;
  commandId: string;
  command: EditorCommand;
}
interface SaveReceipt {
  sessionId: string;
  savedRevision: number;
  projectFileId: string;
}
```

`EditorSnapshot` in the starter is a **summary**, not the composition graph. Production results include a graph projection or bounded delta with base revision and stable entity IDs. Validate enum members, integer/safe bounds, cross-references, nullability, revision monotonicity and session identity. Do not convert arbitrary `u64` directly to a JS number; bound quantities to the shared safe range or serialize as decimal strings where precision matters.

Committed command families cover rename; add/remove/reorder tracks; insert/remove/move/trim/split clips; group/ungroup/duplicate/paste; volume/mute/solo; fade/transition; speed; transform/crop/frame/color; teaching cues; captions; chapters; canvas and output preferences; Undo/Redo. Each has a pure precondition validator, all-or-nothing graph operation, human-readable undo label and semantic regression fixture.

## Progress, cancellation and reconciliation

Use a Tauri `Channel` for ordered job messages scoped to a subscriber, or equivalent controlled delivery. Include `{sessionId,jobId,sequence,phase,fraction}` and terminal product/error payload. Phases are `queued → preparing → rendering → publishing → complete`, with `cancelled` or `failed` as terminal alternatives. Display fraction as advisory; `fraction:1` alone is not completion. Once terminal, ignore later stale/nonterminal messages. A fresh native job query supersedes incomplete event history.

The adapter discards wrong-session/job and non-increasing sequence messages. Disposing a listener stops UI delivery; it does not cancel native work. Unmount-safe listener cleanup handles asynchronous registrations that complete after unmount. Raw video frames/audio samples do not flow through global JSON events or Pinia. Use a bounded preview path and explicit backpressure.

## Error contract

```ts
type EditorErrorCode =
  | 'invalidRequest' | 'invalidProject' | 'revisionConflict'
  | 'sessionGone' | 'unauthorizedSource' | 'sourceMissing'
  | 'unsupportedMedia' | 'deviceUnavailable' | 'permissionDenied'
  | 'diskFull' | 'writeDenied' | 'destinationUnavailable'
  | 'encoderUnavailable' | 'cancelled' | 'internal';
interface EditorError {
  code: EditorErrorCode;
  message: string;             // concise, safe user explanation
  retryable: boolean;
  operationId: string;
  retainedAssetIds?: string[]; // native-authorized opaque identifiers
}
```

Record diagnostic correlation IDs, not secret file contents or arbitrary full paths in telemetry. Present specific recovery: reconnect source, choose destination, retry save, free storage, view retained capture. Never clear unsaved state on rejection. Request/result schemas must reject unknown enum variants safely; supported forward-compatible fields need an explicit policy, not silent destructive normalization.

## Security requirements

All commands check calling window and session association. Native dialogs grant file access; frontend strings do not. Native output targets are resolved from configured vault IDs and safe relative paths. Cancellation/discard verifies ownership and active references. Metadata returned for missing media is informational, not executable markup. Apply Tauri app-command permission configuration and capability checks, and enforce native paths regardless of those grants. [Security details](PERSISTENCE-AND-SECURITY.md).
