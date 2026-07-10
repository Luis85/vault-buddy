---
type: UseCase
status: in-progress
domain: ai-platform
source_prd: "docs/prds/ai-platform.md"
tags: [use-case, ai-platform]
---

# Vault Buddy Runtime & Embedded MCP Server

> Every capability (Knowledge Intake, Processing, Task Management, Retrieval, Workflow Engine, Vault Management) exposed through a common internal Runtime Service Layer, reachable equally by the desktop UI and by an embedded MCP server so any MCP-compatible AI client (Claude Code, Cursor, Codex, ...) can act on the user's vaults with the same permission model as a human.

## Source

[AI Platform & Agent Runtime PRD](../prds/ai-platform.md) in full — Runtime Architecture, Human and AI Parity, Capability Domains, MCP Server, Runtime Service Layer, Internal Event Bus, Permissions, AI Clients. Referenced from the main PRD's foundational-documents list and from the Knowledge Lifecycle PRD's [Product Domains](../prds/knowledge-lifecycle.md#product-domains) section. Roadmap: Phase 1 (runtime architecture, service layer, embedded MCP server) through Phase 4 (multi-agent collaboration).

## Status: First slice shipped (v0.6.0 target) — embedded MCP server

The PRD's Phase-1 "embedded MCP server" exists: an opt-in, disabled-by-default
streamable-HTTP server on `127.0.0.1:22082/mcp` (bearer token + Origin
validation), exposing seven tools — `list_vaults`, `list_tasks`,
`list_recordings`, `open_vault`, `open_daily_note`, `add_task`,
`set_task_status` — with writes behind a separate "Allow vault writes" grant
and every call audit-logged. The seam beneath it is the PRD's service-layer
idea in miniature: `core::services` is ONE implementation per capability,
called by both the Tauri IPC commands and the MCP tools (design:
[2026-07-09-local-mcp-server-design.md](../superpowers/specs/2026-07-09-local-mcp-server-design.md)).

Still vision, not started: the internal event bus
(`TaskCreated`/`RecordingFinished`/…), typed service objects
(`KnowledgeService`/`TaskService`/…), fine-grained permission scopes,
capture/transcription control over MCP, MCP resources/prompts, the plugin
API, and everything in Phases 2–4.

## Related use-cases

- [Local MCP Hub Assistant](local-mcp-hub-assistant.md) — a narrower, already-drafted slice (local Ollama model + user-configured MCP *client* connections) that could plausibly precede or coexist with the full runtime described here; note the direction differs (Local MCP Hub makes Vault Buddy an MCP *client*, this PRD makes Vault Buddy an MCP *server*).
- [Plugin & Agent Platform](plugin-and-agent-platform.md)
- [Workflow Automation Engine](workflow-automation-engine.md)
