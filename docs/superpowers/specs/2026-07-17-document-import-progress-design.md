# Document import: visible in-progress state — design

Date: 2026-07-17
Status: accepted (autonomous session — user feedback is the driver; see
Requirements)

## Problem

User feedback: "when importing a doc it just grays out the button, nothing
more — I would love to see a better in-progress visualization."

A Pandoc conversion (`convert_document`) can take several seconds, and the
current feedback is minimal and local to the initiating control:

- `RecordMode.vue`'s **Import Document** button disables and swaps its
  hint line to "Converting… this can take a few seconds" — easy to read as
  an inert/broken button.
- `ImportVaultPicker.vue` shows a one-line "Converting {file}…" and a
  16 px spinner on the picked row, above a grayed-but-fully-rendered vault
  list.
- Navigating away (back to the list, or the panel auto-hiding and
  reopening — which defaults to the list view) loses ALL visible trace of
  the running import until the success/error toast fires. Meanwhile the
  other intake surface looks idle: its Import control appears clickable
  but any attempt fails fast on the Rust-side `ImportLock` ("An import is
  already in progress.").

## Constraints (from the code as it is)

- **No real progress data exists.** Pandoc reports nothing incremental;
  `convert_document` is one async IPC call with no events. An honest
  visualization is *indeterminate* (activity + elapsed time), not a
  percentage.
- **At most one conversion runs process-wide** (`ImportLock` in
  `document_commands.rs`, fail-fast `try_lock`). A single "active import"
  slot is the correct frontend model.
- **No cancellation exists** backend-side; `CONVERT_TIMEOUT` bounds a
  runaway conversion. So the UI shows status only — no cancel control.
- The panel webview is hidden/shown, never unmounted: Pinia state
  survives a panel close/reopen. Stores are per-webview; the import state
  is panel-only, which is fine — both intake surfaces and the list view
  live in the panel window.

## Approaches considered

1. **Local polish only** — richer converting UI inside each of the two
   initiating views, no shared state. Rejected: does not fix the core
   complaint's worst case — the working state still vanishes as soon as
   the user leaves the initiating view, and the sibling surface still
   dead-ends into the ImportLock error.
2. **Shared frontend import store + one reusable progress card**
   (chosen) — a small Pinia store owns the conversion lifecycle; a
   self-contained card component renders it anywhere it matters (both
   intake surfaces + the list view). Mirrors the established
   RecordingBar / TranscriptionSummary precedent for "background work is
   visible on the list view".
3. **Rust-side events + buddy indicator** — emit
   `document:importStarted/Finished`, animate the buddy like recording
   does. Rejected as disproportionate: the frontend initiates the call
   and already knows start/end; Rust has no extra progress information to
   contribute; and "the buddy is the recording indicator" is a capture
   invariant — extending the buddy's meaning is its own design change,
   not a patch on this request.

## Design (approach 2)

### `src/stores/documentImports.ts` — the conversion lifecycle owner

```ts
state: {
  // The single in-flight conversion (ImportLock guarantees ≤ 1), or null.
  active: {
    fileName: string;   // basename of the source, for display
    vaultId: string;
    vaultName: string;
    startedAtMs: number;
  } | null;
}
actions: {
  // Sets `active`, invokes convert_document, ALWAYS clears in finally,
  // returns the note path / rethrows the raw IPC error. Callers keep
  // their own success-toast / error-toast / navigation behavior.
  async convert(vault: { id, name }, sourcePath): Promise<string>
}
```

- `convert` pre-checks `active` and throws the same message the Rust
  lock would return ("An import is already in progress.") as a plain
  string — matching the IPC error shape callers already `String(e)` —
  WITHOUT touching `active`. This guards the same-tick race two clicks
  could produce; in normal use the converting UI removes the triggering
  controls entirely. Not overwriting `active` matters: the first
  conversion's `finally` must be the only thing that clears it.
- State lifecycle lives in ONE place so a surface can never strand a
  stale "converting" flag (the picker and record-mode `ref`s it replaces
  each hand-rolled this).

### `src/components/ImportProgress.vue` — the visualization

Self-contained (reads the store directly — the TranscriptionSummary
pattern), renders nothing when `active` is null. When active:

- A sky-accented card (recording is red, transcription violet — a third
  accent keeps concurrent activities distinguishable on the list view):
  - Line 1: spinner + `Converting "{fileName}"` + right-aligned elapsed
    time (1 s tick, shared `formatDuration` — the RecordingBar pattern).
  - Line 2: an **indeterminate sliding progress bar** (scoped CSS
    keyframes; a sweeping segment loops left→right). Honest motion — no
    fake percentage. Under `prefers-reduced-motion` the sweep is
    replaced by a static partial bar (no animation).
  - Line 3: `into {vaultName} — Pandoc is working, this can take a few
    seconds.`
- `role="status"` so screen readers announce the state change; the
  elapsed tick is NOT announced (the timer text sits outside the live
  region), only the appearance of the card is.

### Render sites

- **`RecordMode.vue`**: while any import is active (this vault's or
  another's — only one can run), the Import Document button is replaced
  by the card. This both shows the working state prominently and removes
  the dead-end click into the ImportLock error. The local `importing`
  ref is deleted in favor of the store; the button keeps its
  Pandoc-checking disable.
- **`ImportVaultPicker.vue`**: `viewState` gains a `"converting"` branch
  (checked first): the vault list is replaced by the card while the
  conversion runs — the pick decision is made, so the grayed list is
  noise. The "+N more queued" line stays visible beside the head
  filename (queue semantics unchanged, GAP-55/epoch logic untouched).
  The local `busyVaultId` ref is deleted in favor of the store.
- **`ActionPanel.vue`** (list view): the card renders under
  RecordingBar/TranscriptionSummary whenever an import is active — so
  backing out mid-conversion, or the panel auto-hiding and reopening on
  the list default, never loses the working state.

### Error/success behavior — unchanged

Success toast (with "Open in Obsidian") and error toast stay exactly as
they are; the card disappears when `active` clears in `finally`. The
picker's epoch-guarded dequeue/navigation logic is untouched.

## Testing

TDD, Vitest + happy-dom + mockIPC (no Tauri runtime):

- `tests/document-imports-store.test.ts` — active set during a held
  invoke; cleared on success AND failure; note path returned; error
  rethrown; a concurrent second convert rejects without clobbering the
  first's `active`.
- `tests/import-progress.test.ts` — renders nothing when idle; shows
  filename/vault/elapsed when active; elapsed ticks under fake timers;
  `role="status"` present.
- `tests/import-vault-picker.test.ts` — converting state: card shown,
  vault rows gone, queued line still visible; existing flows unchanged.
- `tests/record-mode.test.ts` — import button replaced by the card
  while a conversion is active (including one started elsewhere).
- `tests/action-panel.test.ts` — list view shows the card while active,
  not when idle.

## Out of scope

- Backend progress events, cancellation, buddy-window indication.
- Any change to the sanctioned vault-write machinery or the ImportLock.
