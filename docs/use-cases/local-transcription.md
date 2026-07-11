---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.3.0
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-04-increment-3-local-speech-to-text-design.md"
  - "docs/superpowers/specs/2026-07-05-increment-3-transcription-polish-design.md"
  - "docs/superpowers/specs/2026-07-06-transcription-control-and-progress-design.md"
  - "docs/superpowers/specs/2026-07-07-transcription-reliability-and-verification-design.md"
tags: [use-case, knowledge-intake]
---

# Local Speech-to-Text Transcription

> After a recording finishes, Vault Buddy can transcribe it entirely on-device (whisper.cpp) and write a Markdown sidecar the companion note embeds — no cloud, no API, no telemetry.

## Source

Main PRD, [§18 Product Roadmap → Phase 3 — Intelligence](../PRD.md): *"Landed early: Local AI ✓ (on-device speech-to-text)... shipped in v0.3.0, ahead of Phase 2."* Knowledge Intake PRD, [Vault Settings → Transcription (shipped)](../prds/knowledge-intake.md): model tier, language/auto-detect, segment timestamps, follow-up template toggle.

## Status: Shipped (v0.3.0), hardened through v0.5.0

## Implementation

- `vault_buddy_transcribe` crate: MP3 → 16kHz mono PCM (Symphonia) → whisper.cpp via `whisper-rs` (behind the `whisper` feature; the real engine is Windows-only, CI-gated).
- Single worker queue in the shell (`enqueue_transcription`/`process_transcription`, `capture_commands.rs`) — one `TranscriptionJob` at a time, so the model loads once per tier. Models download on demand with progress events (`capture:modelDownload`).
- `core::transcript` writes a `<base>.transcript.md` sidecar under a strict never-clobber discipline: a `vault-buddy-transcript: pending/failed/complete` frontmatter marker tags Vault Buddy's own regenerable output; only `pending`/`failed` sidecars are ever replaced (`replace_if_ours`) — a completed transcript or hand-edited file is left untouched.
- IPC: `transcribe_recording_now`, `retranscribe`, `cancel_transcription`, `transcription_queue_status` (`src-tauri/src/`).
- Recovery backfill (`pending_transcriptions`) re-enqueues interrupted jobs on next launch; a `failed` sidecar is deliberately **not** auto-retried, requiring an explicit user action.

## Related use-cases

- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md)
- [Companion Note & Follow-up Template](companion-note-and-follow-up-template.md)
- [Re-transcription](re-transcription.md)
- [Recordings Browser](recordings-browser.md)
- [AI-Enriched Meeting Notes](ai-enriched-meeting-notes.md) (planned successor)
