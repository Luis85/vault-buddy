---
type: UseCase
status: planned
domain: workflow-engine
source_prd: "docs/PRD - Product Vision.md"
tags: [use-case, workflow-engine]
---

# Workflow Automation Engine

> Visual, triggerable workflows (Morning Routine, Meeting Preparation, Research Workflow, Project Startup, Daily Shutdown, ...) that chain runtime capabilities together, executable equally by the UI, a schedule, MCP, or an AI agent.

## Source

Main PRD, [§11 Core Capabilities → Workflow Automation](../PRD%20-%20Product%20Vision.md), [§14 Functional Requirements → Workflow Engine](../PRD%20-%20Product%20Vision.md) (visual workflows, triggers, actions, conditions, variables, reusable templates, scheduling), and [§18 Roadmap → Phase 2 — Productivity](../PRD%20-%20Product%20Vision.md). Knowledge Lifecycle PRD, [Lifecycle Stage 6 — Workflow Automation](../prds/knowledge-lifecycle.md) (Morning Routine, Meeting Preparation, Research Sessions, Release Planning, Daily Reviews, Inbox Processing, Documentation Generation, Knowledge Reviews). AI Platform PRD, [Workflow Integration](../prds/ai-platform.md) example ("Morning Routine": open daily note → show tasks → review calendar → summarize yesterday → generate focus plan).

## Status: Not started

No workflow definition format, trigger system, or execution engine exists in the codebase. This is a cross-cutting orchestration layer that presupposes several of the runtime services described in [MCP Server & Runtime](mcp-server-and-runtime.md) already existing.

## Related use-cases

- [Knowledge Search & Retrieval](knowledge-search-and-retrieval.md)
- [MCP Server & Runtime](mcp-server-and-runtime.md)
- [Plugin & Agent Platform](plugin-and-agent-platform.md)
