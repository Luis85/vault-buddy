---
type: UseCase
status: planned
domain: nl-interface
source_prd: "docs/PRD.md"
tags: [use-case, nl-interface]
---

# Natural Language Interface

> Chat with the buddy, quick commands, intent recognition, contextual suggestions, and conversation/command history — the primary way users express intent instead of navigating menus.

## Source

Main PRD, [§11 Core Capabilities → Natural Language Interface](../PRD.md) and [§18 Roadmap → Phase 2 — Productivity](../PRD.md) ("Natural Language", "Quick Commands"). This is also the mechanism implied throughout §2 Product Vision ("users simply express intent... Vault Buddy translates intent into safe, contextual actions") and §17's High-Level Architecture diagram (`User → Natural Language → Intent Recognition → Permission & Safety → Workflow Orchestrator`).

## Status: Not started

The current panel is a fixed set of buttons/views (vault list, settings, recordings, tasks) — there is no chat surface, no intent-recognition layer, and no command history anywhere in `src/` or `src-tauri/`. The `announce` IPC command (`src-tauri/src/commands.rs`) is one-directional (Rust → buddy speech bubble text), not a conversational input channel.

## Related use-cases

- [Knowledge Search & Retrieval](knowledge-search-and-retrieval.md)
- [Local MCP Hub Assistant](local-mcp-hub-assistant.md) — the closest existing PRD to a conversational entry point, though scoped as tool-use over MCP servers rather than general NL command execution.
