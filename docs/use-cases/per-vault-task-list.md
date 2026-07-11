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

## Status: Shipped (v0.5.0, extended through v0.5.3 and the lists increment)

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

## Implementation

- `core::tasks` (pure, unit-tested): `type: Task` frontmatter identity
  (closed-fence `is_task`), `render_task`/`create_task` (title, created,
  optional due/priority/tags), `list_tasks` (recursive, clock-free sort:
  open → due asc → priority → newest created; archived excluded),
  `note_tags` (frontmatter tags in every Obsidian form), and `set_fields` —
  the generalized surgical multi-key frontmatter writer behind every edit
  (byte-preserving; consumes block-style lists on rewrite/removal).
- Two sanctioned vault writes, mirroring the transcript sidecar's
  never-clobber discipline: collision-safe create and the surgical
  field write (`update_task_fields`: canonical containment + atomic
  replacing rename).
- Config: `tasks_folder` on `VaultCaptureConfig`, default `"Tasks"`, edited
  in the per-vault Vault settings view.
- IPC: `get_tasks_config`, `set_tasks_config`, `list_tasks`, `add_task`,
  `set_task_status`, `count_open_tasks`, `open_task`, `update_task`
  (`src-tauri/src/task_commands.rs`).
- Frontend: `Tasks.vue` (self-contained, no dedicated Pinia store), reached
  via the Tasks button on each vault row (which carries the open-task
  badge); `vaults` store holds `view: 'tasks'` / `tasksVaultId` /
  `openTasks()` and the per-vault counts.

## Explicitly out of scope (single-vault list)

Task lists (Inbox/Next/Today/etc. as metadata), project and estimated-effort
fields, parent-task hierarchy, the cross-vault aggregated dashboard, Task
Tags on non-Task notes, inline-Todo scanning, templates, the standalone
Quick Task modal, delete/move/duplicate, un-archiving / a show-archived
view, recurring tasks and notifications — see
[Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md),
[Task Tags & Todos](task-tags-and-todos.md), and
[AI-Assisted Task Management](ai-assisted-task-management.md).

## Related use-cases

- [Vault Discovery, Listing & Opening](vault-discovery-and-open.md)
- [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md) (planned)
- [Task Tags & Todos](task-tags-and-todos.md) (planned)
