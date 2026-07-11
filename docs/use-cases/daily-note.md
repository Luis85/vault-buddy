---
type: UseCase
status: shipped
domain: vault-management
shipped_in: v0.3.0
source_prd: "docs/PRD.md"
related_specs:
  - "docs/superpowers/specs/2026-07-03-increment-1-companion-daily-note-design.md"
tags: [use-case, vault-management, foundation]
---

# Daily Note: Open & Create

> One click opens today's Daily Note in the active vault, using that vault's own Daily Notes plugin configuration — creating it in Obsidian if it doesn't exist yet.

## Source

Main PRD, [§14 Functional Requirements → Daily Notes](../PRD.md) (open, create) and [§11 Core Capabilities → Obsidian Integration → Daily Notes](../PRD.md). Shipped in Phase 1 — Foundation ("Daily Notes ✓").

## Status: Shipped (v0.3.0)

## Implementation

- `core::daily_notes` reads each vault's `.obsidian/daily-notes.json` (folder + moment-style format); only `YYYY`/`MM`/`DD` tokens are supported — an unsupported format (`MMMM`, `YYYYMMDD`) falls back to the default format entirely rather than half-substituting, to avoid Obsidian silently creating a misnamed note.
- `core::daily_note_uri` picks `obsidian://open` when the file exists and `obsidian://new` otherwise — the actual file creation happens inside Obsidian, keeping Vault Buddy's vault domain read-only.
- IPC: `open_daily_note` (`src-tauri/src/commands.rs`).

## Roadmap gap

The PRD lists **Append**, **Review**, and **Archive** as Daily Notes functional requirements beyond Open/Create — none of these are implemented; only open/create-via-Obsidian exists.

## Related use-cases

- [Vault Discovery, Listing & Opening](vault-discovery-and-open.md)
