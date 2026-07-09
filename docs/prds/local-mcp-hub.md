# Local MCP Hub - Product Requirements Document

- **Status:** Draft
- **Version:** 0.1
- **Parent Product:** Vault Buddy
- **Related PRD:** [AI Platform & Agent Runtime](ai-platform.md)

Tracked as [Local MCP Hub Assistant](../use-cases/local-mcp-hub-assistant.md)
in [docs/use-cases/](../use-cases/README.md) — not started as of this
writing; see that note for how it relates to the AI Platform PRD's runtime.

---

## Executive Summary

The Local MCP Hub adds an optional local assistant layer to Vault Buddy.

When explicitly enabled and configured by the user, Vault Buddy can connect a cheap local LLM served by Ollama to user-configured MCP servers and let the buddy use those servers as tools for knowledge ingestion, retrieval, and lightweight workflow assistance.

This is not a mandatory runtime dependency. A default Vault Buddy installation must continue to start, build, and run exactly as it does today without Ollama, MCP server configuration, downloaded models, or any assistant state.

## Vision

Vault Buddy becomes a local MCP client hub for personal knowledge work.

The buddy remains the visible, lightweight desktop companion. Behind it, an optional local agent runtime can route user intent to configured MCP servers, call tools with explicit permission boundaries, and turn results into safe, reviewable knowledge proposals.

The first version should optimize for simple, local, tool-using interaction rather than advanced reasoning. The model selects tools and drafts responses. Vault Buddy owns permissions, confirmation, audit, and all sanctioned writes.

## Product Goals

### Primary Goals

- Provide an opt-in local assistant mode that is disabled by default.
- Let users configure Ollama as the local model provider.
- Let users configure MCP servers that Vault Buddy can connect to as a client.
- Discover MCP tools and expose a small, relevant subset to the local model.
- Support ingestion-oriented workflows that produce previews before anything is written.
- Keep all vault writes behind Vault Buddy-owned services and confirmation flows.
- Audit model requests, tool calls, approvals, failures, and user-confirmed writes.

### Secondary Goals

- Keep the internal model adapter replaceable so other local providers or embedded llama.cpp can be considered later.
- Support both stdio and streamable HTTP MCP transports over time.
- Provide a foundation for future workflow automation without introducing background autonomy in the first version.
- Keep the UX understandable for users who know what MCP servers are but do not want to operate a separate agent framework.

### Non Goals

- No cloud-hosted model dependency.
- No assistant feature enabled by default.
- No autonomous background agents in the first version.
- No unrestricted shell execution.
- No direct filesystem writes into Obsidian vaults through arbitrary MCP filesystem tools.
- No requirement that Vault Buddy ships, downloads, or manages an LLM model itself.
- No replacement for external power-user MCP clients such as Cursor or Claude Desktop.

## User Experience

The feature appears in Settings as an experimental local assistant section.

Before opt-in:

- No assistant entry point is shown in the main panel.
- No local model ports are probed.
- No MCP server processes are spawned.
- No assistant configuration is required.
- No failure in this feature can affect normal vault, capture, recording, transcription, or update flows.

After opt-in:

- The user configures the Ollama local endpoint.
- The user selects or enters a model name.
- The user adds MCP servers with command, arguments, environment references, or HTTP endpoint.
- Vault Buddy shows each server's health, discovered tools, and permission classification.
- The panel exposes a compact assistant view only when the feature is enabled and minimally configured.
- Tool calls that can write, execute, browse, call external services, or affect a vault require explicit confirmation.

The assistant should feel like asking the buddy to use connected tools, not like opening a full IDE-grade agent environment.

## Core User Stories

- As a user, I can enable the local assistant only when I want it, so Vault Buddy remains lightweight by default.
- As a user, I can configure Ollama as the model provider, so I control the local model and its resource usage.
- As a user, I can add MCP servers, so Vault Buddy can use external sources such as files, calendars, issue trackers, or knowledge stores.
- As a user, I can see what tools a server exposes before the assistant uses them.
- As a user, I can ask the buddy to gather information through connected MCP servers and produce an ingestion preview.
- As a user, I must approve any action that writes data, changes state, or touches my vault.
- As a user, I can disable the feature and know that managed MCP servers and assistant sessions stop.

## Functional Requirements

### Opt-in Activation

- The local assistant has a top-level `enabled: false` default.
- Disabled mode is a first-class state, not an error state.
- Enabling requires an Ollama endpoint and selected model before chat is available.
- Disabling stops managed MCP server processes, cancels active assistant sessions, clears transient tool state, and hides assistant UI entry points.
- The app must not fail startup if a previously configured Ollama endpoint or MCP server is unavailable.

### Model Provider

- The first provider is Ollama through its local HTTP API, defaulting to `http://localhost:11434`.
- The implementation should still hide Ollama behind an internal adapter so future providers do not affect MCP orchestration.
- The provider lists installed local models through Ollama when available.
- The provider sends chat requests through Ollama's `/api/chat` endpoint with MCP-derived tool definitions in the `tools` parameter.
- When Ollama returns `tool_calls`, Vault Buddy validates the call, applies permission rules, executes the approved MCP tool locally, appends the tool result to the conversation, and sends a follow-up chat request for the final answer.
- Streaming responses are preferred because Ollama supports streaming chat and streaming tool calls, but non-streaming mode is acceptable for the first spike.
- Tool-call parsing must be validated deterministically before any MCP call is executed.

### Candidate Ollama Models

Research summary: Ollama supports tool calling for models that expose the tools capability. Current Ollama documentation demonstrates `qwen3` with tools, and Ollama's tool-calling blog names Qwen 3, Devstral, Qwen2.5, Qwen2.5 Coder, Llama 3.1, Llama 4, and other tool-capable models. Community guidance consistently points to Qwen 2.5+, Llama 3.1+, and Mistral-family instruct models as practical local tool-calling choices.

Recommended first choices:

| Tier | Model | Why | Notes |
| --- | --- | --- | --- |
| Default cheap | `qwen2.5:7b` | Strong JSON adherence and good small-model tool use | Good default for constrained machines. |
| Current cheap | `qwen3` or a small Qwen 3 tag | Official Ollama docs use Qwen 3 for tool-calling examples | Validate exact tag and thinking behavior in the spike. |
| Reliable small | `llama3.1:8b` | Widely supported Ollama tool-calling baseline | Good fallback if Qwen performs poorly. |
| Better reliability | `qwen2.5:14b` | More reliable tool selection than 7B-class models | Needs more RAM/VRAM; optional recommendation. |
| Specialized dev tools | `devstral` or `qwen2.5-coder:7b` | Better for code-oriented MCP servers | Not the default for general knowledge ingestion. |
| Multilingual/general alternative | `mistral-nemo` or `mistral-small` | Useful multilingual instruct family | Heavier than the default small tier. |

Models below roughly 7B parameters may be offered as experimental only. They can answer simple prompts cheaply, but tool selection and argument quality are likely to be unreliable for MCP orchestration. The first product slice should keep the active tool set small, prefer read-only tools during evaluation, and include an internal evaluation harness before recommending a default model.

### MCP Server Registry

- MCP server configuration is stored app-side, not in a vault.
- Each server has a user-visible name, enabled flag, transport type, command or endpoint, arguments, environment references, timeout, and permission policy.
- Vault Buddy can connect to enabled servers only after the local assistant is enabled.
- Vault Buddy discovers tools and resources and caches enough metadata to display them in settings.
- Server stderr and startup failures are logged through existing diagnostics.
- Long-running or hung server/tool calls must time out and surface a user-visible failure.

### Tool Permissions

Each discovered tool is classified before use:

- Read-only
- Write or mutate
- External side effect
- Shell or process execution
- Browser or network access
- Vault-affecting
- Unknown

Read-only tools may be allowed with lower friction once the user trusts a server. All other classes require explicit confirmation before execution. Unknown tools are treated as risky.

### Assistant Interaction

- The assistant view is available only after opt-in and minimal configuration.
- The assistant shows model status and MCP server status.
- The assistant renders tool-call chips with server name, tool name, risk class, and state.
- Risky tool calls pause for user approval.
- Failed tool calls produce visible, non-fatal errors.
- The assistant should expose a narrow tool set per turn where possible, reducing the burden on small local models.

### Ingestion Preview

Ingestion flows produce a preview before any write:

- Source and server used
- Suggested title
- Extracted facts or summary
- Suggested target vault and folder
- Proposed note body
- Required write action

The user must confirm before Vault Buddy writes anything. Confirmed writes go through Vault Buddy-owned services, not arbitrary MCP filesystem tools.

## Architecture

The recommended architecture is a Rust-owned local agent runtime behind Tauri IPC.

```mermaid
flowchart LR
  User[User] --> BuddyPanel[Buddy Panel]
  Settings[Settings Opt In] --> BuddyPanel
  BuddyPanel --> TauriCommands[Tauri Commands]
  TauriCommands --> AgentRuntime[Local Agent Runtime]
  AgentRuntime --> ModelProvider[Local Model Provider]
  AgentRuntime --> McpHub[MCP Client Hub]
  McpHub --> McpServerA[MCP Server A]
  McpHub --> McpServerB[MCP Server B]
  AgentRuntime --> VaultBuddyServices[Vault Buddy Services]
  VaultBuddyServices --> Obsidian[Obsidian Via Safe Chokepoints]
```

The Rust side owns process supervision, logging, permissions, audit, model calls, MCP clients, and safe handoff to Vault Buddy services. The Vue side owns settings, status display, chat interaction, approval prompts, and previews.

The first implementation should use Ollama rather than embedding model inference in the Tauri process. This keeps the app lightweight and avoids making model runtime setup part of the core Vault Buddy installation. The boundary should still be an internal `ModelProvider`-style adapter so later provider changes stay isolated.

## Safety And Privacy

- All model and MCP activity is local unless the user configures an MCP server that calls external services.
- Vault Buddy must make external side effects visible before execution.
- No MCP server receives broad vault write capability by default.
- Every tool call is audited with server, tool, argument summary, permission decision, approval state, result state, and timestamp.
- Sensitive arguments should be summarized or redacted in logs when possible.
- Managed MCP server processes should inherit only explicitly configured environment values.
- The assistant must degrade gracefully when Ollama or configured MCP servers are offline.

## Phased Delivery

### Phase 1 - Inert Configuration

- Add disabled-by-default configuration.
- Add settings UI for opt-in, Ollama endpoint, model name, and server definitions.
- Verify normal startup does not initialize the assistant runtime.

### Phase 2 - Read-only MCP Spike

- Connect to one configured stdio MCP server behind the opt-in gate.
- List tools and display health state.
- Call a harmless read-only tool from Rust and log the result.

### Phase 3 - Ollama Model Adapter

- Add an Ollama model provider adapter.
- Support model listing and a configurable endpoint.
- Send a small tool set to the model.
- Parse and validate tool calls.
- Test with `qwen2.5:7b`, one Qwen 3 tag, and `llama3.1:8b` before choosing the default recommendation in the UI.

### Phase 4 - Assistant UI

- Add a compact assistant panel view.
- Show streaming or incremental responses if supported.
- Render tool-call state and approval prompts.

### Phase 5 - Ingestion Preview

- Convert tool results into structured ingestion previews.
- Require confirmation before writes.
- Route confirmed writes through Vault Buddy-owned services.

### Phase 6 - Audit And Hardening

- Add audit records.
- Add timeouts and process cleanup.
- Add tests for disabled-by-default behavior, config parsing, permission classification, feature-gated UI, approval prompts, and preview confirmation.

## Success Metrics

- Vault Buddy starts and functions normally with the feature disabled.
- Users can enable the feature and connect Ollama plus at least one MCP server.
- The hub can discover and display tools from a configured MCP server.
- The model can successfully request a read-only tool call in a constrained test flow.
- Risky tools are never executed without confirmation.
- Confirmed ingestion writes preserve existing vault safety invariants.
- Disabling the feature reliably stops managed processes and hides assistant UI.

## Open Questions

- Which MCP SDK should be used in Rust for the first spike?
- Which Ollama model should the UI recommend first after empirical testing on typical Windows hardware?
- Should MCP server configuration support importing Claude Desktop-style MCP JSON?
- How much conversation history should be persisted, if any?
- Should audit records live in the existing app log, a separate assistant audit file, or both?

## Research References

- [Ollama Tool Calling](https://docs.ollama.com/capabilities/tool-calling)
- [Ollama API Documentation](https://docs.ollama.com/api)
- [Ollama Streaming Tool Calling Blog](https://ollama.com/blog/streaming-tool)
