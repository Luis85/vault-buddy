# Task Lists, Sorting & Manual Ordering Design — lists as folders, a sort selector, drag-to-reorder, and the SelectMenu scroll fix

- **Date:** 2026-07-11
- **Status:** Approved
- **Source:** Second sub-project of the
  [Aggregated Task Dashboard & Lists](../../use-cases/aggregated-task-dashboard-and-lists.md)
  use case, plus a user-reported SelectMenu bug. Three workstreams: (1)
  user-defined task Lists, reflected as folders inside the vault with a
  per-vault settings object in the buddy's config; (2) user-controlled
  sorting plus manual drag-to-reorder in the task views; (3) the "All
  tasks" vault picker closing on scroll. **This spec amends the Task
  Management PRD**: the PRD drafted lists as "metadata rather than physical
  folders"; the product decision is now the opposite — a List IS a folder —
  and the PRD/use-case text is updated as part of this increment.

## Goals

1. **Lists are folders.** A List is a folder under the vault's tasks
   folder. Obsidian users see and organize them natively; the buddy never
   invents a parallel taxonomy.
2. **A per-vault lists settings object in the buddy** (`config.json`):
   the default list for new tasks and the display order of lists — never
   list existence, which only the filesystem defines.
3. **Sort the task views**: a sort selector (Default / Due date /
   Priority / Created / Title / Manual) with an asc/desc toggle, persisted
   per view.
4. **Manual ordering**: drag a task to reorder it; the position persists
   in the task file itself (an `order` frontmatter number) so it is
   Obsidian-Properties/Dataview-visible and survives the buddy.
5. **Fix SelectMenu's close-on-scroll**: scrolling the picker's own
   overflowing option list must not close it (today it does — with enough
   vaults the lower options are unreachable), and scrolling the panel
   behind it should track, not dismiss.

## Domain model

- **List** — a folder under a vault's tasks folder. Identity = its
  vault-relative path from the tasks root, `/`-separated regardless of
  platform (`Projects/Alpha` for a nested folder). The tasks root itself
  is "no list" (`list: ""`), rendered as **No list** (the "No date" /
  "No tags" precedent). Read-lenient: ANY folder counts, hand-created and
  nested alike — the same philosophy that makes a hand-authored
  `type: Task` file a task. Write-strict: the buddy only *creates*
  single-segment list folders (no `/`), mirroring the tags read/write
  posture.
- **Order** — an optional `order` frontmatter number on a Task giving its
  manual rank (ascending). Absent on new tasks (files stay minimal, like
  `priority: normal` is never written); assigned on first drag. Lenient
  read (unparseable → unranked), written as a plain YAML number.

## Core crate (`core/src/tasks/`)

- **`TaskItem` gains `list: String` and `order: Option<f64>`.** `list` is
  derived during the existing recursive walk — the walk hands canonical
  paths under the canonical root, so the parent dir's `strip_prefix` of
  the root, joined with `/`, is free (no extra I/O). `order` parses via
  `scalar_field` + `f64` parse; non-finite or unparseable → `None`.
- **`task_lists(root) -> Vec<String>`** enumerates list folders: every
  directory under the canonical root, walked with the exact `vault_walk`
  discipline (canonicalize-and-contain before descending, walked-set
  cycle guard, dot-dir skip, name order → deterministic), returning
  relative `/`-joined paths — including EMPTY folders, so a just-created
  list appears before its first task. Missing/unresolvable root → empty.
  (GAP-17's `tasks_folder: "."` trap applies here too, but a dirs-only
  scan reads no file content — the gap's entry is extended, not worsened.)
- **`create_task_list(root, name)`** — validated single segment (trimmed,
  non-empty, no `/` or `\`, no leading `.` — the walk skips dot-dirs, so a
  `.foo` list would be invisible; not `.`/`..`), then `create_dir_all`
  with containment asserted before AND after creation (the
  document-import discipline: pre-create stops following a planted
  symlink/junction, post-create closes the swap-in race). OS-invalid
  characters surface the create error verbatim.
- **`move_task_to_list(root, path, list) -> new PathBuf`** — the tasks
  domain's file move: canonicalize root + source and require containment
  (same as `update_task_fields`), lexically validate the target list path
  (multi-segment allowed — existing nested lists are real targets),
  `create_dir_all` the target folder (a list deleted between fetch and
  move is recreated — lists are folders) with the same pre/post asserts,
  then `rename_noreplace` keeping the basename, with the ` (N)` suffix
  retry on collision — never clobbers. Moving a task to the list it is
  already in is a no-op `Ok`.
- **`create_task` gains the target list**: the file is created in
  `root/<list>` (validated + contained the same way). `render_task` is
  unchanged — no `order`, no list frontmatter (the folder IS the list).
- **`order` writes ride `set_fields`** — it is just another frontmatter
  key to the surgical writer; no writer changes.

These extend the tasks domain's sanctioned writes (AGENTS.md write paths
#3/#4): list-folder create and the in-tasks-root task move, both on the
existing never-clobber machinery.

## Config (`core/src/capture_config.rs`)

`VaultCaptureConfig` gains two fields, parsed per-field defensively and
round-tripped by `serialize_config` (both regression-tested — the
config-clobber failure mode is already catalogued):

- `default_list: Option<String>` (`defaultList`) — where unpicked new
  tasks land; `None`/empty = the tasks root. Honored by the panel
  composer's initial pick AND by `services::add_task` when no list is
  passed, so MCP adds follow it too.
- `list_order: Vec<String>` (`listOrder`) — display order for list
  sections and pickers; folders not named append alphabetically; names
  without folders are ignored (folders are truth). Non-string array
  entries are dropped at parse.

`set_capture_config` preserves both (it already preserves
`tasks_folder`); `TasksConfigDto`/`get_tasks_config`/`set_tasks_config`
widen to carry them, with `set_tasks_config` validating `default_list`
lexically against the vault (safe join, containment class) before saving.

## IPC surface (55 → 59)

| Command | Shape |
| --- | --- |
| `list_task_lists(id)` *(async)* | `Vec<String>` — best-effort empty on unknown vault/unsafe root, mirrors `list_tasks`. Read-only. |
| `create_task_list(id, name)` *(async)* | `Result<String>` — the created list's relative path. |
| `move_task_to_list(id, path, list)` *(async)* | `Result<String>` — the landed absolute path (may carry a collision suffix). |
| `set_task_lists_config(id, default_list, list_order)` *(async)* | `Result<()>` — persists the lists settings object; preserves `tasks_folder` (and everything else) under `ConfigWriteLock`. |

Widened existing commands: `add_task` gains `list: Option<String>`
(`None` → config `default_list`; explicit `""` → root); `update_task`'s
patch gains `order: Option<f64>` (validated finite; nothing un-ranks a
task this slice, so no clear flag); `get_tasks_config` carries
`defaultList`/`listOrder`. `set_tasks_config` keeps its single-field
shape (the folder save site is a generic one-field helper) and now
PRESERVES the two new fields on its read-modify-write, exactly as
`set_capture_config` preserves `tasks_folder` — the settings-object
write is its own command so a lists-config failure can't block the
folder save and vice versa (the CaptureSettings precedent). `TaskDto`
gains `list` and `order` — additive for the frontend and the MCP
`list_tasks` tool alike. All writes are async on the blocking pool
(fsync'd vault I/O, GAP-22 class); the MCP tool surface gains NO new
tools or params this slice.

## Frontend

### SelectMenu scroll fix (`SelectMenu.vue`)

The capture-phase `window` scroll listener closes the menu on ANY scroll
today — including the popup's own `overflow-y-auto` option list. Fix:
scroll events originating inside the popup are ignored; scrolls outside
(the panel content behind) **reposition** the popup via the existing
`positionPopup` so it tracks its trigger instead of dismissing. Resize
keeps closing (the panel windows are fixed-size; harmless). Regression
tests pin both behaviors. Fixes every SelectMenu consumer at once.

### Sort selector + persistence

- A sort control joins the grouping-toggle row: a compact `SelectMenu`
  (Default / Due date / Priority / Created / Title / Manual) plus an
  asc/desc icon toggle. Direction is disabled for Default and Manual;
  each key has a natural default direction (due ↑, priority ↑ high-first,
  created ↓ newest-first, title ↑).
- Sorting applies **within sections** — date buckets, tag sections, list
  sections, and the Done section keep their grouping; only row order
  inside each changes. Default = today's comparator, bit-identical.
- Persisted in localStorage (`vault-buddy:task-sort`), a defensive-parsed
  map keyed per view (`"all"` / vault id) — the `recentSearches` pattern;
  pure-UI preference, deliberately NOT in `config.json`.
- Pure helpers live in `src/utils/taskSort.ts` (comparator factory +
  storage), unit-tested.

### Manual ordering (drag-to-reorder)

- Active when sort = **Manual**: ranked tasks first (by `order`
  ascending), unranked after them in Default-comparator order.
- Each row shows a grip handle **only in Manual sort with no active
  title/tag filter** (reordering a filtered subset writes ranks against
  invisible neighbors — disabled like most task apps). Reordering is
  within-section only.
- **Pointer-based drag** (Tauri's drag-drop interception breaks HTML5
  DnD in the webviews): pointerdown on the handle captures the pointer,
  pointermove tracks the target slot with a drop indicator, pointerup
  commits, Escape cancels. **Keyboard alternative**: the handle is
  focusable; ArrowUp/ArrowDown move the row one slot with the same rank
  math.
- **Rank math** (`src/utils/taskOrder.ts`, unit-tested):
  `rankBetween(before?, after?)` = midpoint, or ±1024 past the end; when
  neighbors leave no representable gap or are unranked in the way, the
  section **materializes** — every task in the section gets a spaced
  (1024-step) rank in its current visual order, written as serialized
  `update_task` calls behind a view-level reordering guard. Rare by
  construction after first materialization.
- A drop writes the dragged task's own file (`update_task` with
  `order`), optimistic with revert + toast, via the existing per-path
  busy guard. Aggregate mode: ranks interleave across vaults (the
  existing `vaultName`/`path` tiebreaks keep equal ranks stable); a drop
  still writes only the dragged task's file.

### Lists UI

- **Grouping toggle** becomes `Dates | Tags | Lists`. Lists mode:
  one section per list — `listOrder` first, remaining lists
  alphabetical, then **No list**, then **Done** (headers always render,
  like tag mode). Aggregate merges sections by case-insensitive list
  name (first-seen casing wins — the tags precedent), rows keep their
  vault chips. Empty lists render as empty sections in per-vault mode so
  a fresh list is visible; the aggregate view skips empty sections
  (cross-vault noise).
- **Composer** options row gains a List picker (`TaskListPicker.vue`,
  shared with the editor: No list + lists in display order + **New
  list…**, which swaps to an inline name input → `create_task_list` →
  selects the new list). Defaults to the vault's `default_list`;
  aggregate mode re-fetches lists/config when the picked vault changes
  (lazy, cached per vault). `add()` passes the picked list; the returned
  `TaskDto` (now carrying `list`) merges into the view as today.
- **Inline editor** gains the same picker seeded with the row's list
  (existing lists only — creation lives in the composer's flow; routing a
  create round-trip through the presentational editor would add a second
  signalling channel for no real gain); on save, field changes go first
  (one `update_task` at the old path), then a changed list issues
  `move_task_to_list` and the row re-keys to the returned path. A failed
  move after a successful patch keeps the patch and raises a toast naming
  the move — never silently half-applied.
- **Per-vault Vault settings** (CaptureSettings' tasks section) gains
  the settings object's UI: a Default-list picker and a list-order
  editor (the vault's lists with up/down controls, persisted via
  `set_tasks_config`).
- Rows do NOT grow a list chip this slice (aggregate rows already carry
  vault chip + tags + due; the editor and Lists grouping expose
  membership).

### Aggregate fan-out

`list_task_lists` joins the existing per-vault fan-out (parallel,
best-effort — a failed vault contributes no lists and is logged; the
tasks toast already names broken vaults). Per-vault mode issues one call.

## Error handling summary

- Every new write is optimistic-with-revert + toast, serialized through
  the per-path busy guard; materialization uses a view-level guard.
- Move collisions land with a ` (N)` suffix (never clobber); the UI
  adopts the returned path.
- List-name validation errors are inline (write-strict, naming the rule);
  enumeration/read paths degrade to empty (read-lenient).
- Editor patch-then-move partial failure: patch kept, move named in the
  toast (documented above).

## Testing

- **Core:** `list` derivation (root, nested, Windows separators via the
  canonical-path strip), `task_lists` enumeration (empty folders, dot-dir
  skip, symlink escape, cycle), `create_task_list` validation + pre/post
  containment, `move_task_to_list` (containment, collision suffix, no-op
  same-list, recreate-deleted-list), `create_task` into a list, `order`
  parse (absent/int/float/garbage) and write round-trip, config
  parse/serialize round-trip + `set_capture_config` preservation of the
  new fields.
- **Shell:** command validation gates (list names, finite order, list
  containment), async signatures.
- **Vitest:** SelectMenu scroll regression (inside-scroll stays open,
  outside-scroll repositions, pointerdown-outside still closes), sort
  selector per key/direction + persistence round-trip, Manual mode
  ordering (ranked/unranked split), rank math (midpoint, ends, no-gap →
  materialize), pointer + keyboard reorder writing the right patch,
  Lists grouping sections + ordering + aggregate merge, composer picker
  default/vault-follow/new-list flow, editor move flow (re-key on landed
  path, partial-failure toast), settings editor save shape.
- All existing tests pass unchanged — Default sort is bit-identical and
  per-vault behavior without lists config is unchanged.

## Docs

AGENTS.md (IPC table 55→59, tasks-domain section, sanctioned-writes
list, frontend-state notes), CONTEXT.md (List, Order terms), DEVELOPMENT.md
(config field reference), the Task Management PRD + aggregated use-case
(the folders decision replaces "metadata rather than physical folders";
roadmap ticks), Gaps.md (SelectMenu scroll entry marked fixed; GAP-17
extended to list enumeration; any accepted residuals from this slice).

## Out of scope (YAGNI)

List rename/delete (Obsidian file operations), bulk move, drag across
sections (a bucket-to-bucket drag implying a due-date rewrite), per-list
sort overrides, persisting the grouping toggle, list chips on rows, MCP
move/create-list tools or an MCP `list` param on `add_task` (the DTO
field + default-list honoring are the whole MCP delta), Task-tagged
notes, the dashboard, the Quick Task modal, full-text task search.
