# Task Management — Capability PRD

- **Status:** Draft — the per-vault task experience has shipped: creation,
  listing, and completion in v0.5.0; archive + open-task counter in v0.5.1;
  due dates, priority, date buckets, inline editing, open-in-Obsidian and
  filtering in v0.5.2; Obsidian-compatible tags on Tasks (chips, tag filter,
  tag grouping) in v0.5.3. Cross-vault aggregation, lists, the dashboard,
  templates, Task Tags on non-Task notes / Todos, and the AI features below
  remain unbuilt. See [Per-Vault Task List](../use-cases/per-vault-task-list.md).
- **Version:** 1.0
- **Parent Product:** Vault Buddy

Use cases extracted from this PRD, with shipping status: [Per-Vault Task
List](../use-cases/per-vault-task-list.md) (shipped v0.5.0, extended
through v0.5.3), [Task Tags &
Todos](../use-cases/task-tags-and-todos.md), [Aggregated Task Dashboard &
Lists](../use-cases/aggregated-task-dashboard-and-lists.md), [AI-Assisted
Task Management](../use-cases/ai-assisted-task-management.md) (all
planned). See [docs/use-cases/](../use-cases/README.md) for the full
catalog.

---

## Vision

Every task deserves context.

Vault Buddy transforms task management from maintaining isolated checklists into managing actionable knowledge.

Tasks become first-class knowledge objects that are directly connected to projects, notes, meetings, recordings and documents inside an Obsidian Vault.

Users should be able to create, organize and complete tasks without ever opening Obsidian.

## Domain Model: Task vs Task Tag vs Todo

Vault Buddy draws a hard line between three concepts that everyday language flattens into "task":

- **Task** — always its own Markdown document, with frontmatter, living in the Vault's Task Folder. A Task optionally names a parent Task, so Tasks can form hierarchies (a large Task broken into child Tasks) without ever leaving the file system.
- **Task Tag** — a tag on any other Note (one whose frontmatter type is not Task) marking that Note itself as something to be done — a meeting note awaiting follow-up, a project note with an open action. The Note keeps its own type, location and purpose; it does not move into the Task Folder and does not gain Task properties like Status or Parent Task. Task Management surfaces it without owning it.
- **Todo** — an inline checklist line (`- [ ] description`) inside any Note's body — a Task, a Task-tagged Note, or any other Note — used to track granular progress or present a checklist. A Todo has no frontmatter and no identity outside the Note containing it.

Every capability below that says "Task" means the Markdown document; where Task Tags or Todos are involved, this PRD says so explicitly.

## Mission

Provide a fast, desktop-native task management experience that integrates seamlessly with every configured Vault while remaining completely independent from the Obsidian application.

## Problem Statement

Knowledge workers create tasks continuously throughout the day.

Unfortunately these tasks become fragmented across many systems:

- Sticky notes
- Teams chats
- Outlook
- Jira
- Markdown files
- Daily Notes
- Meeting notes
- Memory

Capturing and organizing tasks often interrupts the current workflow.

Users should be able to create and manage tasks instantly from the desktop while preserving all contextual knowledge inside their Vault.

## Goals

### Primary Goals

- Create tasks in less than 10 seconds.
- Never require Obsidian to be opened.
- Store tasks directly inside the configured Vault.
- Aggregate tasks across multiple Vaults.
- Support multiple personal task lists.
- Preserve project context.

### Secondary Goals

- AI-assisted task creation.
- Automatic prioritization.
- Task extraction from recordings.
- Task scheduling.
- Workflow automation.

### Non Goals (MVP)

- Team collaboration
- Kanban boards
- Gantt charts
- Time tracking
- Notifications based on cloud services
- Mobile synchronization

## User Experience

Every Vault entry provides three primary actions.

- 📅 Daily Note
- ➕ New Task
- 🎙 Capture

Selecting New Task immediately opens a lightweight modal. No Obsidian window is required.

### Quick Task Modal

The modal is optimized for speed.

**Fields**

- Title (required)
- Description (optional)
- Due Date (optional)
- Priority (optional)
- Task List (optional)
- Tags (optional)
- Project (optional)
- Estimated Effort (optional)

**Buttons**

- Create
- Create & New
- Cancel

The default action should require only entering a title.

### Vault Settings

Each Vault defines its own task configuration.

**Task Storage**

- Task Folder
- Archive Folder
- Template
- Naming Convention
- Default Tags
- Default Priority

**Lists**

- Inbox
- Next
- Today
- Waiting
- Someday
- Completed
- Custom Lists

**Defaults**

- Default Task List
- Default Priority
- Default Due Date
- Default Assignee

## Task Model

Every task is stored as an individual Markdown document.

The filename should be generated automatically.

Example:

```
2026-07-04-prepare-release-cutover.md
```

Each task contains structured metadata.

**Example Properties**

- Title
- Status
- Priority
- Created
- Due Date
- Completed
- Vault
- Tags
- Project
- Task List
- Parent Task
- Related Notes
- Attachments

`Parent Task` is optional and references another Task, so Tasks can be organized into hierarchies (e.g. a large Task broken into child Tasks) purely through frontmatter — no nested folders required.

This allows compatibility with Obsidian Properties, Dataview and future AI capabilities.

## Task Tag Model

Any Note that is not itself a Task can carry a Task Tag — e.g. a `Task` entry in its `tags` frontmatter, or an inline `#Task` — to mark it as something to be done. Tagging a Note this way never moves it into the Task Folder, never gives it a Parent Task, and never adds Task properties like Status or Priority; it only marks an existing Note as actionable so Task Management can surface it alongside real Tasks.

## Todo Model

Todos live inside a Note's Markdown body, not in frontmatter. This applies equally to a Task, a Task-tagged Note, or any other Note in the Vault:

```markdown
- [ ] Draft the cutover checklist
- [x] Confirm release window with stakeholders
```

A Todo is a plain checklist line — no properties, no filename, no identity beyond its position in the Note's body. Todos let a single Note carry its own granular checklist (e.g. the steps of a release) without spawning a file per step. Toggling a Todo's checkbox is the primary way progress is recorded within a Note; on a Task, it does not, by itself, change the Task's own `Status` property.

## Functional Requirements

### Task Creation

- Create task
- Create from template
- Create in configured folder
- Generate filename automatically
- Support keyboard-only workflow

### Task Editing

- Rename
- Edit metadata
- Edit content
- Move
- Archive
- Delete
- Duplicate

### Task Tags

- Apply a Task Tag to any Note that is not itself a Task
- Remove a Task Tag from a Note
- Surface Task-tagged Notes in the Aggregated Task View alongside Tasks

### Todos

- Add a Todo line to any Note's body (Task, Task-tagged Note, or otherwise)
- Toggle a Todo's checked state
- Remove a Todo line
- Reorder Todos within a Note

### Task Lists

Users can create multiple lists.

Examples:

- Inbox
- Today
- This Week
- Backlog
- Research
- Personal
- Shopping
- Ideas
- Release
- Projects

Custom lists are stored as metadata rather than physical folders.

### Aggregated Task View

Vault Buddy aggregates every configured Vault into a unified task dashboard, including both Tasks and Task-tagged Notes. Todos are surfaced within whichever Note or Task contains them, not as separate rows.

Users can filter by:

- Vault
- Task List
- Status
- Priority
- Due Date
- Project
- Tag
- Creation Date
- Completion Date
- Search Text

### Task Dashboard

The desktop dashboard displays:

- Today's Tasks
- Overdue Tasks
- Inbox
- Upcoming
- Completed Today
- Recently Created
- High Priority
- Recently Modified

### Bulk Operations

- Move multiple tasks
- Complete multiple tasks
- Assign task list
- Change priority
- Archive
- Delete
- Export

### Search

- Search title
- Search description
- Search tags
- Search metadata
- Full-text search

### AI Features (Future)

- Generate child Tasks
- Generate Todos
- Estimate effort
- Suggest priority
- Suggest due date
- Summarize related notes
- Extract tasks from meetings
- Extract tasks from recordings
- Merge duplicates
- Suggest dependencies
- Weekly review

### Workflow Integration

Tasks can be created from:

- Audio recordings
- Meeting notes
- Clipboard
- Screenshots
- Browser captures
- Emails
- Future AI conversations

Every capture provider can create tasks automatically.

### Notifications

- Task created
- Task completed
- Task overdue
- Task archived
- Reminder
- Daily review
- Weekly review

## Non Functional Requirements

### Performance

| Action | Target |
| --- | --- |
| Open modal | < 150 ms |
| Create task | < 500 ms |
| Load dashboard | < 1 second |

### Reliability

- Atomic file creation
- Automatic recovery
- No duplicate filenames
- Offline capable

### Security

- Local-only storage
- No cloud dependency
- Audit log
- Read-only mode

## Technical Architecture

Task Management becomes its own bounded context.

```
Task Management
├── Task Service
├── Task Repository
├── Task Lists
├── Dashboard
├── Search Engine
├── Aggregation Service
├── Template Engine
├── Notification Service
└── AI Extensions
```

The Task Repository writes Markdown documents directly into the configured Vault.

Obsidian is never required for creating or editing tasks.

## Roadmap

### Version 1

- Task creation — ✅ shipped (v0.5.0; due/priority/tags on create by v0.5.3)
- Task editing — ✅ shipped per vault (v0.5.2/v0.5.3: inline rename, due,
  priority, tags, archive; delete/move/duplicate still open)
- Task aggregation — unbuilt
- Task lists — unbuilt
- Dashboard — unbuilt (per-vault date buckets shipped in v0.5.2 are the
  single-vault precursor)

### Version 2

- Templates
- Recurring tasks
- Saved filters
- Quick actions

### Version 3

- AI task generation
- Task extraction
- Project suggestions
- Smart priorities

### Version 4

- Workflow automation
- Dependency graph
- Review assistant
- Knowledge-aware planning

## Success Metrics

- Time to create a task
- Tasks created per day
- Percentage created without opening Obsidian
- Number of aggregated Vaults
- Dashboard usage
- Average task completion time
- User satisfaction

## Long-Term Vision

Task Management becomes the operational layer of the personal knowledge system.

Instead of isolated checklists scattered across sticky notes and chat threads, users manage interconnected Tasks that are directly linked to the knowledge from which they originated.

Every task has context. Every task is searchable. Every task is connected.

Vault Buddy turns personal knowledge into actionable work.
