# Capture panel & document-import UX polish — design

Date: 2026-07-11
Status: approved (brainstorm)
Area: frontend (panel views, buddy window) + one new IPC command

## Why

Document import (Pandoc) was bolted onto a panel that was built for
audio recording, and the seams show. The record chooser is no longer
record-only, so its title and the mic icon's tooltip misdescribe it; the
"browse recordings" affordance out-competes the newer import action for
position; the vault-settings form drifted visually from the buddy-settings
page; and the Pandoc-not-installed path dumps the user at the bottom of a
long settings page with no explanation. Two smaller gaps: dropping a
document on the buddy gives no feedback that the drop registered, and a
finished import ends in a toast the user can't act on.

This increment is UX polish across those seams. No new vault write paths,
no new domains — one small read-only IPC command (open an already-imported
note in Obsidian) and otherwise frontend-only changes.

## Scope (seven items)

### 1. Rename the "Record" chooser title → "Capture knowledge"

`ActionPanel.vue` `VIEW_TITLES.recordMode`: `"Record"` → `"Capture
knowledge"`. Matches the mic tooltip (#2) and CONTEXT.md's ubiquitous
language, where a Capture is not necessarily audio.

### 2. Fix the mic icon tooltip

`VaultList.vue` mic button `title`: `"Capture knowledge (record audio)"` →
`"Capture knowledge"`. The `aria-label` is already "Capture knowledge in
{vault}"; this drops the now-inaccurate "record audio" qualifier.

### 3. "Browse recordings" becomes the last option

`RecordMode.vue` button order: Meeting → Voice Note → **Import Document** →
**Browse recordings**, then the transcription settings block. Browse moves
below Import.

### 4. Align vault settings with the buddy settings page

`CaptureSettings.vue` is a flat `<form>`; `BuddySettings.vue` groups
controls into uppercase-headed `rounded-xl border-white/10 bg-white/5`
cards. Regroup vault settings into matching carded sections — **Recording**
(folder + bitrate), **Companion note** (note + follow-up), **Audio
devices** (mic + desktop audio), **Transcription**, **Tasks**, **Document
import** — keeping the single Save button.

Invariant: purely presentational. Every `ref`, the `save()`/load logic,
every `data-testid`, and all field IDs stay identical so behavior and the
existing `capture-settings` tests hold. Only wrapper markup and classes
change.

### 5. Dedicated Pandoc view instead of redirecting into buried settings

Today a blocked Import (Pandoc missing/too old) calls `store.openSettings()`,
landing on the long Buddy-settings page with the Pandoc card last —
confusing. Replace with a focused view:

- New panel view `documentImport` in the `vaults` store `view` union.
- `ActionPanel.vue` renders `DocumentImportSettings.vue` for it, under the
  panel title "Document import" with a one-line intro so it reads as a
  standalone screen rather than a settings fragment.
- New store action `openDocumentImport()`; `back()` parent = the vault list
  (consistent with `search`/`transcriptions`; the codebase has no history
  stack, so a single fixed parent is required and the list is the only
  common ancestor of the two entry points).
- `RecordMode.vue` (blocked Import click) and `ImportVaultPicker.vue`
  ("Install Pandoc" button) route to `openDocumentImport()` instead of
  `openSettings()`.

The Pandoc card stays in Buddy settings too (out of scope to restructure
that page); `DocumentImportSettings.vue` is simply reused by the new view.

### 6. Buddy visual feedback on document drag-over

`BuddyRoot.vue` already registers `onDragDropEvent` but handles only
`drop`. Also handle `enter`/`over`/`leave` to drive a `dragActive` ref,
gated to supported docs (`docx/odt/rtf`) when the payload carries paths
(Tauri delivers paths on `enter`/`drop`; `over` carries only position, so
the enter-time verdict is held until `leave`/`drop`). A drop clears it, so
does `leave`.

A new `dropTarget?: boolean` prop on `CompanionCharacter.vue` applies a
highlight (glow ring + slight scale on the character box) while a droppable
file hovers. Gated so an unsupported drag (or a drag with no path info that
resolves to nothing) doesn't imply a drop will do something — matching the
existing drop handler, which ignores unsupported files.

### 7. Ask to open the converted note (success toast with "Open")

- `notifications.ts`: an optional `action { label, run }` on a
  `Notification`. Actionable toasts skip the dedupe-reuse (each carries its
  own callback) and linger until clicked or dismissed (default TTL `null`),
  so "Open" doesn't disappear on the 4s success timer.
- `NotificationHost.vue`: render the action button before the dismiss `✕`;
  clicking runs `action.run()` then dismisses the toast.
- New IPC command `open_imported_document(id, path)` (Rust): resolve the
  vault by id, accept the rel-or-abs note path `convert_document` returns
  (join with the vault root if relative), compute the vault-relative,
  extension-stripped path, and launch `obsidian://open` via the existing
  `uri::open_file_uri` + `uri::launch`. Read-only, logged — the same audit
  trail as `open_recording`. Registered in `lib.rs` `generate_handler`
  (52 → 53 commands).
- `RecordMode.vue` and `ImportVaultPicker.vue` replace their plain success
  toast with one carrying an **Open in Obsidian** action wired to the new
  command (the vault id and returned note path are both in scope at each
  call site).

## Testing

TDD per the repo convention. Touch or add:

- `record-mode.test.ts` — button order, blocked-import routes to the
  document-import view, success toast exposes an Open action.
- `import-vault-picker.test.ts` — blocked gate routes to the document-import
  view; success toast Open action.
- `capture-settings.test.ts` — unchanged behavior/testids after the carded
  restructure (regression guard).
- `buddy-root.test.ts` — drag `enter`/`leave` toggles the drop-target state;
  unsupported drag does not.
- `notifications-store.test.ts` — action stored, actionable toast not
  deduped, no auto-dismiss by default.
- `notification-host.test.ts` — action button renders, runs, and dismisses.
- `action-panel.test.ts` — `documentImport` title + view render.
- `vaults-store.test.ts` — `openDocumentImport()` sets the view; `back()`
  returns to the list.
- Rust unit test for `open_imported_document`'s URI building (rel and abs
  inputs, outside-vault rejection).

## Docs

Update `AGENTS.md`: the IPC surface table + command count (53), the
frontend-state view list (`documentImport`), and the document-import
domain note about the blocked-gate destination. `CONTEXT.md` unaffected
(no new terms). Check `docs/Gaps.md` for anything this closes or opens.

## Out of scope

- Restructuring the Buddy-settings page ordering.
- Any change to conversion mechanics, containment, recovery, or the
  never-clobber publish path.
- Threading the record-view origin through the Pandoc view for `back()`
  (fixed parent = list is accepted).
