---
type: UseCase
status: planned
domain: knowledge-retrieval
source_prd: "docs/PRD - Product Vision.md"
tags: [use-case, knowledge-retrieval]
---

# Knowledge Search & Retrieval

> Keyword, semantic, and tag search across notes/tags/properties/links/tasks/files/templates/commands; graph exploration, backlinks, recent activity, related notes — "the user should never ask: where did I save that?"

## Source

Main PRD, [§14 Functional Requirements → Search](../PRD%20-%20Product%20Vision.md) and [§11 Core Capabilities → Knowledge Search](../PRD%20-%20Product%20Vision.md). Explicitly the *other* named gap in Phase 1 — Foundation alongside Tasks (*"Shipped in v0.3.0, except Search and Tasks"*) — Tasks has since partially shipped ([Per-Vault Task List](per-vault-task-list.md)), Search has not moved. Also Knowledge Lifecycle PRD, [Lifecycle Stage 5 — Knowledge Retrieval](../prds/knowledge-lifecycle.md), and Phase 2 — Productivity ("Semantic Search", "Context Awareness") in the main PRD's roadmap.

## Status: Not started

No search index, search command, or search UI exists anywhere in the codebase (`grep` for `SearchEngine`/full-text/semantic search returns nothing). This remains the single oldest unshipped item from Phase 1 of the roadmap.

## Related use-cases

- [Natural Language Interface](natural-language-interface.md)
- [MCP Server & Runtime](mcp-server-and-runtime.md) (Knowledge Retrieval is a named capability domain there too)
