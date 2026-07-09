---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.3.0
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md"
tags: [use-case, knowledge-intake]
---

# Voice Note Recording

> One click records a quick spoken idea (microphone only) into the vault — no Teams call, no Obsidian window needed.

## Source

Knowledge Intake PRD, [User Stories → Voice Note](../prds/knowledge-intake.md): *"As a user, I want to quickly record an idea without opening Obsidian."* Also covers the adjacent stories **Customer Call** ("every customer conversation stored in my project Vault") and **Research Session** ("capture spoken observations while working") — both are usage patterns of the same Voice Note capture path (microphone-only recording into a per-vault folder), not separate implementations. Recording Mode: **Voice Note** (Microphone only).

## Status: Shipped (v0.3.0)

## Implementation

Shares the entire capture pipeline with [Meeting Recording](meeting-recording.md) (`vault_buddy_capture`, `start_capture`/`stop_capture`/`capture_status`); the only difference is `mode: "voice-note"` in `RecordMode.vue`, which skips desktop-audio device capture and defaults the target folder to the vault's Voice Notes folder (`CaptureSettings.vue`, per-vault config in `%APPDATA%\vault-buddy\config.json`).

## Related use-cases

- [Meeting Recording](meeting-recording.md)
- [Local Speech-to-Text Transcription](local-transcription.md)
- [Companion Note & Follow-up Template](companion-note-and-follow-up-template.md)
