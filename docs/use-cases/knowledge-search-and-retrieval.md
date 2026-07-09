---
type: UseCase
status: shipped
domain: knowledge-retrieval
shipped_in: "main (unreleased, post-v0.5.1)"
source_prd: "docs/PRD - Product Vision.md"
related_specs:
  - "docs/superpowers/specs/2026-07-09-vault-search-design.md"
  - "docs/superpowers/specs/2026-07-09-search-polish-design.md"
  - "docs/superpowers/specs/2026-07-09-search-ux-design.md"
tags: [use-case, knowledge-retrieval]
---

# Knowledge Search & Retrieval

> Keyword, semantic, and tag search across notes/tags/properties/links/tasks/files/templates/commands; graph exploration, backlinks, recent activity, related notes — "the user should never ask: where did I save that?"

## Source

Main PRD, [§14 Functional Requirements → Search](../PRD%20-%20Product%20Vision.md) and
[§11 Core Capabilities → Knowledge Search](../PRD%20-%20Product%20Vision.md) — the
*other* named gap in Phase 1 — Foundation alongside Tasks. Also Knowledge
Lifecycle PRD, [Lifecycle Stage 5 — Knowledge
Retrieval](../prds/knowledge-lifecycle.md), and Phase 2 — Productivity
("Semantic Search", "Context Awareness") in the main PRD's roadmap.

## Status: Shipped — keyword slice (merged to main after v0.5.1, unreleased)

The **keyword** slice of this use-case shipped in PR #44 (2026-07-09): live,
cross-vault, read-only substring search over note names, note contents, and
attachment filenames, opened from a magnifier in the panel header (or `/` /
`Ctrl+F` from the vault list). Semantic search, tag/property/link/task/
template/command search, graph exploration, backlinks, recent activity, and
related notes remain unbuilt — see "Out of scope" below.

## Implementation

- `core::search` (pure, unit-tested on Linux): on-demand scan, no index.
  Case-insensitive substring matching against note stems + note content
  (notes are any-case `.md`; content ≤ 1 MiB UTF-8 with a whole-file
  early-out) and attachment filenames; extensionless files are excluded
  (Obsidian cannot open them). The walk is the shared `core::vault_walk`
  (canonical containment, reparse-cycle guard, dot-dir skips — single-sourced
  with the tasks scan). Hard caps: 2-char minimum query (code points),
  100 hits with a `truncated` flag; "filename matches surface before
  content-only matches" is a hard guarantee (two capped class lists).
- IPC (`src-tauri/src/search_commands.rs`): `search_vaults` — **async**
  (a sync command would run the scan on the main thread and freeze window
  show/hide and drags), `spawn_blocking`ed, `Result`-returning, with a
  scan-generation counter so superseded scans abort; per-vault scans run in
  parallel on named threads. `open_search_result` opens a hit by vault **ID**
  via the launch-logged `obsidian://` path — search never writes into a vault.
- Frontend: `search` panel view (`Search.vue`, self-contained) — 300 ms
  debounce, stale-response ticket, vault-grouped rows with count chips,
  note/attachment icons and index-based highlighting, keyboard navigation
  over the visible rows (arrows + Enter; Ctrl+Enter/Ctrl+click multi-open,
  backed by a Rust-side panel pin that survives Obsidian's focus grab),
  aria-live match summary, collapsible vault groups, All/Notes/Files filter
  chips, IME composition guards, and localStorage-backed recent-search chips
  (`src/utils/recentSearches.ts`).

## Explicitly out of scope for this slice

Semantic search and embeddings; searching tags, properties, links, tasks,
templates, or commands as first-class entities; graph exploration;
backlinks; recent activity; related notes; relevance ranking beyond
filename-before-content; any persistent index. These stay with Phase 2 —
Productivity ("Semantic Search", "Context Awareness") and the [Knowledge
Lifecycle](../prds/knowledge-lifecycle.md) vision.

## Related use-cases

- [Vault Discovery, Listing & Opening](vault-discovery-and-open.md)
- [Natural Language Interface](natural-language-interface.md)
- [MCP Server & Runtime](mcp-server-and-runtime.md) (Knowledge Retrieval is a named capability domain there too)
