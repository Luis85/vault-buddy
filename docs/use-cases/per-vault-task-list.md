---
type: UseCase
status: shipped
domain: task-management
shipped_in: v0.5.0
source_prd: "docs/prds/task-management.md"
related_specs:
  - "docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md"
tags: [use-case, task-management]
---

# Per-Vault Task List

> Every vault gets a simple todo list backed by `type: Task` Markdown documents in a configurable Tasks folder — add a task, check it off — with no Obsidian window required.

## Source

First vertical slice of the [Task Management capability PRD](../prds/task-management.md), whose Domain Model, Task Model, and User Experience sections describe the full-featured version (Quick Task Modal, priorities, due dates, lists, aggregation). This use-case implements only the smallest end-to-end path: configure a per-vault tasks folder, list tasks, add a task, toggle completion.

## ⚠ PRD status is stale

The Task Management PRD's header still reads **`Status: Draft`** and its Roadmap → Version 1 bullets ("Task creation", "Task editing", "Task aggregation", "Task lists", "Dashboard") do not distinguish what has actually shipped. In reality, task creation/listing/completion **shipped in v0.5.0** (2026-07-08/09) — task editing beyond the status checkbox, aggregation across vaults, lists, and the dashboard remain unbuilt. `AGENTS.md`'s own IPC command inventory (§ IPC surface) is also out of date: it does not list `task_commands::*`, even though those commands are registered in `src-tauri/src/lib.rs`.

**Recommendation:** update `docs/prds/task-management.md`'s status line and Version 1 roadmap checklist, and add the five `task_commands` entries to `AGENTS.md`'s IPC surface list.

## Status: Shipped (v0.5.0)

## Implementation

- `core::tasks` (pure, unit-tested): `type: Task` frontmatter identity, `task_basename`/`render_task`/`list_tasks`/`is_task`/`set_status`. A task file is any `.md` in the tasks folder whose frontmatter `type` is `Task` — including hand-authored files, per the PRD's Obsidian-Properties/Dataview-compatible model.
- Two sanctioned vault writes, mirroring the transcript sidecar's never-clobber discipline: collision-safe create (`write_note_collision_safe`) and a surgical `status:`-only read-modify-write (validated path containment, replacing rename into place).
- Config: `tasks_folder` on `VaultCaptureConfig` (`core/src/capture_config.rs`), default `"Tasks"`.
- IPC: `get_tasks_config`, `set_tasks_config`, `list_tasks`, `add_task`, `set_task_status` (`src-tauri/src/task_commands.rs`).
- Frontend: `Tasks.vue` (self-contained, no dedicated Pinia store), reached via a new Tasks button on each vault row; `vaults` store gained `view: 'tasks'` / `tasksVaultId` / `openTasks()`.

## Explicitly out of scope for this slice

Due dates, priority, task lists (Inbox/Next/Today/etc.), tags, project, estimated effort, parent-task hierarchy, the cross-vault aggregated dashboard, Task Tags on other notes, inline-Todo scanning, templates, the standalone Quick Task modal, archive/delete/rename/move/duplicate, search/filtering, opening a task in Obsidian, recurring tasks and notifications — see [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md), [Task Tags & Todos](task-tags-and-todos.md), and [AI-Assisted Task Management](ai-assisted-task-management.md).

## Related use-cases

- [Vault Discovery, Listing & Opening](vault-discovery-and-open.md)
- [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md) (planned)
- [Task Tags & Todos](task-tags-and-todos.md) (planned)
