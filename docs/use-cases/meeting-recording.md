---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.3.0
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md"
  - "docs/superpowers/specs/2026-07-04-increment-3-capture-polish-design.md"
tags: [use-case, knowledge-intake]
---

# Meeting Recording

> One click records a meeting (microphone + desktop/loopback audio) straight into the configured vault as an MP3, with no Obsidian interaction required.

## Source

Knowledge Intake PRD, [User Stories → Meeting Recording](../prds/knowledge-intake.md): *"As a user, I want to record a Teams meeting so I can review it later."* Recording Mode: **Meeting** (Microphone + Desktop Audio) in the Recording Modes table. Part of the MVP Feature: Audio Recording, shipped in v0.3.0.

## Status: Shipped (v0.3.0)

## Implementation

- `vault_buddy_capture` crate: cpal device capture (WASAPI loopback on Windows in meeting mode) → mixer → streaming LAME MP3 into a hidden `.mp3.part` (flush ~1s, fsync ~30s) → finalize via `rename_noreplace`.
- IPC: `start_capture`, `stop_capture`, `capture_status`, `pause_capture`, `resume_capture`, `list_audio_devices` (`src-tauri/src/capture_commands.rs`).
- Mode selection: `RecordMode.vue` (`mode: "meeting" | "voice-note"`), microphone + output device pickers shown only in meeting mode (`CaptureSettings.vue`).
- Safety invariants (never lose captured audio, never clobber files, recovery touches only our own files) — see AGENTS.md § The capture domain.
- The buddy itself is the recording indicator; all hide paths funnel through `tray::hide_buddy`, which no-ops mid-recording.

## Roadmap / PRD gaps

The PRD's Recording Modes table also lists **Desktop Audio** (output-only) and **Custom** (manual device selection) modes — neither exists in the shipped UI (`RecordMode.vue` only offers `meeting` and `voice-note`). Treat as unshipped.

## Related use-cases

- [Voice Note Recording](voice-note-recording.md)
- [Local Speech-to-Text Transcription](local-transcription.md)
- [Companion Note & Follow-up Template](companion-note-and-follow-up-template.md)
- [Recordings Browser](recordings-browser.md)
