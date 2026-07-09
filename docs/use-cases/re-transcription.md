---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.3.0
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-05-increment-3-transcription-polish-design.md"
  - "docs/superpowers/specs/2026-07-06-transcription-control-and-progress-design.md"
  - "docs/superpowers/specs/2026-07-07-transcription-reliability-and-verification-design.md"
tags: [use-case, knowledge-intake]
---

# Re-transcription

> Every recording row offers an explicit "re-transcribe" action that regenerates its transcript on demand — after switching models, or to recover a failed attempt — with a confirmation before replacing a finished transcript.

## Source

Knowledge Intake PRD, [Re-transcription](../prds/knowledge-intake.md): *"Every recording row offers a re-transcribe action... confirms before replacing a finished transcript, bypasses the vault's automatic-transcription toggle... overwrites only the transcript sidecar — never the audio or the note."*

## Status: Shipped (v0.3.0)

## Implementation

- IPC: `retranscribe` (force path, `src-tauri/src/`) vs `transcribe_recording_now` (gated retry path) — see AGENTS.md § The transcription & recordings domains for the exact never-clobber distinction between the two.
- `retranscribe` uses `force_write_sidecar`, an unguarded but **sidecar-only** overwrite — it regenerates even a `complete` transcript, but the up-front "transcribing…" placeholder is skipped when the sidecar is already `Complete`, so a forced job that fails mid-flight leaves the original intact.
- Frontend: per-row action in `Recordings.vue`, confirmation dialog before replacing a finished transcript.

## Related use-cases

- [Local Speech-to-Text Transcription](local-transcription.md)
- [Recordings Browser](recordings-browser.md)
