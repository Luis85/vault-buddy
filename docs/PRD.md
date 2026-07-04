# Vault Buddy — Product Requirements Document (PRD)

- **Version:** 1.0 (Vision Draft)
- **Status:** Product Discovery
- **Product:** Vault Buddy
- **Platform:** Windows (MVP)
- **Technology Stack:** Tauri 2 · Vue 3 · TypeScript · Rust
- **Product Owner:** TBD

This PRD is underpinned by two foundational vision documents:

- [The Knowledge Lifecycle](prds/knowledge-lifecycle.md) — what happens to knowledge as it moves through Vault Buddy.
- [AI Platform & Agent Runtime](prds/ai-platform.md) — how humans, workflows and AI agents access those capabilities.

---

## 1. Executive Summary

Vault Buddy is an AI-native desktop companion that transforms how knowledge workers interact with their digital workspace.

Instead of opening applications, searching through folders, remembering keyboard shortcuts, or switching between dozens of tools, users simply interact with a small animated desktop companion that understands context, orchestrates workflows and performs actions on their behalf.

Vault Buddy begins as the best desktop companion for Obsidian.

Over time, it evolves into an extensible desktop operating layer capable of orchestrating knowledge, AI agents and local developer tools through a secure, local-first architecture.

Unlike cloud-based assistants, Vault Buddy is built around one core principle:

> Your knowledge stays yours.

Every interaction is transparent, auditable and fully under the user's control.

---

## 2. Product Vision

Become the AI-native operating layer for knowledge work.

Vault Buddy provides a persistent desktop companion that connects humans, knowledge and AI through a delightful, trustworthy and extensible experience.

The desktop itself becomes the primary interface for interacting with information.

Instead of navigating software, users simply express intent.

Vault Buddy translates intent into safe, contextual actions.

---

## 3. Mission

Empower knowledge workers to focus on thinking instead of operating software.

Vault Buddy should make interacting with personal knowledge feel as natural as talking to a trusted colleague.

---

## 4. Product Principles

### Local First

Everything runs locally whenever possible.

User knowledge remains under the user's ownership.

Cloud services are optional.

### Human in Control

Vault Buddy never performs destructive actions without explicit confirmation.

The assistant recommends.

The user decides.

### AI Native

AI is not an additional feature.

AI is the primary interaction model.

### Extensible

Everything should be modular.

Every capability should be replaceable.

Every integration should become a plugin.

### Delightful

Software should feel alive.

Vault Buddy should have personality without becoming distracting.

### Safe by Default

Security is more important than convenience.

Every action is transparent.

Every action is auditable.

---

## 5. Problem Statement

Knowledge workers spend large amounts of time performing repetitive operations:

- Opening notes
- Finding documentation
- Managing tasks
- Navigating projects
- Launching applications
- Executing scripts
- Searching information
- Context switching

Although powerful automation technologies exist, they are fragmented across CLIs, plugins, APIs and scripts.

Most users never benefit from them.

Vault Buddy unifies these capabilities behind a natural conversational interface.

---

## 6. Product Positioning

Vault Buddy is not another chatbot.

Vault Buddy is not another note-taking application.

Vault Buddy is not another AI wrapper.

Vault Buddy is an intelligent desktop operating layer that orchestrates tools through natural interaction.

Think:

- Clippy
- Raycast
- Obsidian
- MCP
- Local AI
- Desktop Automation

combined into a single experience.

---

## 7. Long-Term Vision

Vault Buddy starts with Obsidian.

Eventually it becomes the central orchestrator for the user's digital workspace.

Future integrations include:

- Obsidian
- Git
- Cursor
- VS Code
- Claude Code
- Codex
- Terminal
- Browser
- Outlook
- Jira
- GitHub
- Google Workspace
- Local LLMs
- MCP Servers
- Smart Home
- Voice Interfaces

Vault Buddy becomes the operating system for knowledge work.

---

## 8. Target Audience

### Primary

- Developers
- Product Managers
- Delivery Managers
- Architects
- Consultants
- Researchers
- Engineers
- Technical Writers
- PKM Enthusiasts

### Secondary

- Students
- Creators
- Writers
- Teams using Obsidian
- AI enthusiasts

---

## 9. User Experience Vision

Vault Buddy lives directly on the Windows desktop.

The companion is always available.

It reacts naturally.

It moves.

It sleeps.

It celebrates.

It thinks.

It provides emotional feedback while remaining unobtrusive.

The interaction should feel closer to having a tiny assistant sitting on the desktop than opening another application.

---

## 10. Product Pillars

### Intelligent Assistant

Natural language interaction.

Context awareness.

Conversation.

Recommendations.

### Knowledge Layer

Understanding notes.

Projects.

Tasks.

Documents.

Relationships.

### Automation Layer

Executing workflows.

Launching tools.

Managing projects.

Creating documents.

Running scripts.

### Agent Layer

Orchestrating AI agents.

Coordinating tools.

Delegating work.

Monitoring execution.

### Platform Layer

Plugins.

Extensions.

Community ecosystem.

---

## 11. Core Capabilities

### Desktop Companion

- Animated character
- Emotional states
- Drag & Drop
- Transparent window
- Always on top
- Multiple monitors
- System tray
- Global hotkeys
- Startup with Windows

### Natural Language Interface

- Chat
- Quick commands
- Intent recognition
- Contextual suggestions
- Conversation history
- Command history

### Obsidian Integration

- Vault discovery
- Vault switching
- Daily Notes
- Templates
- Search
- Tasks
- Metadata
- Properties
- Tags
- Canvas
- Commands
- Plugins
- Workspace management

### Knowledge Search

- Keyword search
- Semantic search
- Tag search
- Graph exploration
- Backlinks
- Recent activity
- Related notes

### Workflow Automation

- Morning routine
- Meeting preparation
- Research workflow
- Writing workflow
- Project startup
- Knowledge capture
- Task review
- Daily shutdown

### AI Features

- Summarization
- Translation
- Brainstorming
- Writing assistance
- Task extraction
- Meeting minutes
- Knowledge linking
- Tag recommendations
- Context understanding

---

## 12. Plugin Architecture

Vault Buddy capabilities are organized as Plugins.

Examples:

- Obsidian Plugin
- Git Plugin
- GitHub Plugin
- Jira Plugin
- Terminal Plugin
- Browser Plugin
- Filesystem Plugin
- Calendar Plugin
- Email Plugin
- Cursor Plugin
- Claude Plugin
- MCP Plugin
- Voice Plugin

Each plugin provides:

- Actions
- Commands
- Events
- Permissions
- Settings
- Documentation

---

## 13. Agent Architecture

Future versions introduce specialized agents.

Examples:

- Research Agent
- Documentation Agent
- Writing Agent
- Planning Agent
- Architecture Agent
- Meeting Agent
- Developer Agent
- Review Agent
- Automation Agent

Vault Buddy acts as the orchestrator.

---

## 14. Functional Requirements

### Vault Management

- Detect installed Obsidian
- Detect CLI
- Discover Vaults
- Open Vault
- Switch Vault
- Multiple Vault support
- Favorite Vaults
- Recent Vaults

### Notes

- Create
- Read
- Update
- Rename
- Duplicate
- Archive
- Move
- Delete
- Template support
- Metadata editing

### Daily Notes

- Open
- Create
- Append
- Review
- Archive

### Tasks

- Create
- Complete
- Schedule
- Prioritize
- Filter
- Search
- Move
- Recurring Tasks

See the [Task Management capability PRD](prds/task-management.md) for the detailed requirements.

### Search

- Notes
- Tags
- Properties
- Links
- Tasks
- Files
- Templates
- Commands

### Command Execution

- Safe CLI execution
- URI execution
- Plugin execution
- Workflow execution

### Workflow Engine

- Visual workflows
- Triggers
- Actions
- Conditions
- Variables
- Reusable templates
- Scheduling

---

## 15. Non Functional Requirements

### Performance

- Startup < 2 seconds
- Command latency < 500 ms
- Memory usage < 250 MB
- Idle CPU < 1%

### Reliability

- Offline capable
- Automatic recovery
- Graceful failures
- Crash reporting (optional)

### Security

- Local-first
- Encrypted configuration
- Permission model
- Command allowlists
- Confirmation dialogs
- Audit log
- Read-only mode
- Secrets management

### Accessibility

- Keyboard navigation
- Screen reader compatibility
- High contrast
- Configurable scaling
- Reduced motion mode

---

## 16. Technical Architecture

### Frontend

- Vue 3
- TypeScript
- Pinia
- VueUse
- Floating UI

### Desktop

- Tauri 2

### Native Layer

- Rust

### Communication

- Tauri Commands

### Animation

- Rive
- Lottie
- Sprite Sheets

### Styling

- TailwindCSS

### Testing

- Vitest
- Playwright
- Rust Tests

---

## 17. High-Level Architecture

```
                 User
                  │
          Natural Language
                  │
          Intent Recognition
                  │
        Permission & Safety
                  │
        Workflow Orchestrator
                  │
      ┌───────────┼────────────┐
      │           │            │
   Plugins     AI Agents    Automations
      │           │            │
      └───────────┼────────────┘
                  │
          Tool Execution Layer
                  │
   ┌──────────────┼────────────────┐
   │              │                │
Obsidian      Local Tools      MCP Servers
   │              │                │
   └──────────────┼────────────────┘
                  │
          User Knowledge
```

---

## 18. Product Roadmap

### Phase 1 — Foundation

- Desktop Companion
- Transparent Window
- Animated Character
- Obsidian CLI
- Vault Detection
- Daily Notes
- Search
- Tasks

### Phase 2 — Productivity

- Natural Language
- Quick Commands
- Templates
- Workflow Engine
- Semantic Search
- Context Awareness

### Phase 3 — Intelligence

- Local AI
- Knowledge Graph
- Recommendations
- Meeting Assistant
- Writing Assistant
- Research Assistant

### Phase 4 — Platform

- Plugin SDK
- Plugin Marketplace
- Agent SDK
- Workflow Marketplace
- Community Extensions

### Phase 5 — Knowledge Operating System

- Multi-agent orchestration
- Background automation
- Cross-application context
- Workspace intelligence
- Personal knowledge graph
- Desktop operating layer

---

## 19. Success Metrics

- Daily Active Users
- Weekly Active Users
- Vault Commands per Day
- Average Response Time
- Search Success Rate
- Automation Usage
- User Satisfaction
- Crash-Free Sessions
- Average Time Saved
- Plugin Adoption
- Agent Usage
- Workflow Executions

---

## 20. Risks

- Obsidian CLI evolution
- Windows desktop limitations
- Performance on large vaults
- AI hallucinations
- Security of external tools
- Plugin compatibility
- Over-automation
- User trust

---

## 21. Future Opportunities

- Voice-first interaction
- Wearables
- Mobile companion
- Team collaboration
- Shared knowledge agents
- Cloud synchronization
- Enterprise edition
- Team knowledge graphs
- Marketplace economy
- AI-generated workflows

---

## 22. Product Statement

Vault Buddy is more than an assistant.

It is a trusted desktop companion that transforms software from something users operate into something that collaborates with them.

The long-term ambition is simple:

> Make interacting with knowledge as effortless as having a conversation.
