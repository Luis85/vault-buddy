---
type: UseCase
status: vision
domain: ai-platform
source_prd: "docs/prds/ai-platform.md"
tags: [use-case, ai-platform]
---

# Vault Buddy Runtime & Embedded MCP Server

> Every capability (Knowledge Intake, Processing, Task Management, Retrieval, Workflow Engine, Vault Management) exposed through a common internal Runtime Service Layer, reachable equally by the desktop UI and by an embedded MCP server so any MCP-compatible AI client (Claude Code, Cursor, Codex, ...) can act on the user's vaults with the same permission model as a human.

## Source

[AI Platform & Agent Runtime PRD](../prds/ai-platform.md) in full — Runtime Architecture, Human and AI Parity, Capability Domains, MCP Server, Runtime Service Layer, Internal Event Bus, Permissions, AI Clients. Referenced from the main PRD's foundational-documents list and from the Knowledge Lifecycle PRD's [Product Domains](../prds/knowledge-lifecycle.md#product-domains) section. Roadmap: Phase 1 (runtime architecture, service layer, embedded MCP server) through Phase 4 (multi-agent collaboration).

## Status: Not started

Today's Tauri commands (`commands.rs`, `capture_commands.rs`, `task_commands.rs`) are the closest thing to a "service layer," but they are UI-only IPC endpoints — there is no MCP server process, no internal event bus (`TaskCreated`/`RecordingFinished`/etc.), no typed service objects (`KnowledgeService`/`TaskService`/...), and no permission-scoped external-client model. This PRD is explicitly a **Vision** document ("Status: Product Vision"), not a committed increment.

## Related use-cases

- [Local MCP Hub Assistant](local-mcp-hub-assistant.md) — a narrower, already-drafted slice (local Ollama model + user-configured MCP *client* connections) that could plausibly precede or coexist with the full runtime described here; note the direction differs (Local MCP Hub makes Vault Buddy an MCP *client*, this PRD makes Vault Buddy an MCP *server*).
- [Plugin & Agent Platform](plugin-and-agent-platform.md)
- [Workflow Automation Engine](workflow-automation-engine.md)
