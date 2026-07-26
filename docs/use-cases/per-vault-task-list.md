---
type: UseCase
status: shipped
domain: task-management
shipped_in: v0.5.0
extended_in: [v0.5.1, v0.5.2, v0.5.3]
source_prd: "docs/prds/task-management.md"
related_specs:
  - "docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md"
  - "docs/superpowers/specs/2026-07-09-tasks-polish-design.md"
  - "docs/superpowers/specs/2026-07-09-recursive-tasks-scan-design.md"
  - "docs/superpowers/specs/2026-07-09-tasks-todo-list-design.md"
  - "docs/superpowers/specs/2026-07-09-task-tags-design.md"
  - "docs/superpowers/specs/2026-07-24-task-detail-surface-description-verbs-design.md"
  - "docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md"
tags: [use-case, task-management]
---

# Per-Vault Task List

> Every vault gets a todo list backed by `type: Task` Markdown documents in a
> configurable Tasks folder — add tasks with due dates, priorities and tags,
> check them off, edit them inline, archive them, group by date or tag, and
> open any task in Obsidian — with no Obsidian window required.

## Source

First vertical slice (and follow-up increments) of the
[Task Management capability PRD](../prds/task-management.md), whose Domain
Model, Task Model, and User Experience sections describe the full-featured
version (Quick Task Modal, lists, cross-vault aggregation). This use-case now
covers the complete **single-vault** experience; aggregation across vaults,
lists, and the dashboard remain with
[Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md).

## ~~⚠ PRD status is stale~~ — resolved

The staleness this note originally flagged has been fixed since: the Task
Management PRD's status line now narrates what shipped and what remains
unbuilt, and `AGENTS.md` documents the `task_commands::*` surface. Kept as a
struck-through record per this catalog's convention.

## Status: Shipped (v0.5.0, extended through v0.5.3, the lists increment, the do-date/planner-foundation increment, the task-detail increment, and the subtasks & parent-tasks increment)

- **v0.5.0** — the vertical slice: configure a per-vault tasks folder, list
  tasks, add a task, toggle completion.
- **v0.5.1** — polish: open-task counter badge on the vault row,
  `status: archived` + archive action, progress bar, recursive tasks-folder
  scan, tasks-folder setting moved into the Vault settings view.
- **v0.5.2** — the todo list: `due`/`priority` frontmatter, date buckets
  (Overdue / Today / Upcoming / No date / Done), inline row editor (rename,
  due, priority), click-to-open in Obsidian, title filter.
- **v0.5.3** — Obsidian-compatible `tags`: chips, click-to-filter, tags on
  add/edit, and a Dates | Tags grouping toggle.
- **Lists increment** — Lists as folders under the tasks folder: a
  `Dates | Tags | Lists` grouping toggle, list pickers on the composer
  (inline "New list…" creation) and the inline editor (moving the task's
  file between list folders), a per-vault default list + list order
  settings card, user-selectable sorting (persisted per view), and manual
  drag-to-reorder writing an `order` frontmatter rank.
- **Do-date/planner-foundation increment** — a Do Date (`scheduled`,
  distinct from Due Date) on every Task; the `Dates` tab relabeled **Plan**
  (grouping key unchanged), bucketing by the effective plan date (Do Date if
  set, else Due Date) into Overdue / Today / Upcoming / Anytime / Done; an
  additive do-date chip on each row; a per-row quick-schedule popover
  (Today / Tomorrow / This weekend / pick / Clear); a best-effort
  "Reschedule → Today" action on the whole Overdue bucket; and a Do Date
  field on both the add-task composer and the inline editor.
- **Task-detail increment** — the **Task Detail** surface: a plain click on a
  task's title now opens a full-height per-task home (Ctrl/⌘-click still opens
  the task in Obsidian), which shows and edits the title, an editable free-text
  `description` field, the Do Date / deadline, priority, tags, and list, and
  adds the first per-task lifecycle verbs — Open in Obsidian, **Duplicate** (a
  faithful, collision-safe copy that resets only identity), and a permanent
  **Delete** behind a hardened two-step confirm (the app's first destructive
  vault write).
- **Subtasks & parent-tasks increment** — a Task can now name a **Parent
  Task**: a hybrid reference (the parent's stable Task ID, authoritative for
  resolution, plus an Obsidian link for click-through/Dataview), managed from
  the Task Detail surface's new **Parent row** (current parent as a
  clickable chip, Change/Clear through a cycle- and ambiguous-id-aware
  picker) and **Subtasks section** (a done/total progress line, each child
  as a compact row, and Add Subtask). Setting a vault's first Parent Task
  turns on Task IDs for it automatically. The main list gets a light touch
  only — an open-subtask-count badge on a parent and a parent chip on a
  child — with no nested tree or collapse/expand.

## Implementation

- `core::tasks` (pure, unit-tested): `type: Task` frontmatter identity
  (closed-fence `is_task`), `render_task`/`create_task` (title, created,
  optional due/priority/tags), `list_tasks` (recursive, clock-free sort:
  open → due asc → priority → newest created; archived excluded),
  `note_tags` (frontmatter tags in every Obsidian form), and `set_fields` —
  the generalized surgical multi-key frontmatter writer behind every edit
  (byte-preserving; consumes block-style lists on rewrite/removal).
- Sanctioned vault writes, mirroring the transcript sidecar's never-clobber
  discipline: collision-safe create (optionally carrying an already-resolved
  `parent-id`/`parent` pair for a Subtask); the surgical field write
  (`update_task_fields`: canonical containment + atomic replacing rename —
  also what sets/clears a Task's `parent-id`/`parent` pair, not a new write
  capability); a faithful collision-safe **duplicate** (`duplicate_task` —
  copies the bytes and resets only identity, keeping the source's Parent
  Task so the copy lands as a sibling); a task-file move
  (`move_task_to_list` — also recomposes the landed file's own stale
  markdown-fallback `parent` link when it is itself a child); and a
  permanent **delete** (`delete_task` — the app's first destructive vault
  write: canonical containment + `type: Task` re-validation + file-identity
  re-check at unlink time + a no-follow symlink refusal; a deleted parent's
  children become orphans rather than being cascade-deleted or repaired).
- The `description` field: an optional free-text `description:` frontmatter
  property, written as a single escaped YAML scalar the app round-trips
  exactly (`core::yaml_scalar`) and read leniently by
  `core::tasks::description_field` (decodes each single-line scalar form as
  Obsidian's js-yaml does; a block or flow value degrades to none rather than
  corrupting the frontmatter). Reserved in both task key-sets so it can never
  be smuggled in as a template key nor configured as the Task ID property.
- The Parent Task / Subtask hierarchy: `parent-id` (the parent's stable Task
  ID, authoritative — read via a strict optional-field decode, not the
  lenient title decode, since a wrong reference is worse than none) plus
  `parent` (an Obsidian link, wikilink by default with a percent-encoded
  markdown fallback for a List name carrying a wikilink metacharacter),
  both additive and reserved like `description`. `services::tasks::parent`
  validates (containment, self-parent, ambiguous ids, cycles — the last two
  against a STRICT structural scan that includes archived Tasks and refuses
  on any unreadable file, never a lenient view) before enabling Task IDs
  (the vault's first Parent Task turns them on) and writing the pair — see
  AGENTS.md's tasks-domain section for the full phase ordering and the
  ID-configuration lock that follows once a hierarchy exists.
- Config: `tasks_folder` on `VaultCaptureConfig`, default `"Tasks"`, edited
  in the per-vault Vault settings view.
- IPC: `get_tasks_config`, `set_tasks_config`, `list_tasks` (rows now carry
  `description` and `parentId`/`parentLink`; takes an `include_archived` flag
  the hierarchy resolution opts into), `add_task` (optional `parent_path`,
  returning `idsEnabled`), `set_task_status`, `count_open_tasks`,
  `open_task`, `update_task` (patch now carries `description` /
  `clearDescription` and `parentPath`/`clearParent`, returning
  `parentId`/`parentLink`/`idsEnabled` alongside the task's own id),
  `delete_task`, `duplicate_task` (`src-tauri/src/task_commands.rs`); the
  five per-vault Tasks-settings commands, including `set_task_id_config`
  (now refusing an ID re-point/disable while a hierarchy exists), live in
  the sibling `src-tauri/src/task_config_commands.rs`.
- Frontend: `Tasks.vue` (self-contained, no dedicated Pinia store), reached
  via the Tasks button on each vault row (which carries the open-task
  badge); `vaults` store holds `view: 'tasks'` / `tasksVaultId` /
  `openTasks()` and the per-vault counts. The **Task Detail** surface is
  `TaskDetail.vue`, driven by the `useTaskDetail` composable
  (save / delete / duplicate / open under one shared busy guard) and, for
  the hierarchy, `useTaskHierarchy` (parent / children / progress / set)
  plus `TaskParentRow.vue`, `TaskParentPicker.vue`, and `TaskSubtasks.vue`;
  a title click routes through `useTaskActions`' `onOpenTask` (plain →
  detail, Ctrl/⌘ → Obsidian), and the store adds `view: 'taskDetail'` +
  `taskDetailTask` + `openTaskDetail()`.

## Explicitly out of scope (single-vault list)

Task lists (Inbox/Next/Today/etc. as metadata), project and estimated-effort
fields, the cross-vault aggregated dashboard, Task
Tags on non-Task notes, inline-Todo scanning, the standalone
Quick Task modal, un-archiving / a show-archived
view, recurring tasks and notifications — see
[Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md),
[Task Tags & Todos](task-tags-and-todos.md), and
[AI-Assisted Task Management](ai-assisted-task-management.md).

## Related use-cases

- [Vault Discovery, Listing & Opening](vault-discovery-and-open.md)
- [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md) (planned)
- [Task Tags & Todos](task-tags-and-todos.md) (planned)
