# Task Management — Vertical Slice 1 Design

- **Date:** 2026-07-08
- **Status:** Approved
- **Source:** First vertical slice of the [Task Management capability
  PRD](../../prds/task-management.md). That PRD is large (aggregated
  dashboards, lists, templates, AI, hierarchy); this slice ships the smallest
  end-to-end path: configure a per-vault tasks folder, list a vault's tasks as
  a simple todo list, add a task from the vault list view, and check tasks off.

## Goal

Give every registered vault a **per-vault Tasks view**: a simple todo list
derived from a configured tasks folder, with an inline "add task" box and
checkboxes to mark tasks done. Reached by a new button on each vault row. Two
sanctioned vault writes — creating a task file and flipping a task's `status`
— both under the codebase's never-clobber discipline. Everything else in the
PRD is out of scope (see the end of this document).

## Domain model (this slice)

Per the PRD, a **Task is its own Markdown document** living in the vault's
tasks folder. This slice implements exactly that — not inline Todos, not Task
Tags. A task file:

```markdown
---
type: Task
status: new
title: "Buy milk"
created: 2026-07-08
---
```

- **`type: Task`** is the identity. Any `.md` in the tasks folder whose
  frontmatter `type` is `Task` is a task — including files the user authored
  themselves in Obsidian. This is the Obsidian-Properties / Dataview-compatible
  model the PRD calls for; there is no private Vault Buddy marker.
- **`status`** is the completion state. This slice treats it as binary for the
  checkbox: a task renders **checked iff `status: done`**; any other value
  (`new`, or a user's own e.g. `in-progress`) renders **unchecked**. Newly
  created tasks are `status: new`.
- **`title`** is the display text (falls back to the filename stem when
  absent). **`created`** is an audit date (`YYYY-MM-DD`).
- The **body is empty** on creation — the user adds Todos/notes/context in
  Obsidian later. Nothing in this slice reads or writes the body.

Filename: `YYYY-MM-DD-<slug>.md`, flat inside the tasks folder (the PRD's
example layout). `<slug>` is derived from the title; collisions take the
existing capture suffix scheme (`… (2).md`).

## Safety model — the two vault writes

The vault domain's hard rule is "never clobber a vault." Task files may be
hand-authored, so identity (`type: Task`) is **not** ownership. Safety comes
from *surgical, validated* writes, mirroring the transcript sidecar discipline:

1. **Add (create)** reuses `capture_note::write_note_collision_safe` — atomic
   hidden temp opened `create_new` (never truncates/follows a planted symlink)
   + fsync + `rename_noreplace` (non-replacing; suffixes on collision). A
   create can never overwrite an existing file.
2. **Toggle (`status` flip)** is a read-modify-write that is *surgical*:
   - Read the file. Refuse unless its frontmatter is `type: Task` **and** the
     target path resolves inside the vault's tasks root (else skip with a
     `log::warn!` + a user-facing notification — never a silent no-op that
     looks like success).
   - Change **only** the single `status:` line; preserve every other
     frontmatter field and the entire body byte-for-byte (including the file's
     existing line endings).
   - Write to a hidden `*.vault-buddy.tmp` temp (`create_new` + fsync), then
     **replacing** rename into place. This is the one *replacing* write into a
     vault — justified exactly like `transcript::replace_if_ours`: the
     destination is the `type: Task` file we just read and are editing in
     place, and we touch only its status.
   - **Known trade-off (documented, accepted for the slice):** like any
     read-modify-write there is a TOCTOU window — if the user edits the same
     file in Obsidian between our read and our replace, their concurrent edit
     is lost. Acceptable for a single-user desktop app with small task files;
     revisited only if it bites.

Path validation reuses `capture_paths::safe_recording_root` (lexically rejects
`..` / absolute / drive-letter escapes) + `assert_root_inside_vault`
(canonicalizes both, catches pre-existing symlink/junction escapes) before any
read or write.

## Rust — new `core/src/tasks.rs` (pure, unit-tested on Linux)

Keep all logic in the core crate; the shell only wires IPC. Public surface:

```rust
pub struct TaskItem {
    pub path: PathBuf,     // absolute path to the task file
    pub title: String,     // frontmatter `title`, else filename stem
    pub status: String,    // frontmatter `status`, else "new"
    pub created: String,   // frontmatter `created`, else "" (best-effort)
    pub done: bool,        // status == "done"
}

/// `YYYY-MM-DD-<slug>` (no extension); slug lower-cased, non-alphanumerics
/// collapsed to single hyphens, trimmed, length-capped; empty slug → "task".
pub fn task_basename(title: &str, today: &str) -> String;

/// Frontmatter-only task document (empty body).
pub fn render_task(title: &str, created: &str) -> String;

/// Every `type: Task` file directly under `root`, best-effort. Missing /
/// unreadable root or files degrade to an empty list. Sort: open first
/// (newest `created`, then title), completed after.
pub fn list_tasks(root: &Path) -> Vec<TaskItem>;

/// True iff the file's frontmatter `type` is `Task`.
pub fn is_task(content: &str) -> bool;

/// Return `content` with only the `status:` frontmatter line set to
/// `new_status`, everything else preserved (line endings included). `None`
/// if the file is not `type: Task` or has no frontmatter `status` line to
/// replace — the caller then skips + warns.
pub fn set_status(content: &str, new_status: &str) -> Option<String>;
```

Reading frontmatter reuses `capture_note::note_field` (single top-level scalar
from the leading `---` block; never scans the body). `list_tasks` mirrors
`recordings::list_recordings`: enumerate the folder, keep only `type: Task`
`.md` files, read `title`/`status`/`created` best-effort, degrade silently.
`task_basename`, `render_task`, `set_status`, `is_task`, and the sort order all
get direct unit tests; `set_status` gets tests proving it preserves the body,
other frontmatter fields, and CRLF endings, and refuses a non-`type: Task`
file.

The `today` / `created` date is passed in from the shell (the core crate stays
clock-free and testable), formatted `YYYY-MM-DD`.

## Config — extend `core/src/capture_config.rs`

Per-vault config already lives here (app-side `%APPDATA%\vault-buddy\config.json`,
keyed by vault id, never inside a vault). Add one field to
`VaultCaptureConfig`:

```rust
/// Vault-relative folder holding this vault's task documents.
/// None → the default "Tasks".
pub tasks_folder: Option<String>,
```

- JSON key `tasksFolder` (camelCase), parsed per-field-defensively in
  `vault_entry` (a malformed value defaults only itself), serialized only when
  `Some`, with a round-trip test. `Default` → `None`.
- Helper `pub fn tasks_root(&self) -> &str { self.tasks_folder.as_deref().unwrap_or("Tasks") }`.
- `CaptureConfigDto` (in `capture_commands.rs`) is **not** extended; the Tasks
  view uses its own dedicated config commands (below) so the two domains stay
  decoupled at the IPC layer while sharing one storage struct.

## IPC commands (in `src-tauri/src/`, registered in `lib.rs`)

All synchronous (main-thread), following the existing command idiom
(`discover_vaults().find(id)` → `vault_config` → `safe_recording_root`). A new
`task_commands.rs` module, or an extension of `capture_commands.rs` — a new
module, to keep the file focused.

- `get_tasks_config(id) -> TasksConfigDto { tasks_folder }` — returns the
  configured folder (or default `"Tasks"`) for the vault; defaults for unknown
  vaults, never errors.
- `set_tasks_config(lock, id, tasks_folder) -> Result<(), String>` — trims /
  empties→None, validates the folder against the real vault path with
  `safe_recording_root` **before** writing, then read-modify-writes the shared
  config behind `ConfigWriteLock` via `update_vault_config`.
- `list_tasks(id) -> Vec<TaskDto>` — resolve vault → tasks root via
  `safe_recording_root` → `tasks::list_tasks`. Read-only; degrades to empty.
- `add_task(id, title) -> Result<TaskDto, String>` — reject empty/whitespace
  title; resolve+validate root (creating the folder if missing, like capture
  does for its root); render with today's date; `write_note_collision_safe`;
  return the created `TaskDto`.
- `set_task_status(id, path, done) -> Result<(), String>` — resolve+validate
  root; assert `path` is inside it; read → `is_task` + `set_status(content,
  if done { "done" } else { "new" })` → atomic replacing write; on refusal
  (not a task / no status line) return a descriptive `Err`.

`TaskDto` is `#[serde(rename_all = "camelCase")]` with `path`, `title`,
`status`, `created`, `done`.

## Frontend

- **`stores/vaults.ts`**: add `"tasks"` to the `view` union; add
  `tasksVaultId: string | null`; `openTasks(vaultId)` setter (sets view +
  id); `showList()` clears it; `back()` maps `tasks` → `showList()`. Header
  title "Tasks".
- **`components/VaultList.vue`**: a new **Tasks** button per row (checklist
  icon), disabled while `busyVaultId !== null`, emitting `open-tasks` with the
  vault id. Placed alongside the existing open/daily/mic/cog buttons.
- **`components/ActionPanel.vue`**: wire `@open-tasks="store.openTasks($event)"`
  on `<VaultList>`; add a `v-else-if="view === 'tasks'"` branch rendering
  `<Tasks :key="tasksVaultId" :vault-id="tasksVaultId" />` inside a
  `panel-scroll`; add the header title case.
- **New `components/Tasks.vue`** — self-contained local state (refs), mirroring
  `Recordings.vue`; **no new Pinia store**. On mount (and on `shownNonce`
  change) it `get_tasks_config` + `list_tasks`. Renders:
  - a compact **tasks-folder setting** in/under the header (text input,
    placeholder/default `Tasks`, saved via `set_tasks_config`; on save it
    re-lists);
  - an **add-task input** at the top (Enter or a button → `add_task` → prepend
    to the list; empty title is a no-op);
  - the **todo list**: each row a checkbox + title, open tasks first,
    completed rows greyed. Toggling calls `set_task_status` and updates the row
    optimistically, reverting + notifying on failure.
  - Errors surface through the existing notifications store; an empty folder
    shows a friendly empty state.
- **`types.ts`**: `TaskItem { path; title; status; created; done }` and
  `TasksConfig { tasksFolder }`.

## Testing

- **Rust (`core/src/tasks.rs`)**: `task_basename` (slug, collisions via
  `candidate`, empty-slug fallback), `render_task`, `is_task`, `set_status`
  (flips only the status line; preserves body / other fields / CRLF; refuses
  non-`type: Task`; refuses when no status line), `list_tasks` (only
  `type: Task` files; title/status/created read-back; sort order; silent
  degrade on missing root). Config round-trip test for `tasksFolder`.
- **Vitest**: `Tasks.vue` — initial load, add flow, toggle flow, folder-config
  save (all via `mockIPC`); empty state. `VaultList.vue` — the new Tasks button
  emits `open-tasks`. `vaults` store — `openTasks` sets view+id, `back()` from
  `tasks` returns to the list. `ActionPanel.vue` — `tasks` view renders
  `<Tasks>` and the header shows the back button + "Tasks" title.
- TDD per the repo convention: failing test first, then implementation.
  Regression tests name their failure mode in a comment.

## Out of scope for this slice (deferred to later increments)

Due dates, priority, task lists/inbox/next, tags, project, estimated effort,
parent-task hierarchy; the cross-vault aggregated dashboard; Task Tags on other
notes and inline-Todo scanning; templates and naming conventions; the
standalone Quick Task modal (this slice adds tasks inside the Tasks view);
archive/delete/rename/move/duplicate; search and filtering; opening a task in
Obsidian; recurring tasks and notifications. The frontmatter and folder
conventions here are chosen to grow into all of it without a migration.
