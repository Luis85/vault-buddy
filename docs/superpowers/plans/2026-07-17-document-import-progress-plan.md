# Document import: visible in-progress state — implementation plan

Executes: `docs/superpowers/specs/2026-07-17-document-import-progress-design.md`

TDD throughout — each step lands its failing tests first, then the code.

## Task 1 — `documentImports` store

- `tests/document-imports-store.test.ts`: active set while `convert_document`
  is held pending; cleared on success and on failure; note path returned;
  raw IPC error rethrown; second concurrent `convert` rejects with the
  ImportLock message without touching the first run's `active`.
- `src/stores/documentImports.ts`: `active` slot + `convert(vault, sourcePath)`
  per the spec (pre-check → set → invoke → clear in `finally`).

## Task 2 — `ImportProgress` card

- `tests/import-progress.test.ts`: renders nothing when idle; filename,
  vault name, elapsed shown when active; elapsed ticks under fake timers;
  `role="status"` present; indeterminate bar element present.
- `src/components/ImportProgress.vue`: sky-accented card, spinner, 1 s
  elapsed tick (`formatDuration`), scoped-keyframes sliding bar with a
  `prefers-reduced-motion` static fallback, subtitle naming the vault.

## Task 3 — wire the surfaces

- `tests/record-mode.test.ts`: while a conversion is active the Import
  Document button is replaced by the card (also when started elsewhere).
- `RecordMode.vue`: drop the local `importing` ref; `importDocument` runs
  through `documentImports.convert`; button `v-if`-swapped for the card.
- `tests/import-vault-picker.test.ts`: converting state shows the card,
  hides the vault rows, keeps the queued line.
- `ImportVaultPicker.vue`: drop `busyVaultId`; `viewState` gains
  `"converting"` (first branch) rendering the card instead of the list.
- `tests/action-panel.test.ts`: list view shows the card while an import
  is active, nothing when idle.
- `ActionPanel.vue`: render the card on the list view beside
  RecordingBar/TranscriptionSummary.

## Task 4 — docs + gates

- AGENTS.md: amend the document-import frontend paragraph (progress store
  + card, converting states of the two surfaces, list-view visibility).
- `npm run lint && npm run check:loc && npm run check:quality &&
  npm run test:coverage` and `npm run build` — all green, no baseline
  loosening expected (new files are small; touched files stay under caps).
