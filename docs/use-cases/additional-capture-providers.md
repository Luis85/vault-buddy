---
type: UseCase
status: planned
domain: knowledge-intake
source_prd: "docs/prds/knowledge-intake.md"
tags: [use-case, knowledge-intake]
---

# Additional Capture Providers (Screenshot, Clipboard, File, Screen, Browser, Camera)

> Audio Recording was the first of many planned "Capture Providers," all following the same `Capture → Process → Store → Link → (optional) AI Pipeline` workflow.

## Source

Knowledge Intake PRD, [Capability Overview](../prds/knowledge-intake.md) and [Future Roadmap](../prds/knowledge-intake.md):

- **Version 3:** Screenshot Capture, Clipboard Capture, File Import
- **Version 4:** Screen Recording, Browser Capture, Camera Capture
- **Version 5:** Continuous Capture, Meeting Detection, Automatic Project Assignment, Knowledge Inbox, Workflow Automation

Explicitly listed as MVP **Non-Goals**: Video recording, OCR, Cloud synchronization, Live transcription, Collaboration, Mobile support.

## Status: Not started

Only the Audio capture provider (`vault_buddy_capture`) exists. No screenshot, clipboard, file-import, screen-recording, browser-capture, or camera code exists anywhere in `src-tauri/` or `src/`.

## Also unshipped: Desktop Audio / Custom recording modes

Within the *existing* Audio provider, the Knowledge Intake PRD's Recording Modes table lists **Desktop Audio** (output-only) and **Custom** (manual device selection) alongside the two shipped modes (Meeting, Voice Note). `RecordMode.vue` only implements `meeting` and `voice-note` — treat Desktop Audio and Custom as part of this same backlog rather than separately shipped.

## Related use-cases

- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md) (the one shipped provider)
