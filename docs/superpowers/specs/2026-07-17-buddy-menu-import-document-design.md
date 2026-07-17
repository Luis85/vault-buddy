# Buddy-menu "Import document…" (vault-first intake) — design

Date: 2026-07-17
Status: accepted (autonomous session — user request: "an 'add document'
option in the right-click menu; using it opens a vault picker, and after
the target vault is chosen, the file picker opens")

## Problem

Document import has two entry points today — dragging a file onto the
buddy, and the per-vault record chooser's Import Document action. Both
start from the FILE (a drop) or from the VAULT (a chooser already scoped
to one vault). There is no entry point from the buddy's right-click menu,
and no vault-first flow: "I want to add a document somewhere" currently
requires either locating the file first (drop) or navigating
list → vault → Capture knowledge → Import.

Requested flow: right-click menu option → vault picker → (vault chosen) →
OS file picker → convert. The conversion then reuses the ImportProgress
working-state machinery landed earlier today.

## Design

### Menu item (Rust, `commands.rs::show_buddy_menu`)

A new first item `Import document…` (id `buddy-import-document`) in the
buddy's right-click popup, above the Animation/Dragging toggles — it is
the menu's only *action* on the vault, so it leads; the ellipsis signals
more UI follows. Label uses the domain's existing verb ("Import
document", matching the panel view title and RecordMode's action) rather
than the request's literal "add document" — CONTEXT.md's ubiquitous
language; a one-string change if the product owner prefers otherwise.
Scope: the buddy context menu only (the tray menu stays
recording-focused).

### Routing (Rust, `document_commands.rs` + `lib.rs`)

The menu handler (`lib.rs` `on_menu_event`, main thread) calls
`document_commands::begin_add_document(app)`: set a pending flag, then
`commands::show_panel(app)` — exactly `begin_document_import`'s shape
minus the path (there is no file yet; it is picked AFTER the vault).
The flag is Rust-owned state (`AddDocumentPending`, an `AtomicBool`
managed beside `DocumentImportPending`) drained by a new one-shot command
`take_add_document_request` inside the panel's `refresh()` — the
established race-free pattern: the buddy and panel webviews have separate
Pinia stores, and an emitted event would race the panel-shown refresh's
list default.

`refresh()` drain order: dropped paths (`take_pending_import`) still win —
both land on the same `importPicker` view, and a drop carries more
information (a concrete file). The add request is consumed in the same
drain so it can't fire stale on a later reopen.

### Vault-first mode (frontend, `ImportVaultPicker.vue`)

The `importPicker` view gains a second mode instead of a second view: an
EMPTY `pendingImports` queue (previously unreachable — the picker only
opened via `enqueueImports`) now means "vault first, file after":

- Header reads "Import a document into which vault?" (no filename yet).
- Picking a vault opens the OS file picker (`tauri-plugin-dialog` `open`,
  docx/odt/rtf filter, wrapped in `withDialogSuppressed` so the panel's
  focus-out auto-hide stays quiet — the DIALOG_ACTIVE mechanism built for
  the existing pickers). Cancel = stay on the picker, no toast, no
  navigation (the user may pick another vault or back out).
- A chosen file converts through `documentImports.convert` — the
  ImportProgress card, Pandoc gate, error toast, and success toast
  ("Open in Obsidian") all apply unchanged.
- On success the picker returns to the list, guarded by the SAME
  `importEpoch` check the drop flow's `dequeueImport` uses: a back-out
  (which bumps the epoch) means the completion no longer owns navigation.

The existing drop mode (non-empty queue) is untouched, including the
GAP-55 queue semantics. Pandoc checking/blocked/empty gates apply to both
modes — they sit above the mode split in `viewState`.

## Alternatives considered

- **A dedicated new view/component** for the vault-first picker: rejected
  — it would duplicate the Pandoc gate, vault list, converting state, and
  navigation of `ImportVaultPicker` for a flow that differs only in where
  the file comes from.
- **Emit an event to the panel + show it**: rejected — races the
  panel-shown refresh (the reason `begin_document_import` is a stash, per
  its own comment).
- **Menu item routes to RecordMode's Import for a picked vault**:
  rejected — lands the user in the full capture chooser when they asked
  for a two-step add-document flow.

## Testing

- Frontend (Vitest): vaults-store refresh drains the add request onto the
  picker (and drops win over it); picker add-mode — header, pick → file
  dialog → convert args, cancel stays put, success returns to the list,
  converting shows the card. The picker test file gains a
  `@tauri-apps/plugin-dialog` module mock (it had no dialog use before).
- Rust: `take_add_document_request` is a trivial AtomicBool swap; the
  shell change is covered by the Linux compile gate
  (`npx tauri build --no-bundle`) + `cargo fmt --check`, with CI's
  windows-app as the behavior gate — same posture as the existing menu
  items.

## Out of scope

- A tray-menu counterpart; drag-drop or multi-file selection in the
  vault-first flow (the OS picker is single-file, matching RecordMode).
