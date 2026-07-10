---
type: UseCase
status: shipped
domain: vault-management
shipped_in: v0.3.0
source_prd: "docs/PRD.md"
tags: [use-case, vault-management, foundation]
---

# Vault Discovery, Listing & Opening

> Vault Buddy reads Obsidian's own vault registry and lets the user open or switch vaults with one click — without ever writing into a vault itself.

## Source

Main PRD, [§14 Functional Requirements → Vault Management](../PRD.md) (detect installed Obsidian, discover vaults, open vault, switch vault) and [§11 Core Capabilities → Obsidian Integration → Vault discovery / Vault switching](../PRD.md). Listed shipped in Phase 1 — Foundation ("Obsidian CLI ✓ (via `obsidian://` URIs)", "Vault Detection ✓").

## Status: Shipped (v0.3.0)

## Implementation

- `core::discovery` parses `%APPDATA%\obsidian\obsidian.json`; `core::process` clears stale `open` flags when no Obsidian process is running.
- IPC: `list_vaults`, `open_vault` (`src-tauri/src/commands.rs`), addressing vaults by registry ID (never by name, since folder names can collide).
- `core::uri::launch` — every launched `obsidian://` URI is logged as the audit trail, per the vault domain's "never writes into a vault" hard rule (see AGENTS.md § The vault domain).
- Frontend: `vaults` Pinia store (`src/stores/vaults.ts`) re-runs discovery on every panel open; `VaultList.vue` surfaces "Open now" vaults first.

## Not in PRD / roadmap gaps

- **Favorite Vaults** and **Recent Vaults** (both listed under Vault Management in the main PRD) are **not implemented** — no favoriting or MRU state exists in `vaults.ts` or `config.json`.
- Multiple-vault support exists implicitly (the list itself), but there is no explicit "default vault" setting despite being called out under Knowledge Intake's General vault settings.

## Related use-cases

- [Daily Note](daily-note.md)
- [Per-Vault Task List](per-vault-task-list.md)
- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md)
