---
type: UseCase
status: draft
domain: ai-platform
source_prd: "docs/prds/local-mcp-hub.md"
tags: [use-case, ai-platform, local-mcp-hub]
---

# Local MCP Hub Assistant (Opt-in Local LLM + MCP Client)

> An optional, disabled-by-default assistant layer: a local Ollama model uses user-configured MCP servers as tools to gather information and produce reviewable ingestion previews — never writing to a vault without explicit confirmation, and never affecting default startup/build/run when disabled.

## Source

[Local MCP Hub PRD](../prds/local-mcp-hub.md) in full — Opt-in Activation, Model Provider (Ollama), MCP Server Registry, Tool Permissions (read-only / write / external side-effect / shell / browser / vault-affecting / unknown), Assistant Interaction, Ingestion Preview. Phased Delivery: Phase 1 (inert config) → Phase 6 (audit & hardening).

## Status: Not started (Draft PRD, Version 0.1)

No `ollama`, MCP client, or local-model code exists anywhere in `src-tauri/` — confirmed by searching the Rust and TypeScript source for `ollama`/`mcp` (only unrelated substring matches in test fixture filenames). The PRD itself is marked **Draft** and includes open questions (which Rust MCP SDK, which default model, JSON import compatibility with Claude Desktop) not yet resolved.

## Relationship to the AI Platform vision

This PRD makes Vault Buddy an MCP **client** (connecting outward to user-configured MCP servers); [MCP Server & Runtime](mcp-server-and-runtime.md) makes Vault Buddy an MCP **server** (exposing its own capabilities to external AI clients like Claude Code or Cursor). They are complementary, not sequential — either could ship first, and a mature Vault Buddy would likely need both.

## Related use-cases

- [MCP Server & Runtime](mcp-server-and-runtime.md)
- [AI-Enriched Meeting Notes](ai-enriched-meeting-notes.md) — a plausible downstream consumer once a local model provider exists
- [Natural Language Interface](natural-language-interface.md)
