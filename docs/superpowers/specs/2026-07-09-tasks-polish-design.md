# Tasks Polish Design — counter, archive, settings move, progress bar

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** Follow-up polish on the task-management feature (v0.5.0 + the
  recursive scan). Four related refinements to the tasks UX.

## Goals

1. **Open-task counter** on the vault-row ✓ Tasks button.
2. **Move the tasks-folder setting** out of the Tasks view into the per-vault
   settings view (relabeled **"Vault settings"**).
3. **`status: archived`** + a per-row **Archive** action that removes a task
   from the list.
4. **Progress bar** at the top of the Tasks view, where the folder setting was.

## Status model (the spine of this change)

A task's `status` frontmatter now has three surfaced values:

- **`new`** — open. Counts toward the open counter; unchecked in the list.
- **`done`** — completed. Checked + greyed in the list; not open.
- **`archived`** — removed from view. **Never surfaced anywhere in this slice**
  (no "show archived" view — a future concern). Excluded from the list, the
  counter, and the progress bar.

"Open" ≡ `status != "done" && status != "archived"` (so `new`, or any
hand-authored non-done/non-archived status, is open). Any other status a user
hand-authors renders as open, consistent with today.

## Core (`core/src/tasks.rs`)

- **`list_tasks` excludes archived.** In `collect_tasks`, after `is_task`, skip
  a file whose `status` is `archived`. Nothing in this slice shows archived
  tasks, so they shouldn't be returned; the list therefore contains only
  open + done tasks. (A future "show archived" feature adds a parameter.)
- **`set_task_status` takes a status string, not a bool.** Change the core
  signature from `set_task_status(root, path, done: bool)` to
  `set_task_status(root, path, new_status: &str)`; it applies
  `set_status(content, new_status)` (which already accepts any status string)
  and does the canonicalize+containment+atomic-replace exactly as today. No
  other write-path change.
- The open **count** reuses `list_tasks` at the command layer (below) — no new
  core walker; `list_tasks` already excludes archived, so open = the returned
  tasks whose `status != "done"`.

## Shell (`task_commands.rs`, registered in `lib.rs`)

- **`set_task_status(id, path, status: String)`** — was `done: bool`. Validate
  `status ∈ {"new","done","archived"}` (reject others with an inline error),
  then delegate to `tasks::set_task_status(&root, path, &status)`. Same
  root/vault resolution and canonical containment as today.
- **`count_open_tasks(id: String) -> usize`** — resolve the vault + tasks root
  like `list_tasks` (degrade to `0` on unknown vault / unsafe or missing
  folder / escape), then `tasks::list_tasks(root).iter().filter(|t| t.status !=
  "done").count()` (archived already excluded by `list_tasks`). Read-only.
- Register `count_open_tasks` in `generate_handler!`.

## Frontend

### `types.ts`
- No shape change (`TaskItem.status` is already a string). Add a `TaskStatus`
  string-union type (`"new" | "done" | "archived"`) for the command arg's
  clarity if convenient; not required.

### Vault-row open counter (`vaults` store + `VaultList.vue` + `ActionPanel.vue`)
- **`vaults` store:** add `taskCounts: Record<string, number>` state. In
  `refresh()` (after `loadVaults`), invoke `count_open_tasks` for each vault in
  parallel and populate `taskCounts` (best-effort; a failed/absent count is
  omitted → treated as 0). Cleared/replaced each refresh so it can't go stale.
  This runs on every panel open, the same cadence as discovery.
- **`ActionPanel.vue`:** pass `:task-counts="store.taskCounts"` to `<VaultList>`.
- **`VaultList.vue`:** accept a `taskCounts: Record<string, number>` prop; on
  the ✓ Tasks button, render a small count **badge only when the vault's open
  count is > 0** (mirrors the existing status-dot / pill styling). The badge is
  advisory; a missing count renders nothing.

### Tasks view (`Tasks.vue`)
- **Remove** the tasks-folder input + Save (moves to Vault settings).
- **Progress bar** at the top, hidden when there are no tasks. `total =
  tasks.length` (list already excludes archived), `done = tasks.filter(done)`;
  render a filled bar (`done/total` width) with a `done / total` label. Uses
  the already-fetched list — no extra IPC.
- **Checkbox** still toggles completion, but now sends a **status string**:
  checked → `set_task_status {status: "done"}`, unchecked → `{status: "new"}`
  (optimistic + per-row serialized, unchanged mechanism).
- **Archive button** per row (an archive/box icon): sets `{status:
  "archived"}` and **optimistically removes the row** from the local list
  (re-insert + toast on failure). Shares the per-row in-flight `toggling` guard
  (disable the row's checkbox + archive button while a write is pending), so a
  toggle and an archive for the same task can't race.
- Progress and the (implicit) counts recompute from the local list as rows
  toggle/archive, so the bar updates live.

### Vault settings (`CaptureSettings.vue`, view relabeled "Vault settings")
- **Relabel** the per-vault settings view title from "Capture settings" to
  **"Vault settings"** in `ActionPanel.vue`'s header ternary. The internal view
  id stays `captureSettings` and the component stays `CaptureSettings.vue`
  (relabel only — renaming the view id/emit/component would touch the store,
  ActionPanel, VaultList, and tests for no functional gain).
- Add a **Tasks** section with a tasks-folder input (placeholder `Tasks`),
  loaded via `get_tasks_config` and saved via `set_tasks_config` — the same
  commands the Tasks view used, just relocated. Saving is independent of the
  capture-config save (which already preserves `tasks_folder`).

## Testing

- **Core:** `list_tasks` omits an `archived` task (present alongside new/done);
  `set_task_status` writes `archived` (and still `new`/`done`); the escape/
  cycle guards are unaffected.
- **Shell contract** (via frontend `mockIPC`): `set_task_status` sends `status`;
  `count_open_tasks` is invoked per vault.
- **Vitest:**
  - `Tasks.vue`: progress bar shows the right fraction and hides at zero;
    checkbox sends `{status:"done"|"new"}`; Archive sends `{status:"archived"}`
    and removes the row; archive failure re-inserts + toasts; the folder input
    is gone.
  - `VaultList.vue`: the open-count badge shows when > 0 and is absent at 0.
  - `vaults` store: `refresh()` populates `taskCounts` from `count_open_tasks`.
  - `CaptureSettings.vue`: the tasks-folder field loads and saves via
    `get_tasks_config`/`set_tasks_config`; header reads "Vault settings".

## Out of scope

Un-archiving / a "show archived" view; per-status filters; drag-reorder; due
dates/priority (still deferred). Archived tasks remain valid `type: Task`
documents in the vault — only hidden from Vault Buddy's list.
