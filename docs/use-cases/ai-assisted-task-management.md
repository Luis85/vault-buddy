---
type: UseCase
status: planned
domain: task-management
source_prd: "docs/prds/task-management.md"
tags: [use-case, task-management, ai]
---

# AI-Assisted Task Management

> Generate child tasks and Todos, estimate effort, suggest priority/due date, extract tasks from meetings and recordings, merge duplicates, suggest dependencies, and run a weekly review — all AI-assisted.

## Source

Task Management PRD, [AI Features (Future)](../prds/task-management.md) and [Workflow Integration](../prds/task-management.md) (tasks created from audio recordings, meeting notes, clipboard, screenshots, browser captures, emails, future AI conversations). Roadmap Version 3 (AI task generation, task extraction, project suggestions, smart priorities) and Version 4 (workflow automation, dependency graph, review assistant, knowledge-aware planning).

## Status: Not started

No AI integration exists anywhere in the task-management code path (`core::tasks`, `task_commands.rs`, `Tasks.vue`). Meeting/recording → task extraction specifically depends on [AI-Enriched Meeting Notes](ai-enriched-meeting-notes.md) landing first, since there is no summarization/extraction pipeline yet to source tasks from.

## Related use-cases

- [Per-Vault Task List](per-vault-task-list.md)
- [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md)
- [AI-Enriched Meeting Notes](ai-enriched-meeting-notes.md)
