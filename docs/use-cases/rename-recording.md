---
type: UseCase
status: shipped
domain: knowledge-intake
shipped_in: v0.4.0 (approximate — recordings-enhancements increment)
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-05-recordings-enhancements-design.md"
tags: [use-case, knowledge-intake, undocumented-in-prd]
---

# Rename Recording

> A recording row can be renamed in place — the audio file, its companion note, and the note's embed line are all retargeted atomically, keeping the capture's date/time filename prefix.

## ⚠ Minor PRD gap

Renaming is a fully shipped, safety-critical feature (its own core function `rename_plan`, its own IPC command, its own recovery-safety reasoning in AGENTS.md) that is **not narrated anywhere in the Knowledge Intake PRD's User Stories, MVP Feature description, or Functional Requirements** — it exists only as an AGENTS.md invariant ("Rename never breaks the capture contract") and its design spec. It is adjacent to, but not the same as, the PRD's "Configurable naming templates" bullet under File Management.

## Status: Shipped

## Implementation

- `core::rename::rename_plan` keeps the `YYYY-MM-DD HHmm ` prefix and refuses non-capture files; execution reuses the reservation + `rename_noreplace` + suffix-retry loop, retargets exactly the note's embed line.
- A note-side failure after a successful audio move degrades to a warning rather than an error (audio-first safety).
- IPC: `rename_capture` (`src-tauri/src/capture_commands.rs`).

## Related use-cases

- [Recordings Browser](recordings-browser.md)
- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md)
