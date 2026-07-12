# Settings Auto-Save & Tab Groups — Design

- **Date:** 2026-07-12
- **Status:** Approved (design only — no implementation yet)
- **Source:** User request to (1) auto-save changes in the settings views
  instead of requiring an explicit Save button, and (2) break the long,
  scrolling settings views into tab groups. Brainstormed end-to-end before
  any code is written; this spec documents the *why* behind the shape below.

## Goal

Make both panel settings views feel modern and effortless:

1. **Auto-save** — a change persists on its own; no Save button, no "did I
   save that?" doubt.
2. **Tab groups** — the sections that today stack into one long scroll become
   a small horizontal tab bar, so each screen shows one focused group.

Both requests reinforce each other: tabs split the view, and per-group
auto-save dissolves the single monolithic Save button that spanned all of
them.

## Current state (what we're changing)

Two settings views render inside the panel (`ActionPanel.vue`, keyed on the
`vaults` store `view`):

- **Buddy settings** (`view === "settings"`, `BuddySettings.vue`) —
  app-global. Sections today: Buddy character · Behavior · System (autostart)
  · Updates (`UpdateSettings`) · Diagnostics (`DiagnosticsSettings`) · MCP
  (`McpSettings`) · Document import (`DocumentImportSettings`).
- **Vault settings** (`view === "captureSettings"`, `CaptureSettings.vue`) —
  per-vault. Sections today: Recording (`RecordingSettings`) · Tasks (Tasks
  folder + `TaskListSettings`) · Documents (Documents folder + date-folders
  toggle).

Two save styles already coexist in the codebase:

- **Instant / auto-save (already present).** The Buddy-settings toggles write
  to localStorage on every change (via `settings` store actions); autostart
  saves optimistically on toggle; the whole **MCP** card saves on every
  `@change`, serialized behind a `saving` in-flight guard that disables its
  controls while a write is in flight.
- **Manual Save button (the pain point).** `CaptureSettings.vue` has one Save
  button that fans out to three independent config commands
  (`set_capture_config`, `set_tasks_config`, `set_documents_config`), and
  `TaskListSettings.vue` has its own Save button (`set_task_lists_config`).

Crucially, **much of the complexity in `CaptureSettings.vue` and
`useOptionalFolderField.ts` exists only because of the deferred Save button**:
the `loaded` / `edited` gates, the "the form is submittable before its loads
resolve" gymnastics, and the stashed `documentsLoadPromise` all defend against
a Save click that lands before an async load has hydrated a field (which would
persist a default seed over a real value — the class of bug GAP-02 also lives
in). Auto-save triggered by user edits removes that window entirely, so this
work is a net simplification, not just a UX change.

## Design decisions (locked during brainstorming)

| Question | Decision |
| --- | --- |
| Auto-save trigger for typed fields (folders, MCP port) | **On blur + debounce.** Save when focus leaves the field, and ~600 ms after typing stops. Toggles/checkboxes save immediately on click. |
| Save confirmation (Save button is gone) | **Transient status in the view header** — `Saving…` → `Saved ✓` fading after ~2 s, one indicator covering the whole view. |
| Buddy-settings tab grouping | **3 tabs:** Buddy (Character + Behavior) · System (autostart + Updates + Diagnostics) · Integrations (MCP + Document import). |
| Vault-settings tab grouping | **3 tabs:** Recording · Tasks · Documents (the existing three `group-*` sections). |
| Detailed field errors | Stay **inline** next to their field (e.g. "folder must be inside the vault"); the header shows a compact `⚠ Couldn't save`. |
| Selected tab across opens | **Resets to the first tab** on each mount (matches the panel's no-history philosophy). |

## Architecture

Approach **B** of three considered (see "Alternatives"): keep each settings
card **self-contained** (owning its own load/save — the established
`McpSettings` / `TaskListSettings` / `DocumentImportSettings` convention),
introduce a **reusable tab container** and a **shared auto-save composable**,
and route save status to a **tiny store** the header reads. Three small,
independently-testable new units, then targeted refactors of the existing
cards.

### New unit 1 — `TabGroup.vue` (reusable tab container)

A presentational, accessible tabbed container used by both settings views.

- **Props:** `tabs: { id: string; label: string }[]`, optional
  `initial?: string` (defaults to the first tab).
- **Renders:** a horizontal tab bar (text labels; the active tab underlined in
  the app's violet accent, matching existing focus/selection styling) and one
  panel per tab via a named slot (`<template #recording>` …). Every panel is
  **mounted, and only the active one is shown** (`v-show`), so each tab still
  owns its own self-contained load, a pending debounced save keeps running when
  you switch tabs (the component isn't torn down), and the tab content is
  present in the DOM for existing tests to read. (Chosen over `v-if`/lazy: the
  eager mount avoids a tab-switch unmount interrupting a queued save and avoids
  churning the many `BuddySettings` tests that read tab content directly; the
  per-tab load isolation the split gives us does not depend on lazy mounting.)
- **Accessibility:** `role="tablist"` / `role="tab"` / `role="tabpanel"`,
  `aria-selected`, `aria-controls`, roving `tabindex`, Left/Right (and
  Home/End) arrow-key navigation between tabs — the same keyboard rigor the
  search list and vault list already apply.
- **State:** selected tab is component-local, resets to `initial` on mount. No
  persistence (YAGNI; trivially made sticky later).
- **Fit:** three short text tabs per view fit comfortably in the 400 px panel;
  the bar is built to wrap/scroll gracefully if a future view adds more.

### New unit 2 — `useAutosave` composable

Wraps a caller-supplied async save fn with the mechanics every auto-saving
field needs, so no card re-implements them:

- **Debounced trigger.** `trigger()` schedules the save ~600 ms out;
  `flush()` runs it immediately (bound to `@blur` on inputs). Toggles call a
  path that saves immediately (no debounce).
- **In-flight serialization.** If a save is already running, a further
  `trigger()` does **not** race a second concurrent write — it coalesces into
  a single trailing run that re-reads the latest values after the current one
  resolves. This is the `McpSettings` `saving`-guard lesson, generalized: two
  quick edits can never land out of order or clobber each other.
- **Status + error reporting.** On start → `saving`; on success → `saved`; on
  failure → `error` with the message. Reports into the shared status store
  (below) and exposes a local `error` ref for the card's inline field error.
- **Lifecycle safety.** Flushes any pending debounced save on blur (`@focusout`
  on the tab container) and on `beforeUnmount` (the settings view navigating
  away/closing unmounts the tab component), so a queued write is never dropped.
  With `v-show` tabs a tab switch does not unmount, so its debounce simply keeps
  running; blur (clicking the tab blurs the focused input first) flushes it
  anyway, and the unmount flush is the belt-and-braces net for leaving settings.

The card supplies only the fn that builds its payload and invokes its command;
`useAutosave` owns the timing, serialization, and status.

### New unit 3 — `settingsStatus` store (tiny Pinia store)

`{ state: 'idle' | 'saving' | 'saved' | 'error', error: string | null }`,
following the existing store convention (`settings`, `vaults`, `capture`,
`updates`, `notifications` are all Pinia).

- `useAutosave` calls its `report(...)` actions.
- `saved` **auto-fades to `idle`** after ~2 s (a single timer, re-armed per
  report, cleared on a new `saving`).
- `error` is **sticky** until the next successful save (so a failure the user
  isn't looking at can't silently disappear).
- Reset to `idle` on settings-view change (ActionPanel already watches
  `view` / `shownNonce`).

### Header indicator (in `ActionPanel.vue`)

Next to the existing view `<h1>` title, render a compact indicator bound to
`settingsStatus`, shown only while `view` is a settings-family view
(`settings` | `captureSettings`): `Saving…`, `Saved ✓` (fading), or a
persistent `⚠ Couldn't save`. It occupies the spare space in the header row
between the title and the back button; it never grows the compact top bar.

## Per-view changes

### Buddy settings (`BuddySettings.vue`) — tabs only

Wrap the existing sections in `TabGroup` with three tabs:

- **Buddy** — Buddy character grid + Behavior card.
- **System** — Start-with-Windows + `UpdateSettings` + `DiagnosticsSettings`.
- **Integrations** — `McpSettings` + `DocumentImportSettings`.

**No auto-save work here.** These already persist instantly: localStorage
toggles (immediate, synchronous, never fail), optimistic autostart, MCP's own
serialized save, and the Pandoc path (saved on pick). Instant localStorage
toggles deliberately **do not** drive the header indicator — there is nothing
async to show and the control's own state is the confirmation. MCP and
Document import keep their existing save logic; optionally reporting into
`settingsStatus` for a consistent header signal is a low-priority nicety, not
required by this slice.

### Vault settings (`CaptureSettings.vue`) — tabs + auto-save (the real work)

`CaptureSettings.vue` (today the largest component, ~478 lines) becomes a thin
**shell**: it renders `TabGroup` over three extracted per-tab components. The
split both organizes the tabs and brings the oversized file back under its LOC
cap ("when a file grows large, it's doing too much").

- **Recording tab** (`RecordingConfigTab.vue`, new) — owns the
  `get_capture_config` + `list_audio_devices` load and hosts the existing
  controlled `RecordingSettings.vue` (unchanged, still `v-model` bundle). Any
  field change → `useAutosave` → `set_capture_config` with the whole struct
  (same payload the Save button sends today).
- **Tasks tab** (`TasksConfigTab.vue`, new) — Tasks folder → `set_tasks_config`
  via `useAutosave`; hosts `TaskListSettings.vue`, whose own Save button is
  likewise replaced by auto-save on change (`set_task_lists_config`). The
  lists-card-reload-on-folder-change concern (today's `listsCardNonce`)
  survives in simpler form: when the persisted tasks folder actually changes,
  remount the lists card so it reloads against the new root.
- **Documents tab** (`DocumentsConfigTab.vue`, new) — Documents folder +
  date-folders toggle → `set_documents_config` via `useAutosave`. The folder
  and the toggle both ride the one command, exactly as today.

Each tab component **loads its own config on mount and renders its form only
after the load resolves** (the existing `v-if="loading"` → "Loading…"
pattern); a **failed** load shows an inline error and renders no editable
fields. Because a save fires only from a user edit of an already-rendered,
already-loaded field, **there is no "save before load" window** — the whole
`loaded` / `edited` / `documentsLoadPromise` guard apparatus in
`CaptureSettings` + `useOptionalFolderField` collapses, and
`useOptionalFolderField` is deleted (each tab does a plain load + a plain
autosaved write inline).

Race-guards we still keep, and why:

- **In-flight serialization** per command (now inside `useAutosave`) — two
  quick edits must still not race the config write.
- **Never write a default seed.** Since a save only fires from a user edit,
  the value written is always the user's intent, not the mount seed — so the
  old `loaded && !edited` write-gate is unnecessary rather than merely
  simplified. (A tab that failed to load shows its load error and does not
  render editable fields, so there is nothing to auto-save from a failed
  load.)

## Invariants (each exists because a prior review found the failure)

- **A change is never silently dropped.** Debounced saves flush on blur and on
  unmount; the shared status makes an in-flight/failed save visible in the
  header; field-specific rejections stay inline. (Preserves the intent behind
  GAP-02 and the current "form state preserved on failure, so the user can
  retry" behavior.)
- **Two edits never race a config write.** `useAutosave` serializes with a
  single trailing coalesced run — the `McpSettings` lesson, generalized.
- **Auto-save cannot persist an unhydrated default.** Forms render only after
  their load resolves; saves fire only from user edits. No config command is
  ever sent from a mount seed.
- **The panel's compact header never grows.** The save indicator fits the
  existing header row and truncates/uses a short label rather than widening
  the 400 px bar (the same discipline as the task-count badge cap).
- **Tabs are keyboard- and screen-reader-navigable** to the standard the
  search/vault lists already meet.

## Testing (TDD, per repo convention — failing test first)

Vitest + happy-dom + `mockIPC`; no real Tauri runtime.

- **`TabGroup`** (new test) — renders tabs, switches active panel on click and
  arrow keys, lazily mounts only the active panel, resets to first tab on
  remount, ARIA roles/`aria-selected` correct.
- **`useAutosave`** (new test) — debounce timing (fake timers), blur flush,
  immediate path for toggles, in-flight coalescing (a trigger during a
  pending save yields exactly one trailing write with the latest values),
  unmount flush, status/error reporting.
- **`settingsStatus` store** (new test) — state transitions, `saved`
  auto-fade, sticky `error` cleared by next success, reset.
- **`capture-settings` / new tab-component tests** — each tab auto-saves its
  command on change (blur + debounce), does not save on mount, surfaces
  inline field errors, remounts the lists card when the tasks folder changes;
  the collapsed race-guards are covered by "does not save on mount" +
  "saves the edited value" rather than the old load-timing regressions.
- **`task-list-settings`** — Save button replaced by auto-save on default-list
  pick and on reorder; serialized.
- **`buddy-settings`** — sections now live under the correct tabs; switching
  tabs reveals the expected controls.
- **`action-panel`** — the header save indicator shows for settings views,
  cycles Saving→Saved, holds the error state, and is absent for non-settings
  views.

Update the LOC baseline (`scripts/loc-baseline.json`) — the `CaptureSettings`
split shrinks it — and the coverage floors / quality baseline as the new files
land, in the same PR (the shrink-only ratchet policy).

## Alternatives considered

- **A — Centralized `SettingsShell` with a provide/inject autosave context.**
  One component owns tabs + save orchestration + status; cards become dumb
  controlled forms. Cleanest in the abstract, but it fights the repo's
  deliberate "self-contained card owns its own load/save" convention and is a
  large, higher-risk diff touching every card at once. Rejected in favor of
  the incremental B.
- **C — Tabs only, status per-card.** Doesn't satisfy the "status in the
  header" decision and leaves the Save-button UX. Rejected.

## Out of scope (YAGNI)

- No generic/config-driven settings framework — only the three small units
  above.
- No per-open tab persistence (reset to first tab).
- No change to the Rust IPC surface — every command
  (`set_capture_config`, `set_tasks_config`, `set_task_lists_config`,
  `set_documents_config`, `set_mcp_config`, autostart, Pandoc path) already
  exists and is unchanged; this is a frontend-only slice.
- No redesign of individual controls; only their grouping (tabs) and their
  save trigger (auto vs button) change.
