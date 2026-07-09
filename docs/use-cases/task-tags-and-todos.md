---
type: UseCase
status: planned
domain: task-management
source_prd: "docs/prds/task-management.md"
tags: [use-case, task-management]
---

# Task Tags & Todos

> Two lighter-weight ways to mark work without creating a full Task document: a **Task Tag** on any other note (marking the whole note as actionable), and a **Todo** checklist line inside any note's body.

## Source

Task Management PRD, [Domain Model: Task vs Task Tag vs Todo](../prds/task-management.md), [Task Tag Model](../prds/task-management.md), and [Todo Model](../prds/task-management.md). Functional Requirements sections "Task Tags" (apply/remove a tag, surface in the Aggregated Task View) and "Todos" (add/toggle/remove/reorder a Todo line).

## Status: Not started

The shipped [Per-Vault Task List](per-vault-task-list.md) implements only the **Task** concept (`type: Task` documents). Its design spec explicitly scopes out Task Tags on other notes and inline-Todo scanning. `core::tasks` has no code path that reads a note's `tags` frontmatter for a Task Tag, nor one that scans a note body for `- [ ]` / `- [x]` lines.

## Related use-cases

- [Per-Vault Task List](per-vault-task-list.md)
- [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md)
