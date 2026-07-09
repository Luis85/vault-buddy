---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.3.0
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md"
  - "docs/superpowers/specs/2026-07-05-increment-3-transcript-stats-design.md"
tags: [use-case, knowledge-intake]
---

# Companion Note & Follow-up Template

> Every recording can generate a Markdown note alongside the audio, embedding the recording, the metadata, and — once ready — the transcript, plus an optional static Follow-up scaffold.

## Source

Knowledge Intake PRD, [Optional Meeting Note](../prds/knowledge-intake.md): metadata, embedded recording, embedded transcript, `## Follow-up` scaffold (action items / decisions / notes) when the per-vault Follow-up Template setting is on (default). Explicit non-goal called out in the PRD: *"the note never contains empty AI placeholder sections"* — the Follow-up section is a static scaffold, not AI output.

## Status: Shipped (v0.3.0)

## Implementation

- `render_note` in the core crate renders the note body; written via the same atomic-temp-then-`rename_noreplace` discipline as the audio file (owned `.vault-buddy.tmp` temp, never clobbers an existing note).
- Per-vault `createNote` (default on) and `follow_up_template` (default on) toggles, `CaptureSettings.vue` / `core::capture_config`.
- The transcript embed is threaded in after the fact by [Local Speech-to-Text Transcription](local-transcription.md) writing the `<base>.transcript.md` sidecar the note already references.

## Related use-cases

- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md)
- [Local Speech-to-Text Transcription](local-transcription.md)
- [AI-Enriched Meeting Notes](ai-enriched-meeting-notes.md) (planned: AI-produced summaries/decisions/action-items replacing the static scaffold)
