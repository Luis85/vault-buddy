# Task Management — Capability PRD

- **Status:** Draft
- **Version:** 1.0
- **Parent Product:** Vault Buddy

---

## Vision

Every task deserves context.

Vault Buddy transforms task management from maintaining isolated checklists into managing actionable knowledge.

Tasks become first-class knowledge objects that are directly connected to projects, notes, meetings, recordings and documents inside an Obsidian Vault.

Users should be able to create, organize and complete tasks without ever opening Obsidian.

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
- Related Notes
- Attachments

This allows compatibility with Obsidian Properties, Dataview and future AI capabilities.

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

Vault Buddy aggregates every configured Vault into a unified task dashboard.

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

- Generate subtasks
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

- Task creation
- Task editing
- Task aggregation
- Task lists
- Dashboard

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

Instead of isolated todos, users manage interconnected work items that are directly linked to the knowledge from which they originated.

Every task has context. Every task is searchable. Every task is connected.

Vault Buddy turns personal knowledge into actionable work.
