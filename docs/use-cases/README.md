# Vault Buddy — Use Cases

This folder is a flat catalog of every discrete use-case implied by Vault
Buddy's PRDs, cross-checked against what's actually in the codebase today.
Each note is a small, linkable unit: what it is, which PRD it comes from,
whether it has shipped, and where the implementation lives.

It exists alongside the PRDs rather than inside them because the PRDs are
vision documents (roadmaps, principles, long-term direction) while these
notes track ground truth: **is this real yet, and where?** PRDs describe
intent; use-cases report status. Re-run this reconciliation whenever a PRD
changes or a release ships, so the two don't drift apart again.

## Frontmatter

Every note has at minimum:

```yaml
type: UseCase
status: shipped | planned | draft | vision
```

`status` follows the PRD's own vocabulary where one exists (`draft` mirrors
the Local MCP Hub PRD's own "Draft" status; `vision` mirrors the Knowledge
Lifecycle / AI Platform docs' "Product Vision" status). `shipped` means the
capability is merged and reachable in the shipping app; `planned` means it's
named in a PRD roadmap but unbuilt; `draft` means the owning PRD itself is
still a draft with open questions.

Most notes also carry `domain`, `source_prd`, `related_specs`, and
`shipped_in` (the approximate version) — see any note for the full pattern.

## How this was built

Extracted by reading every PRD in `docs/` and `docs/prds/`, then verified
against `src-tauri/src/lib.rs`'s registered IPC commands, the `core`/
`capture`/`transcribe` crates, the frontend Pinia stores, and
`docs/superpowers/specs/` design docs — not from the PRDs' prose alone.
Two capabilities were confirmed shipped but **missing from their own PRD's
status line**: [Per-Vault Task List](per-vault-task-list.md) (the Task
Management PRD still says `Status: Draft`) and
[Rename Recording](rename-recording.md) (never narrated in the Knowledge
Intake PRD at all). Two shipped capabilities had **no PRD whatsoever** —
[Software Auto-Update](software-auto-update.md) and
[Diagnostics, Crash Reporting & Recovery](diagnostics-and-crash-reporting.md)
— which is why the [Platform & Cross-Cutting Capabilities
PRD](../prds/platform-and-cross-cutting.md) now exists: a home for product-wide
mechanics that don't map to any one capability domain, also absorbing the
main PRD's former Non-Functional Requirements section.

---

## Shipped

| Use case | Domain | Shipped in | Source PRD |
| --- | --- | --- | --- |
| [Desktop Companion](desktop-companion.md) | Desktop Companion | v0.3.0 | [PRD](../PRD%20-%20Product%20Vision.md) |
| [Vault Discovery, Listing & Opening](vault-discovery-and-open.md) | Vault Management | v0.3.0 | [PRD](../PRD%20-%20Product%20Vision.md) |
| [Daily Note: Open & Create](daily-note.md) | Vault Management | v0.3.0 | [PRD](../PRD%20-%20Product%20Vision.md) |
| [Software Auto-Update](software-auto-update.md) | Platform | v0.3.0 | [Platform & Cross-Cutting](../prds/platform-and-cross-cutting.md) |
| [Diagnostics, Crash Reporting & Recovery](diagnostics-and-crash-reporting.md) | Platform | v0.3.0 | [Platform & Cross-Cutting](../prds/platform-and-cross-cutting.md) |
| [Meeting Recording](meeting-recording.md) | Knowledge Intake | v0.3.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Voice Note Recording](voice-note-recording.md) | Knowledge Intake | v0.3.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Local Speech-to-Text Transcription](local-transcription.md) | Knowledge Intake | v0.3.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Companion Note & Follow-up Template](companion-note-and-follow-up-template.md) | Knowledge Intake | v0.3.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Recordings Browser](recordings-browser.md) | Knowledge Intake | v0.3.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Re-transcription](re-transcription.md) | Knowledge Intake | v0.3.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Rename Recording](rename-recording.md) ⚠ not in PRD text | Knowledge Intake | v0.4.0 | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Per-Vault Task List](per-vault-task-list.md) | Task Management | v0.5.0 | [Task Management](../prds/task-management.md) |
| [Knowledge Search & Retrieval](knowledge-search-and-retrieval.md) ⚠ keyword slice | Knowledge Retrieval | main (unreleased) | [PRD](../PRD%20-%20Product%20Vision.md) |

## Planned (named in a PRD roadmap, not yet built)

| Use case | Domain | Source PRD |
| --- | --- | --- |
| [AI-Enriched Meeting Notes](ai-enriched-meeting-notes.md) | Knowledge Intake | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Additional Capture Providers](additional-capture-providers.md) | Knowledge Intake | [Knowledge Intake](../prds/knowledge-intake.md) |
| [Task Tags & Todos](task-tags-and-todos.md) | Task Management | [Task Management](../prds/task-management.md) |
| [Aggregated Task Dashboard & Lists](aggregated-task-dashboard-and-lists.md) | Task Management | [Task Management](../prds/task-management.md) |
| [AI-Assisted Task Management](ai-assisted-task-management.md) | Task Management | [Task Management](../prds/task-management.md) |
| [Workflow Automation Engine](workflow-automation-engine.md) | Workflow Engine | [PRD](../PRD%20-%20Product%20Vision.md) |
| [Natural Language Interface](natural-language-interface.md) | NL Interface | [PRD](../PRD%20-%20Product%20Vision.md) |

## Vision / Draft (foundational architecture PRDs, longer horizon)

| Use case | Domain | Source PRD |
| --- | --- | --- |
| [Vault Buddy Runtime & Embedded MCP Server](mcp-server-and-runtime.md) | AI Platform | [AI Platform & Agent Runtime](../prds/ai-platform.md) |
| [Plugin Platform & Specialized AI Agents](plugin-and-agent-platform.md) | AI Platform | [PRD](../PRD%20-%20Product%20Vision.md) |
| [Local MCP Hub Assistant](local-mcp-hub-assistant.md) | AI Platform | [Local MCP Hub](../prds/local-mcp-hub.md) |

---

## By source PRD

- [Vault Buddy PRD (Product Vision)](../PRD%20-%20Product%20Vision.md) — Desktop Companion, Vault Discovery & Open, Daily Note, Knowledge Search & Retrieval, Workflow Automation Engine, Natural Language Interface, Plugin Platform & Specialized AI Agents.
- [Knowledge Intake](../prds/knowledge-intake.md) — Meeting Recording, Voice Note Recording, Local Speech-to-Text Transcription, Companion Note & Follow-up Template, Recordings Browser, Re-transcription, Rename Recording, AI-Enriched Meeting Notes, Additional Capture Providers.
- [Task Management](../prds/task-management.md) — Per-Vault Task List, Task Tags & Todos, Aggregated Task Dashboard & Lists, AI-Assisted Task Management.
- [The Knowledge Lifecycle](../prds/knowledge-lifecycle.md) — vision framing for Knowledge Search & Retrieval and Workflow Automation Engine; no capability of its own beyond what those PRDs specify.
- [AI Platform & Agent Runtime](../prds/ai-platform.md) — Vault Buddy Runtime & Embedded MCP Server.
- [Local MCP Hub](../prds/local-mcp-hub.md) — Local MCP Hub Assistant.
- [Platform & Cross-Cutting Capabilities](../prds/platform-and-cross-cutting.md) — Software Auto-Update, Diagnostics/Crash Reporting & Recovery, plus the product-wide Non-Functional Requirements (formerly the main PRD's §15).

## Known documentation gaps found during this extraction

1. ~~**Software Auto-Update has no PRD at all**~~ — **fixed**: now covered by the [Platform & Cross-Cutting Capabilities PRD](../prds/platform-and-cross-cutting.md).
2. ~~**Task Management PRD status is stale**~~ — **fixed**: the PRD's status
   line now narrates the shipped v0.5.0 slice, and `AGENTS.md`'s IPC
   inventory lists the `task_commands::*` (and `search_commands::*`) entries.
3. **Rename Recording is unshipped-by-omission in the PRD's prose** — the feature exists and is safety-critical, but the Knowledge Intake PRD never narrates it as a use case or MVP feature.
4. **Desktop Audio / Custom recording modes** are named in the Knowledge Intake PRD's Recording Modes table but were never built — only Meeting and Voice Note modes ship.
5. ~~**Diagnostics & Crash Reporting has no PRD**~~ — **fixed**: found while drafting the Platform & Cross-Cutting PRD; it was documented only in `AGENTS.md`'s invariants. Now covered alongside Software Auto-Update.
