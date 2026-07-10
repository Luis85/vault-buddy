---
type: UseCase
status: partially-shipped
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
- [Vault Settings → Lists](../prds/task-management.md) and [Task Lists](../prds/task-management.md) — Inbox/Next/Today/Waiting/Someday/custom, stored as metadata rather than physical folders.
- [Aggregated Task View](../prds/task-management.md) — every configured vault merged into one dashboard, filterable by vault/list/status/priority/due date/project/tag/dates/search text.
- [Task Dashboard](../prds/task-management.md) — Today's Tasks, Overdue, Inbox, Upcoming, Completed Today, Recently Created, High Priority, Recently Modified.
- [Bulk Operations](../prds/task-management.md) and [Search](../prds/task-management.md).
- Roadmap Version 1 (creation/editing/aggregation/lists/dashboard) and Version 2 (templates, recurring tasks, saved filters, quick actions).

## Status: Partially shipped — the aggregated view (v0.5.4)

[Per-Vault Task List](per-vault-task-list.md) (shipped v0.5.0, extended through v0.5.3) covers the single-vault experience: `TaskItem` carries due dates, priority and tags; the view has date buckets, a tag grouping mode, filters, an inline editor (rename/due/priority/tags), archive, and open-in-Obsidian.

The cross-vault half of this use-case shipped in v0.5.4, frontend-only (zero new IPC commands): an "All tasks" entry bar above the vault list (badge = summed open-task count across vaults) opens `Tasks.vue` in aggregate mode, which fans out `list_vaults` and a parallel per-vault `list_tasks` into one merged, sorted list. It carries the full interactivity of the single-vault view — toggle, archive, inline edit, date buckets, tag grouping, filters — plus aggregate-only additions: a per-row vault-attribution chip (initial + full name on hover), and an add-task vault picker so a new task lands in the vault you choose. Loading is best-effort per vault (a failed vault is named in one toast; a blocking banner appears only if every vault fails).

What remains planned: no user-defined lists (Inbox/Next/Today/Waiting/Someday/custom, as metadata rather than dates), no dashboard component (Today's Tasks/Overdue/Recently Created/etc. as separate summary widgets), no standalone Quick Task modal, no bulk operations, no full-text task search, and no delete/move/duplicate.

## Related use-cases

- [Per-Vault Task List](per-vault-task-list.md)
- [Task Tags & Todos](task-tags-and-todos.md)
- [AI-Assisted Task Management](ai-assisted-task-management.md)
