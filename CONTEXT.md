# Vault Buddy

Vault Buddy is a local-first, AI-native desktop companion for knowledge work. It is evolving from a desktop companion for Obsidian into a Knowledge Operating Layer that accompanies knowledge through the Knowledge Lifecycle: Capture → Process → Organize → Act → Retrieve → Automate → Learn.

## Language

**Vault**:
An Obsidian vault — a folder on disk holding a user's Markdown notes — that Vault Buddy discovers, opens, and (in later capabilities) writes Tasks into directly.
_Avoid_: Workspace, folder, notebook

**Buddy**:
The small animated character that lives on the desktop and is the visible entry point to Vault Buddy; clicking it opens the vault panel.
_Avoid_: Companion, mascot, widget

**Daily Note**:
The Obsidian note for the current date, opened or created via an `obsidian://` URI. Vault Buddy delegates this to Obsidian rather than writing the note itself.
_Avoid_: Journal entry, today's note

**Capture**:
The act of recording a piece of knowledge (voice, screenshot, clipboard, meeting, etc.) as the first stage of the Knowledge Lifecycle, before it has been turned into structured knowledge.
_Avoid_: Recording — a Capture is not necessarily audio

**Knowledge Lifecycle**:
The seven-stage journey every piece of information follows inside Vault Buddy: Capture → Process → Organize → Act → Retrieve → Automate → Learn. Completing an action produces new knowledge, making the journey continuous.
_Avoid_: Workflow — a Workflow is one concrete automation; the Lifecycle is the overarching journey every capability serves

**Task**:
A first-class knowledge object, stored as its own Markdown document inside a Vault's Task Folder, connected via frontmatter to the notes, Projects, or Captures it originated from, and optionally to a parent Task — letting Tasks form hierarchies of child Tasks. Progress inside a single Task is tracked with Todos in its body; a Todo is never itself a Task, and a Note carrying only a Task Tag is not a Task either.
_Avoid_: Task Note (redundant — a Task is always a note, "Task" alone is canonical), subtask (ambiguous — say "child Task" for a Task-level hierarchy or "Todo" for an inline checklist line), checklist item

**Task Tag**:
A tag placed on a Note whose frontmatter type is not Task, marking that Note itself as something to be done. The Note keeps its own type, location and purpose — Task Management surfaces it as actionable without relocating it into the Task Folder or granting it Task properties (Status, Priority, Parent Task, …).
_Avoid_: Task, tagged Task — a Task-tagged Note is not a Task

**Todo**:
An inline checklist line, written `- [ ] description`, inside any Note's body — a Task, a Task-tagged Note, or any other Note — used to track granular progress or present a checklist for that Note. A Todo has no frontmatter, no file of its own, and no identity outside the Note containing it.
_Avoid_: Task, subtask, todo item

**Task List**:
A named grouping of Tasks (e.g. Inbox, Today, Someday) held as metadata on a Task, not as a physical folder.
_Avoid_: Folder, category, board

**Project**:
Task metadata linking a Task to the larger body of notes or work it belongs to.
_Avoid_: Epic, initiative

**Runtime**:
The local service layer (Knowledge Engine, Task Engine, Workflow Engine, and peers) that owns all business logic. The desktop UI, the MCP Server, and Workflows are all just clients of the Runtime — none of them re-implement its logic.
_Avoid_: Backend, server — the Runtime is embedded and local, not a remote service

**Capability**:
A unit of Runtime behavior (e.g. "Create Task") exposed identically to the desktop UI and to AI clients, so callers express intent without knowing implementation details like filenames or folder layout.
_Avoid_: Endpoint, API, action

**MCP Server**:
The embedded, local Model Context Protocol server that exposes Runtime Capabilities to external AI clients, gated by explicitly granted Permissions and an audit log.
_Avoid_: API server, backend

**Permission**:
An explicit grant (e.g. "Read Vault", "Capture Audio") that an AI client must hold before the Runtime will execute a Capability on its behalf.
_Avoid_: Scope, role

**Workflow**:
A named, repeatable orchestration of Runtime Capabilities (e.g. "Morning Routine") that the UI, a schedule, the MCP Server, or an AI agent can all trigger the same way.
_Avoid_: Automation, script

**Plugin**:
An integration with an external tool or service (e.g. Git, Email, Calendar) that registers new Capabilities with the Runtime, automatically available to Workflows and, if permitted, to the MCP Server.
_Avoid_: Extension, add-on

**Skill (Claude Code)**:
A packaged process definition under `.claude/skills/` that Claude Code follows when developing Vault Buddy itself (e.g. `grill-with-docs`). This is contributor tooling, unrelated to the product's own Plugin/Capability vocabulary above.
_Avoid_: Plugin, Capability — those describe the product; a Skill describes how the product is built
