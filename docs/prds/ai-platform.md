# AI Platform & Agent Runtime — The Intelligence Layer

- **Status:** Product Vision
- **Version:** 1.0
- **Parent Product:** Vault Buddy

Tracked as [Vault Buddy Runtime & Embedded MCP
Server](../use-cases/mcp-server-and-runtime.md) and [Plugin Platform &
Specialized AI Agents](../use-cases/plugin-and-agent-platform.md) in
[docs/use-cases/](../use-cases/README.md) — not started as of this writing.

---

## Executive Summary

Vault Buddy is more than a desktop companion.

It is the intelligent runtime that connects users, knowledge, tools and AI agents through a unified local platform.

Rather than embedding AI into individual features, Vault Buddy exposes every capability through a common runtime that can be consumed by humans, desktop UI, workflows and autonomous agents alike.

The AI Platform provides the foundation for a future where any AI assistant can securely interact with a user's personal knowledge system through a consistent, permission-aware interface.

The first public interface of this platform is the Vault Buddy MCP Server.

## Vision

Every capability should be accessible to both humans and AI.

Vault Buddy becomes the local execution environment for knowledge work.

Humans interact through the desktop companion. AI interacts through standardized APIs. Both operate on the same knowledge model.

## Mission

Provide a secure, local-first runtime that enables AI systems to understand, retrieve, create and organize knowledge while respecting user ownership, permissions and transparency.

## Problem Statement

Today's AI assistants are isolated.

They know very little about:

- local files
- project context
- personal knowledge
- current work
- workflows
- ongoing meetings
- tasks
- documentation

Every integration requires custom tools. Every application exposes different APIs. Every workflow must be recreated.

Vault Buddy provides a unified runtime that centralizes these capabilities and exposes them through a consistent interface.

## Product Philosophy

Applications should no longer expose isolated APIs. Applications should expose capabilities.

AI should not know how Obsidian stores Markdown. AI should simply ask:

> "Create a meeting note."

Vault Buddy decides how that should happen.

The runtime owns implementation. AI expresses intent.

## Product Goals

### Primary Goals

- Provide a unified runtime for all Vault Buddy capabilities.
- Expose every capability through MCP.
- Keep AI integrations completely local.
- Enable any compliant AI client.
- Separate business logic from UI.
- Support future communication protocols.

### Secondary Goals

- Plugin ecosystem
- Agent ecosystem
- Workflow orchestration
- Local AI integration
- Distributed agents

### Non Goals (MVP)

- Cloud execution
- Remote multi-user collaboration
- Autonomous internet access
- Persistent autonomous agents
- External authentication providers

## Runtime Architecture

Vault Buddy consists of independent capability engines.

```text
                    Vault Buddy Runtime

                           │

        ┌──────────────────┼──────────────────┐

 Knowledge Engine     Workflow Engine     Task Engine

 Capture Engine       Search Engine       Vault Engine

 AI Engine            Plugin Engine       Settings Engine

                           │

                     Service Layer

                           │

                   Runtime API

                           │

        ┌──────────────────┼────────────────────┐

         MCP             REST            Future APIs
```

Every engine is internally accessible through a common service architecture. External protocols become adapters.

## Runtime Principles

Every capability:

- owns its domain
- exposes services
- defines permissions
- emits events
- can be automated
- can be consumed by AI

No capability communicates directly with external AI. Everything flows through the runtime.

## Human and AI Parity

Every action available in the user interface should also be available to AI.

Every AI action should be executable by the UI.

The runtime becomes the single source of truth.

## Capability Domains

### Knowledge Intake

- Start recording
- Stop recording
- Take screenshot
- Capture clipboard
- Import file

### Knowledge Processing

- Transcribe
- Summarize
- Extract metadata
- Generate tags
- Link notes

### Task Management

- Create task
- Update task
- Complete task
- Search tasks
- Aggregate tasks

### Knowledge Retrieval

- Search
- Semantic search
- Find related notes
- Timeline
- Graph traversal

### Workflow Engine

- Run workflow
- Schedule workflow
- Pause workflow
- Cancel workflow
- Inspect execution

### Vault Management

- Discover Vaults
- Open Vault
- Create note
- Read note
- Update note
- Properties
- Templates

## MCP Server

The first external interface of the runtime is an embedded MCP Server.

The server is started locally by Vault Buddy. No external dependencies are required.

Every exposed capability is implemented by calling internal runtime services.

The MCP Server never accesses the filesystem directly. It communicates exclusively with runtime services.

### MCP Principles

The MCP server exposes capabilities instead of implementation details.

Example:

Instead of `Create Markdown File`, AI calls `Create Task`.

The runtime determines:

- filename
- metadata
- folder
- template
- links
- indexing

This keeps business rules centralized.

## Runtime Service Layer

Every domain exposes typed services.

Example:

- KnowledgeService
- TaskService
- VaultService
- CaptureService
- WorkflowService
- SearchService
- SettingsService
- NotificationService

These services are shared by:

- Desktop UI
- Workflow Engine
- Plugins
- MCP
- Future REST API
- Future SDK

## Internal Event Bus

Every important action emits domain events.

Examples:

- TaskCreated
- TaskCompleted
- RecordingStarted
- RecordingFinished
- VaultOpened
- WorkflowStarted
- WorkflowFinished
- MeetingProcessed
- KnowledgeCaptured

Events enable automation without tight coupling.

## Permissions

Every capability declares required permissions.

Examples:

- Read Vault
- Write Vault
- Capture Audio
- Capture Screen
- Execute Workflow
- Delete Notes

External AI receives only explicitly granted permissions.

## AI Clients

The runtime should support any MCP-compatible client.

Examples:

- Claude Code
- Cursor
- Codex
- OpenAI
- Gemini
- Continue
- Aider
- OpenHands
- Custom Agents
- Future clients

The runtime remains independent from any individual AI provider.

## Plugin Platform

Plugins register new capabilities.

Examples:

- Git Plugin
- Email Plugin
- Calendar Plugin
- Jira Plugin
- Browser Plugin
- Filesystem Plugin
- Home Assistant Plugin

Plugins automatically become available to workflows and, if permitted, to the MCP server.

## Workflow Integration

Workflows orchestrate runtime services.

Example — Morning Routine:

1. Open Daily Note
2. Show Today's Tasks
3. Review Calendar
4. Summarize Yesterday
5. Generate Focus Plan

Every workflow is equally executable by:

- User Interface
- Automation
- MCP
- AI Agent

## Security Principles

- Local-first execution
- Permission-based capability access
- No unrestricted shell execution
- No direct filesystem access for external clients
- Audit log for every external request
- User confirmation for destructive actions
- Encrypted local configuration
- Transparent execution

## Non Functional Requirements

### Performance

| Metric | Target |
| --- | --- |
| Runtime startup | < 2 seconds |
| MCP request latency | < 100 ms (excluding long-running operations) |
| Service invocation | < 20 ms |

### Reliability

- Offline capable
- Graceful degradation
- Automatic restart
- Event replay support

### Extensibility

- New capability engines require no MCP changes.
- New protocols require no business logic changes.
- Business rules remain isolated within domains.

## Product Roadmap

### Phase 1

- Runtime architecture
- Service layer
- Knowledge services
- Task services
- Baseline permissions and audit log
- Embedded MCP server

### Phase 2

- Workflow engine
- Internal event bus
- Fine-grained permission scopes
- Plugin API

### Phase 3

- Agent orchestration
- Local LLM integration
- Semantic memory
- Planning engine

### Phase 4

- Multi-agent collaboration
- Distributed workflows
- Background reasoning
- Continuous knowledge processing

## Success Metrics

- Number of exposed runtime capabilities
- MCP tool adoption
- Average service latency
- Workflow execution success rate
- Plugin ecosystem growth
- AI task completion rate
- Runtime stability
- Local-only execution percentage

## Long-Term Vision

Vault Buddy evolves into the local operating layer for intelligent knowledge work.

Applications no longer expose isolated functionality. Instead, they contribute capabilities to a shared runtime.

Humans, workflows and AI agents all interact through the same service architecture.

The desktop companion becomes the visible face of a much larger platform.

The Vault Buddy Runtime becomes the trusted foundation for personal AI.

One Runtime. One Knowledge Model. One Capability Platform. Unlimited Intelligent Clients.
