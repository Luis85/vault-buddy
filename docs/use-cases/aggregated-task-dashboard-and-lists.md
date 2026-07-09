---
type: UseCase
status: planned
domain: task-management
source_prd: "docs/prds/task-management.md"
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

## Status: Not started (beyond the single-vault slice)

[Per-Vault Task List](per-vault-task-list.md) (shipped v0.5.0) covers only one vault's flat task list with add + status-toggle. There is no cross-vault aggregation service, no list/tag/priority/due-date model on the `TaskItem` struct (`core::tasks`), no dashboard component, no Quick Task modal, and no bulk operations. Editing a task beyond its `status` (rename, edit metadata/content, move, archive, delete, duplicate) is also unimplemented — the vertical-slice spec explicitly deferred it.

## Related use-cases

- [Per-Vault Task List](per-vault-task-list.md)
- [Task Tags & Todos](task-tags-and-todos.md)
- [AI-Assisted Task Management](ai-assisted-task-management.md)
