---
type: UseCase
status: shipped
domain: task-management
source_prd: "docs/prds/task-management.md"
shipped_in: v0.5.4
tags: [use-case, task-management]
---

# Aggregated Task Dashboard & Custom Lists

> A cross-vault dashboard (Today, Overdue, Inbox, Upcoming, High Priority, ...) and user-defined task lists (Inbox, Next, Today, Waiting, Someday, custom), plus the standalone Quick Task modal, bulk operations, and full-text task search.

## Source

Task Management PRD:

- [User Experience → Quick Task Modal](../prds/task-management.md) — a fast, title-only-required modal reachable without opening a vault view first.
- [Vault Settings → Lists](../prds/task-management.md) and [Task Lists](../prds/task-management.md) — Inbox/Next/Today/Waiting/Someday/custom; amended in the lists increment to be real folders under the tasks folder, not metadata.
- [Aggregated Task View](../prds/task-management.md) — every configured vault merged into one dashboard, filterable by vault/list/status/priority/due date/project/tag/dates/search text.
- [Task Dashboard](../prds/task-management.md) — Today's Tasks, Overdue, Inbox, Upcoming, Completed Today, Recently Created, High Priority, Recently Modified.
- [Bulk Operations](../prds/task-management.md) and [Search](../prds/task-management.md).
- Roadmap Version 1 (creation/editing/aggregation/lists/dashboard) and Version 2 (templates, recurring tasks, saved filters, quick actions).

## Status: Partially shipped — the aggregated view (v0.5.4) + lists, sorting, manual ordering (lists increment) + the do-date planner (do-date/planner-foundation increment)

[Per-Vault Task List](per-vault-task-list.md) (shipped v0.5.0, extended through v0.5.3) covers the single-vault experience: `TaskItem` carries due dates, priority and tags; the view has date buckets, a tag grouping mode, filters, an inline editor (rename/due/priority/tags), archive, and open-in-Obsidian.

The cross-vault half of this use-case shipped in v0.5.4, frontend-only (zero new IPC commands): an "All tasks" entry bar above the vault list (badge = summed open-task count across vaults) opens `Tasks.vue` in aggregate mode, which fans out `list_vaults` and a parallel per-vault `list_tasks` into one merged, sorted list. It carries the full interactivity of the single-vault view — toggle, archive, inline edit, date buckets, tag grouping, filters — plus aggregate-only additions: a per-row vault-attribution chip (initial + full name on hover), and an add-task vault picker so a new task lands in the vault you choose. Loading is best-effort per vault (a failed vault is named in one toast; a blocking banner appears only if every vault fails).

The lists increment then shipped user-defined Lists as real folders under the tasks folder (spec: `docs/superpowers/specs/2026-07-11-task-lists-sorting-ordering-design.md`): a Lists grouping mode (per-list sections, cross-vault case-insensitive merge in the aggregate), list pickers on the composer (with inline "New list…" creation) and the inline editor (moving the task's file between list folders on the never-clobber rails), a per-vault lists settings object (default list + display order, edited in Vault settings), user-selectable sorting (Default/Due/Priority/Created/Title/Manual + direction, persisted per view), and manual drag-to-reorder persisting an `order` frontmatter rank (pointer-based with a keyboard fallback).

The do-date/planner-foundation increment (spec: `docs/superpowers/specs/2026-07-24-task-management-do-date-planner-foundation-design.md`) then shipped the cross-vault planner half of the Task Dashboard: a Do Date (`scheduled`, distinct from Due Date) on every Task, plus a do-date-aware Plan grouping — the `Dates` tab relabeled **Plan** (the internal grouping key is unchanged, no migration) — bucketing by the Task's effective plan date (Do Date if set, else Due Date) into Overdue / Today / Upcoming / Anytime / Done. The aggregate ("All tasks") view now opens on Plan by default (a per-vault view still opens on Lists; a user's own persisted grouping choice always wins). New plan-my-day verbs round it out: a per-row quick-schedule popover (Today / Tomorrow / This weekend / pick / Clear), a best-effort "Reschedule → Today" action on the whole Overdue bucket, and a Do Date field on both the add-task composer and the inline editor — all zero-new-IPC, riding the two commands the aggregate view already used (`list_tasks`/`update_task`), which now carry `scheduled`.

What remains planned: no standalone dashboard screen (Inbox / Completed Today / Recently Created / High Priority / Recently Modified as separate summary widgets — Today/Overdue/Upcoming now ship as the Plan grouping inside `Tasks.vue`, see above), no standalone Quick Task modal, no bulk operations, no full-text task search, no list rename/delete from the buddy, and no delete/duplicate (move shipped as the list move).

## Related use-cases

- [Per-Vault Task List](per-vault-task-list.md)
- [Task Tags & Todos](task-tags-and-todos.md)
- [AI-Assisted Task Management](ai-assisted-task-management.md)
