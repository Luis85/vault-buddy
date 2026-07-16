# Tasks Polish — List Lifecycle, Copy-ID, Drag-to-Move, and Grouping Persistence

- **Date:** 2026-07-16
- **Status:** Approved
- **Source:** Follow-up polish on the
  [lists-first / drag-default / task-ids increment](2026-07-15-tasks-lists-first-drag-default-and-task-ids-design.md)
  (PR #59). Six candidate items were proposed; the user selected a scoped
  subset. This spec covers that subset as one cohesive increment.

## Goals & scope decisions

1. **List lifecycle** — rename, delete, and (reversible) archive a list, from
   the Lists view. Today lists can only be *created* and *moved into*.
2. **Copy-ID in the editor** — surface a task's generated id and let the user
   copy it, closing the loop on why the id exists (referencing tasks).
3. **Drag-to-move-between-lists** — extend the default drag so dropping a task
   on another list's section moves it there.
4. **Persist the grouping choice per view** — like the sort preference already
   does; grouping currently resets to Lists every open.
5. **No drag grips in the aggregate view** — resolves the GAP-63 rough edge
   (per-vault `order` ranks make cross-vault manual order meaningless).

Decided during brainstorming:

- **Delete** moves a list's tasks to *No list* (the tasks root), then removes
  the now-empty folder — non-destructive. (Not: refuse non-empty; not:
  hard-delete tasks.)
- **Archive** reversibly *hides the list* (config flag), leaving the folder and
  its tasks on disk. An archived list's tasks still appear under Dates/Tags
  grouping — archiving hides the **list**, not its tasks.
- **Drag is per-vault only.** Both reorder and the new drag-to-move work only
  in a single-vault view; the aggregate "All tasks" view shows no grips.
- **Task-ID polish = the copy affordance only.** Backfill and a configurable
  prefix were considered and **cut** for this increment (sequential IDs remain
  permanently out — they'd reintroduce cross-device collisions).

Non-goals: bulk backfill of existing tasks, ID format/prefix config, list
merge, multi-level list moves via drag, cross-vault manual ordering,
multi-select/bulk task operations.

## 1. List lifecycle

### 1.1 Core (`core/src/tasks/lists.rs`)

Two new sanctioned vault writes, on the same never-clobber machinery as
`create_task_list` / `move_task_to_list`:

- `rename_task_list(root: &Path, from: &str, to: &str) -> Result<(), String>`:
  - Validate `to` with `is_valid_list_name` (single non-dot segment, no
    separators). `from` is `normalize_list_rel`'d.
  - Canonicalize `root`; assert both `root/from` and `root/to` stay inside
    `root` (symlink/junction escape check) before AND after.
  - **Refuse if `root/to` already exists** (never clobber / never merge) — a
    rename onto an existing list is an inline error.
  - `std::fs::rename(root/from, root/to)` — moves the folder and every task
    inside it in one operation; the tasks' `list` changes as a side effect.
  - Rename is same-parent single-segment: `to` names the leaf; nested lists
    keep their parent. (A full relocate is out of scope.)
- `delete_task_list(root: &Path, list: &str) -> Result<DeleteOutcome, String>`
  where `DeleteOutcome { moved: usize, folder_removed: bool }`:
  - Canonicalize + containment-assert `root/list`.
  - Move every `type: Task` file **directly in** `root/list` (not recursing
    into sub-lists) to `root` (No list) via `move_task_to_list(root, file,
    "")`'s rails (`rename_noreplace` + ` (N)` suffix — never clobbers).
  - Then `std::fs::remove_dir(root/list)` — succeeds only if the folder is now
    empty. If it still holds nested sub-lists or **foreign (non-task) files**,
    `remove_dir` errors; catch it → `folder_removed: false`. Foreign files are
    **never moved or deleted** (vault is sacred); the caller reports "moved N
    tasks; the list folder wasn't empty and was kept."
  - Returns the outcome so the shell/UI can phrase the result.

### 1.2 Archive config (`core/src/vault_config.rs`)

Extend the lists settings object with `archived_lists: Vec<String>` (beside
`default_list` / `list_order`): the `/`-joined relative names of hidden lists.
Parsed per-field defensively (non-string entries dropped, trimmed, empties
filtered — the `list_order` precedent); serialized only when non-empty; JSON
key `archivedLists`. Preserved by `set_capture_config` (add to its
read-inside-the-lock preserve block).

### 1.3 Services + commands

- `services::rename_task_list(paths, id, from, to)` and
  `services::delete_task_list(paths, id, list)` — the vault-level root assert +
  `is_dir` guard wrappers around the core fns (the `create_task_list` /
  `move_task_to_list` precedent). Delete returns the outcome.
- Shell commands `rename_task_list(id, from, to)` and `delete_task_list(id,
  list)` (async, GAP-22 class — folder I/O off the main thread), registered in
  `lib.rs`.
- Archive/unarchive rides the **existing** `set_task_lists_config` command,
  now carrying `archived_lists` alongside `default_list`/`list_order`
  (write-strict: each name `normalize_list_rel`'d, empties dropped).
  `TasksConfigDto` / `get_tasks_config` gain `archived_lists`.

### 1.4 Frontend

- **List enumeration** stays complete; **archived filtering is a view concern.**
  `useTaskLists` exposes the archived set (from the vault's tasks config).
  - **Lists grouping:** `listSections` omits an archived list's section **and
    excludes its member tasks** from the Lists view (a task in an archived
    list has no Lists-grouping home — it is hidden *with* its list, not
    demoted to `No list`). Those tasks stay in the shared `tasks` model, so
    they still surface under Dates/Tags grouping — hiding the list, not the
    tasks globally. `listSections` therefore takes the archived set alongside
    `knownLists`/`listOrder`.
  - **Pickers** (composer + editor `TaskListPicker`) offer only non-archived
    lists (plus `No list`). Exception: the editor keeps a task's *own* current
    list selectable even if archived, so editing a task already in an archived
    list doesn't silently relocate it.
- **Section menu.** Each Lists-grouping section for a *real* list (not `No
  list`, not `Done`) renders a small ⋯ menu: **Rename** (inline input),
  **Archive**, **Delete** (with a confirm; the confirm names the move-to-No-list
  behavior). Rename/Delete call the new commands; Archive appends to the
  config's `archivedLists` via `set_task_lists_config`. `listSections` carries
  the raw list name on each bucket so the menu has it (buckets currently carry
  only `label`).
- **Archived-list management** lives in `TaskListSettings.vue` (Vault settings
  → Tasks): a list of archived lists with an Unarchive action (removes from
  `archivedLists`). Empty when none.
- After rename/delete, `Tasks.vue` reloads lists + tasks (paths changed).
  Actions are optimistic where safe, with revert + toast on failure, and
  serialized against the per-row/per-view busy guards already in place.

## 2. Copy-ID in the editor

- **Surface the id.** `core::tasks::list_tasks` gains an
  `id_property: Option<&str>` parameter; when `Some(prop)`, each `TaskItem`
  gets `id: Option<String>` = `scalar_field(content, prop)` (case-sensitive
  read of the configured property; a task stamped under a since-changed
  property casing simply shows no id — display-only, no correctness impact).
  `services::list_tasks` passes `cfg.task_id_enabled.then(|| cfg.task_id_property_name())`
  (None when IDs are off → id stays null, zero behavior change). `TaskDto` and
  the TS `TaskItem`/`AggTask` gain `id: string | null`.
- **Editor UI.** `TaskEditor.vue` renders the id read-only with a copy button
  (`navigator.clipboard.writeText`, a success toast) **only when
  `task.id` is non-null**. No new command — pure read + clipboard. (A "copy
  Obsidian link" is deferred; the raw id is the primitive.)

## 3. Drag-to-move-between-lists (per-vault, Lists grouping only)

- **Composable.** `useTaskReorder` already tracks a drag over rows within a
  section; extend it to also report the **section currently under the pointer**
  (`overSectionKey`) in its `dragState`, updated on pointer-move by
  hit-testing the rendered section containers (the `[data-reorder-section]`
  rows already exist; add a per-section drop zone keyed by bucket).
- **Decision in `Tasks.vue`.** On drop: if `grouping === 'lists'`, not
  aggregate, no active filter, AND the over-section is a **different `list:`
  section** than the origin → `move_task_to_list(task.vaultId, task.path,
  targetList)`; adopt the landed (possibly ` (N)`-suffixed) path, move the row
  into the target section, refresh counts. Otherwise → the existing
  within-section reorder.
- **Scope.** Cross-section move is Lists-grouping only (moving to a date bucket
  or a tag has no vault meaning). In Dates/Tags grouping, drag stays
  reorder-only within a section (unchanged). Per-vault only (see §5).
- Optimistic with revert + toast on a failed move; serialized by the existing
  per-path busy guard and the view-level reorder guard.

## 4. Persist the grouping choice per view

- Mirror `utils/taskSort.ts` exactly: a new `utils/taskGrouping.ts` with
  `loadGrouping(viewKey)` / `saveGrouping(viewKey, value)` over a
  `vault-buddy:task-grouping` localStorage object keyed by vault id / `"all"`,
  defaulting to `"lists"`, degrading a missing/corrupt entry to `"lists"` with
  a log (never a throw). `Tasks.vue`'s `grouping` ref initializes from it and
  saves on change. Values: `"dates" | "tags" | "lists"`.

## 5. No drag grips in the aggregate view (resolves GAP-63)

- `Tasks.vue`'s `reorderView` computed gains `&& !isAggregate`, so the
  aggregate "All tasks" view never shows grips — no reorder and (per §3) no
  drag-to-move, since per-vault `order` ranks make a merged cross-vault manual
  order meaningless. Update `docs/Gaps.md` GAP-63: mark the aggregate-reorder
  bullet resolved.

## Error handling

- Rename/delete validate + containment-check before any write; an invalid
  target, an escape, or a rename-onto-existing is an inline error, nothing is
  written. Delete never deletes a non-empty (foreign-holding) folder — it
  reports the partial outcome.
- Archived-list reads are lenient (a bad config entry defaults itself);
  writes are strict (`normalize_list_rel`). An archived name that no longer
  exists on disk is simply ignored by the filter (self-healing).
- Copy-ID: a clipboard write failure toasts; the id read never fails the list
  (best-effort `scalar_field`).
- All new writes ride the existing atomic/never-clobber rails; no new vault
  capability.

## Testing

TDD, failing test first. Rust (Linux):

- `lists`: rename (happy path moves the folder + tasks; refuses an existing
  target; rejects an invalid/escaping name; symlink-escape rejected), delete
  (moves direct tasks to root + removes an empty folder; keeps + reports a
  folder still holding a sub-list or a foreign file; never deletes a foreign
  file), archived-lists config round-trip + defensive parse + serialize-omit.
- `list_tasks` id read: `Some(prop)` populates `id`; `None` leaves it null; a
  custom property is honored.
- services/commands: rename/delete wrappers (root assert, missing-vault guard);
  `set_task_lists_config` preserves + round-trips `archivedLists`;
  `get_tasks_config` returns it; `set_capture_config` preserves it.

Frontend (Vitest + happy-dom + mockIPC):

- Section menu renders only for real lists (not No list / Done); Rename calls
  `rename_task_list`; Delete confirms then calls `delete_task_list`; Archive
  writes `archivedLists` and the section disappears from Lists grouping while
  the task still shows under Dates.
- `TaskListSettings`: archived lists listed; Unarchive removes the entry.
- Editor shows the id + copy button only when `task.id` is set; copy invokes
  the clipboard.
- Cross-list drag: dropping on a different list section calls
  `move_task_to_list` with the target; dropping within a section reorders;
  no move in Dates/Tags grouping or aggregate.
- Grouping persists per view (`loadGrouping`/`saveGrouping`), default lists.
- Aggregate view renders no drag grips.

Coverage floors, LOC baselines (`scripts/loc-baseline.json`), and the fallow
quality ratchet are re-run and ratcheted per the shrink-only discipline in
the same PR when a change legitimately grows them — and **`npm run check:loc`
is run on every gate pass** (it guards Rust files too).

## Docs

- `AGENTS.md` — tasks domain: the two new list writes (rename/delete + their
  never-clobber/move-to-root semantics), the `archivedLists` config field and
  archive-hides-the-list semantics, the id surfaced on `TaskItem`, drag-to-move,
  per-view grouping persistence, and aggregate-no-grips. Add
  `rename_task_list`/`delete_task_list` to the IPC surface table and bump the
  command count.
- `CONTEXT.md` — note that a List can now be archived (hidden) and that "delete
  a list" preserves its tasks (moves them to No list).
- `docs/Gaps.md` — resolve GAP-63's aggregate-reorder bullet; record any new
  residual (e.g. rename is single-segment/same-parent; delete keeps a
  foreign-holding folder).

## Rollout / compatibility

Additive and default-preserving: `archivedLists` defaults to empty (omitted
when empty), existing configs parse unchanged, the id surfacing is null when
IDs are off, grouping persistence defaults to today's `lists`, and the
aggregate-grips change only removes an interaction that produced meaningless
order. No migration; every new write rides the existing atomic rails.
