---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.3.0
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-05-recordings-list-design.md"
  - "docs/superpowers/specs/2026-07-05-recordings-enhancements-design.md"
tags: [use-case, knowledge-intake]
---

# Recordings Browser

> A read-only, per-vault list of past recordings — grouped by type, showing title/date/duration/transcript status — that opens the companion note in Obsidian on click. Never writes into the vault.

## Source

Knowledge Intake PRD, [Recordings List](../prds/knowledge-intake.md): *"From the record chooser, Browse recordings opens a read-only list of the vault's captures, grouped by type... Selecting a row opens its companion note in Obsidian."*

## Status: Shipped (v0.3.0, enhanced through v0.4.x)

## Implementation

- `core::recordings::recording_roots` enumerates a vault's capture folders; `list_recordings` scans them and reads each companion note's frontmatter (`type`/title) plus transcript status via `core::transcript::transcript_status` (Missing/Pending/Failed/Complete).
- IPC: `list_recordings`, `open_recording`, `open_transcript` (`src-tauri/src/capture_commands.rs`) — opening hands off to Obsidian via `obsidian://` (logged through `uri::launch`), never writes.
- Frontend: `Recordings.vue`, reached via `RecordMode.vue`'s "Browse recordings" action; `vaults` store's `recordingsVaultId` / `view: 'recordings'`.

## Related use-cases

- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md)
- [Re-transcription](re-transcription.md)
- [Rename Recording](rename-recording.md)
