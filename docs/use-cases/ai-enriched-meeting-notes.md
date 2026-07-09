---
type: UseCase
status: planned
domain: knowledge-intake
source_prd: "docs/prds/knowledge-intake.md"
tags: [use-case, knowledge-intake, ai]
---

# AI-Enriched Meeting Notes (Summaries, Task Extraction)

> Beyond the static Follow-up scaffold, an AI pass over the transcript would generate a real summary, extract action items and decisions, and populate the companion note automatically.

## Source

Knowledge Intake PRD, [Future Roadmap → Version 2 — planned](../prds/knowledge-intake.md): Summaries, Task Extraction, AI-enriched Meeting Notes (decisions, action items, open questions). Also [Vault Settings → AI (planned)](../prds/knowledge-intake.md): Generate Summary, Extract Tasks, Generate Meeting Note (AI-enriched), Preferred LLM. Main PRD Phase 3 lists "Writing Assistant" / "Research Assistant" as still-open Intelligence-phase items alongside this.

## Status: Not started

No summarization/extraction code exists in `vault_buddy_transcribe`, the core crate, or `capture_commands.rs` beyond raw transcript rendering. The companion note explicitly never contains empty AI placeholder sections today (see [Companion Note & Follow-up Template](companion-note-and-follow-up-template.md)) — this use-case is the PRD's sanctioned way to eventually fill that section with real AI output rather than a static scaffold.

## Dependencies

Likely depends on whichever local-LLM integration lands first — see [Local MCP Hub Assistant](local-mcp-hub-assistant.md), which is the only PRD describing a local model provider today (Ollama), though it is scoped as a generic MCP tool-use assistant rather than a dedicated summarization pipeline.

## Related use-cases

- [Local Speech-to-Text Transcription](local-transcription.md)
- [Companion Note & Follow-up Template](companion-note-and-follow-up-template.md)
- [AI-Assisted Task Management](ai-assisted-task-management.md)
