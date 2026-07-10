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

The shipped [Per-Vault Task List](per-vault-task-list.md) implements only the **Task** concept (`type: Task` documents). Since v0.5.3 those Task documents DO carry Obsidian-compatible `tags:` frontmatter (chips, filtering, a tag grouping view — see `core::tasks::note_tags`), but that is a property of Tasks themselves, not this use-case: the tag scanner runs only on `type: Task` files inside the tasks folder. Nothing marks a **non-Task** note as actionable via a Task Tag (no scan outside the tasks folder, no surfacing of tagged Meeting/project notes), and nothing scans any note body for `- [ ]` / `- [x]` Todo lines.

## Related use-cases

- [Per-Vault Task List](per-vault-task-list.md)
- [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md)
