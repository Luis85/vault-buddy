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

**Staged Capture**:
A screen recording that has been made but not yet published into a Vault: an `.mp4` plus a `.json` sidecar (source, duration, and — for a capture edited before the tutorial editor — the phase-4 editor's saved timeline) living in the app's own staging directory, deliberately **outside every Vault**. An unedited, unapproved capture is not knowledge, so Discarding one must leave no litter in the user's notes. It stays staged until it is Discarded — Publishing a Render of it never removes it — and a crash leaves it recoverable rather than lost. A tutorial project that has opened it *pins* it, and a pinned Staged Capture cannot be Discarded until that project is discarded.
_Avoid_: Draft, temp file — a Staged Capture is the user's footage, not scratch

**Stem**:
One audio input of a Staged Capture kept as its own mono file (`<base>.stem-<n>.m4a`) beside the capture's MIXED track, when the vault keeps them (off by default; new recordings only). A Stem is cut from the very samples the mix is made of, so the two cannot drift; it belongs to its capture only when the capture's sidecar lists it, and it is discarded with the capture. In the editor each Stem is its own audio track and the mix is muted.
_Avoid_: Track (the editor's timeline word), channel (a stem is a whole device's input, downmixed), separate recording — nothing is recorded twice

**Export** *(superseded)*:
The phase-5 act of turning a Staged Capture straight into a `.mp4` plus a companion note inside a Vault, and removing the Staged Capture afterwards. Retired by the tutorial editor (Task 59): the same outcome is now a **Render** followed by a **Publish**, and the Staged Capture is kept. Use the word only for that history — and for the editor's own file exports (a project file, subtitles, diagnostics), which write outside every Vault.
_Avoid_: Save (the phase-5 UI's word for it) — a tutorial project is *saved*, a Product (a Render's output) is *published*

**Render**:
Producing a playable video (a **Product**) from a tutorial project's current edit, inside the project's own folder, never a Vault. An untouched Staged Capture renders as a lossless remux; any edit re-encodes exactly what the timeline shows. Repeatable and cancellable, because the project is kept.
_Avoid_: Export, save — nothing reaches a Vault by rendering

**Publish**:
Copying one Product (a Render's output video), plus a companion note, into a Vault — the tenth sanctioned vault write. Never moves or deletes the Product or the Staged Capture it came from.
_Avoid_: Export, upload — nothing leaves the machine

**Discard**:
Permanently deleting a Staged Capture — its video and its sidecar — from staging. Always confirm-gated, because it destroys the only copy of a recording that has not been Published. The only way a Staged Capture's life ends. Discarding a tutorial *project* is a different act: it deletes the project's edits and Renders and unpins its Staged Capture, and never deletes the recording.
_Avoid_: Cancel (that stops a Render in flight and keeps the footage), Archive (nothing is kept), Delete (reserved for the Task domain's own destructive write)

**Tutorial Project**:
The tutorial editor's editable document: Tracks of Clips, Teaching Cues, captions, chapters and a destination Vault, kept in the app's own project store (`editor-projects\<projectId>\`), **outside every Vault**. It refers to its media (a Staged Capture it pins, imported copies, Takes) and is never itself a vault file; *Save project* commits it there, and a portable or lightweight project file is an export of it. `project` stays the field and module name inside the editor's code, where the scope disambiguates it.
_Avoid_: Project (that is Task metadata), timeline (one view of it), draft

**Track**:
One layer of a Tutorial Project's timeline — video or audio — holding Clips that never overlap except across a transition. Video Tracks composite top over bottom; each Track can be hidden, locked, muted or soloed.
_Avoid_: Layer (the render's word for one drawn Clip), Stem (a recorded audio input, which the editor puts on its own Track)

**Clip**:
One placed use of a media source on a Track: which part of the source (its in and out points) plays when, at what speed, where in the frame and how loud. Cutting, trimming, moving and fading all act on Clips; the source file is never changed.
_Avoid_: Segment (the retired phase-4 editor's word), asset (the source a Clip plays from)

**Teaching Cue**:
An instructional overlay attached to a Clip in the Clip's SOURCE time, so it stays on the moment it explains when the Clip is cut, moved or sped up: a text callout, an arrow, a highlight, a spotlight, a zoom, a numbered step or a privacy cover.
_Avoid_: Annotation, effect (the code's wire word for it), sticker

**Take**:
A webcam recording made inside the editor (camera and microphone in the editor window), streamed into the Tutorial Project's own `takes\` folder and added to the project as a new source. A Take is independent of any Staged Capture; only a webcam recorded **with** a screen capture is synchronized with it.
_Avoid_: Recording (a Capture), webcam track (the synchronized one that belongs to a Staged Capture)

**Rendered Product** (or just **Product**, in the editor):
The immutable video a Render produces, kept in the Tutorial Project's `products\` folder with a record of the exact edit it was made from, which can be watched, restored as the current edit, and Published. Nothing ever writes to a Product after it lands.
_Avoid_: Export, output file, render (the act, not its result)

**Flat layout**:
Vault Buddy's **default** on-disk layout for a capture/import domain: files live directly in `<folder>`, with no year/month subfolders. The timestamped base name encodes the full date, so the folders were never what identified a file.
_Avoid_: Migration — switching layouts never moves or rewrites existing files

**Dated layout**:
The opt-in alternative to the Flat layout: files land under `<folder>/YYYY/MM/`, where the year/month folders are organizational, not identifying. It is a per-domain, per-vault choice — Recording, Document Import and Screen Capture each have their own toggle, all three defaulting to Flat — that changes only where **new** files are written; a domain's existing files stay exactly where they are and are still found regardless of which layout is active.
_Avoid_: Archive structure — the folders exist for browsing, not retention policy

**Knowledge Lifecycle**:
The seven-stage journey every piece of information follows inside Vault Buddy: Capture → Process → Organize → Act → Retrieve → Automate → Learn. Completing an action produces new knowledge, making the journey continuous.
_Avoid_: Workflow — a Workflow is one concrete automation; the Lifecycle is the overarching journey every capability serves

**Task**:
A first-class knowledge object, stored as its own Markdown document inside a Vault's Task Folder, connected via frontmatter to the notes, Projects, or Captures it originated from, and optionally naming a Parent Task — letting Tasks form hierarchies of Subtasks. Progress inside a single Task is tracked with Todos in its body; a Todo is never itself a Task, and a Note carrying only a Task Tag is not a Task either.
_Avoid_: Task Note (redundant — a Task is always a note, "Task" alone is canonical), checklist item — and see Subtask below for the one distinction that still needs active guarding

**Task Tag**:
A tag placed on a Note whose frontmatter type is not Task, marking that Note itself as something to be done. The Note keeps its own type, location and purpose — Task Management surfaces it as actionable without relocating it into the Task Folder or granting it Task properties (Status, Priority, Parent Task, …).
_Avoid_: Task, tagged Task — a Task-tagged Note is not a Task

**Todo**:
An inline checklist line, written `- [ ] description`, inside any Note's body — a Task, a Task-tagged Note, or any other Note — used to track granular progress or present a checklist for that Note. A Todo has no frontmatter, no file of its own, and no identity outside the Note containing it.
_Avoid_: Task, subtask, todo item

**Description**:
A Task's free-text detail — an optional `description` frontmatter property on the Task document, edited from the Task Detail surface. Distinct from the Note **body** (the Markdown under the frontmatter, where a Task's Todos live and which Task Management still never writes) and from a **Todo** (an inline checklist line). The Buddy writes it as a single escaped YAML scalar it round-trips exactly, and reads it leniently — a hand-authored block or flow value degrades rather than corrupting the frontmatter (see docs/Gaps.md).
_Avoid_: body, notes, comment — the Description is a managed frontmatter field, not the free Note body

**Task Detail**:
The in-panel home surface for a single Task, opened by a plain click on the Task's title in a task view (Ctrl/⌘-click still opens the Task in Obsidian instead). It shows and edits the Task's title, Description, Do Date / deadline, Priority, Tags, and List, and offers the per-Task lifecycle verbs — Open in Obsidian, Duplicate, and permanent Delete (behind a hardened confirm).
_Avoid_: Task page, task editor — "Task Detail" is the canonical surface name; the inline row editor is a separate, lighter affordance

**Task List** (or just **List**, in the tasks domain):
A named grouping of Tasks (e.g. Inbox, Next, Someday), reflected as a real folder under the Vault's Task Folder — the filesystem defines which Lists exist (a folder created by hand in Obsidian is a List), and moving a Task between Lists moves its file. The Buddy keeps only preferences ABOUT Lists (the default List for new Tasks, their display order) in its own config, never their existence. Tasks at the Task Folder root belong to no List ("No list"). This supersedes the earlier draft that held Lists as Task metadata.
_Avoid_: Category, board; "folder" alone (a List is a folder, but not every vault folder is a List)

**Order** (manual rank):
An optional `order` number in a Task's frontmatter giving its hand-arranged position (ascending) for the Manual sort. Assigned on first reorder — never written at creation — and read leniently: a Task without one is unranked and follows the ranked ones.
_Avoid_: Index, position (both imply a dense sequence; ranks are sparse and gap-tolerant)

**Task ID**:
A generated, stable identifier for a Task: eight random base36 characters written into its frontmatter under a configurable property (default `task-id`). Opt-in per Vault — once turned on, a new Task gets one at creation and an existing Task is stamped with one the next time it's edited, but an ID already present is never overwritten or regenerated. Distinct from both the file path (which can move or be renamed) and Order (the hand-arranged sort rank) — a Task keeps the same Task ID across either kind of change, which is the point of having one.
_Avoid_: UID, key, index — the ID is random, not sequential, and it never doubles as the sort rank

**Parent Task**:
The Task a Subtask names as its ancestor, addressed by the parent's own Task ID (the authoritative reference — resolution never depends on the parent's title or file path) plus an Obsidian link carried alongside for click-through and Dataview. Shown as the Parent row on the child's Task Detail surface, with Change / Clear and a picker that will not let you create a cycle. Setting a Vault's first Parent Task turns on Task IDs for it automatically, since the reference depends on one existing.
_Avoid_: parent note, parent item — a Parent Task is always a Task, addressed the same way any other Task is; category or folder (that is a Task List, a different relationship)

**Subtask**:
A Task that names another Task as its Parent Task — the Task-level hierarchy relationship, shown as the Subtasks section (with Add Subtask) on the parent's Task Detail surface, and as a light-touch subtask-count badge / parent chip in the main Task list. A Subtask is still an ordinary, independent Task — its own Status, Priority, Tags, List — completing it does not complete its Parent Task, and completing every Subtask does not complete the parent either; the parent only ever shows *progress*, never enforcement.
_Avoid_: "subtask" for a Todo, or "checklist item" for a Subtask — the confusion this term used to be avoided over entirely, now drawn precisely instead: a Subtask is always its own Task document (its own file, its own frontmatter, a Parent Task reference); a Todo is an inline checklist line with no file and no identity of its own. Where the two could still be confused, say "Subtask" for the former and "Todo" (or "checklist line") for the latter.

**Do Date** (the `scheduled` frontmatter field):
The day you plan to WORK a Task — the "when will I do this" — kept distinct from the Task's **deadline** (the `due` date, "when is it due"). Stored as an optional `scheduled: YYYY-MM-DD` in the Task's frontmatter, read leniently (a non-date value is treated as unscheduled). It is the field the Plan grouping buckets by: a Task's effective plan date is its Do Date if set, else its deadline, so setting a Do Date moves a Task's plan even when its deadline is already past; a Task with neither sits under **Anytime**.
_Avoid_: due date / deadline (that is `due`, a separate field); "scheduled" spoken as a synonym for due; start date

**Plan** (grouping):
The task-view grouping — the middle tab of the `Lists | Plan | Tags` toggle — that buckets Tasks by their effective plan date (Do Date, else deadline) into Overdue / Today / Upcoming / Anytime / Done. Supersedes the earlier "Dates" grouping label; the internal grouping key stays `dates`. It is the default grouping the aggregate ("All tasks") view opens on, while a per-vault view still opens on Lists.
_Avoid_: Dates (the old label), Calendar, Schedule view

**Focus**:
The set of Tasks scheduled for today (Do Date = today) — a forward-looking term for what a future pinned Focus widget on the desktop will surface. Narrower than the Plan grouping's **Today** bucket, which also catches a due-only Task whose deadline is today (Do Date unset) — a Task with an already-past deadline falls into Overdue, not Today. Focus is scoped to the Do Date specifically. Not yet its own surface — the term exists now so the eventual widget has a name to build toward.
_Avoid_: Today (the broader Plan bucket), Dashboard, Agenda

**Project**:
Task metadata linking a Task to the larger body of notes or work it belongs to.
_Avoid_: Epic, initiative — and the tutorial editor's document, which is a **Tutorial Project**

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
