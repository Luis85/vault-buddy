# Tasks Todo-List Design — open in Obsidian, due dates, priority, buckets, inline edit

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** Follow-up on the task-management feature (v0.5.0 + the v0.5.1
  polish). Drives the per-vault task list from a flat check-off list to a
  proper todo list: click-to-open in Obsidian, due dates, priority,
  date-bucket grouping, a title filter, and an inline row editor.

## Goals

1. **Open a task in Obsidian by clicking its title** (read-only handoff, like
   the recordings list).
2. **Due dates** — a `due:` frontmatter field, set on add or edit, driving
   sort and bucketing, with overdue highlighting.
3. **Priority** — a `priority:` field (high/normal/low), set on add or edit,
   shown on the row and factored into sort.
4. **Date buckets** — Overdue / Today / Upcoming / No date / Done section
   headers in the Tasks view.
5. **Filter** — a search box narrowing the list by title.
6. **Inline row editor** — rename, due date, and priority edited in place via
   a pencil button; one surgical frontmatter write per Save.

## Frontmatter schema (the contract)

Two optional keys join `type` / `status` / `title` / `created`:

```
---
type: Task
status: new
title: "Ship the release"
created: 2026-07-09
due: 2026-07-15
priority: high
---
```

- **`due`** — plain `YYYY-MM-DD` (Obsidian Properties date type,
  Dataview-readable). Absent = no due date. Clearing the date **removes the
  line** (no `due:` with an empty value is ever written).
- **`priority`** — `high` | `normal` | `low`. Absent = normal; `normal` is
  **not written** unless the user explicitly sets it (keeps hand-authored
  files minimal and round-trip stable).
- **Graceful degradation on read:** a hand-authored unparseable `due`
  (anything not `YYYY-MM-DD`) sorts/buckets as "no date"; an unknown
  `priority` value renders and sorts as normal. Never an error — same
  defensive-read posture as the rest of the vault domain.

## Core (`core/src/tasks.rs`, clock-free)

- **`TaskItem` gains `due: Option<String>` and `priority: Option<String>`**
  (raw strings via `note_field`; interpretation stays at the edges).
- **`render_task(title, created, due: Option<&str>, priority: Option<&str>)`**
  writes the optional lines (after `created`) only when present.
  `create_task` threads them through.
- **`set_fields(content, updates: &[(&str, Option<&str>)]) -> Option<String>`**
  — the multi-key generalization of `set_status`'s line rewriter. For each
  `(key, value)`: `Some(value)` rewrites the existing `key:` line in place or
  inserts one at the closing fence; `None` removes the line. Everything else
  is preserved byte-for-byte (CRLF, unknown keys, key order, body). Same
  guards as `set_status`: refuses a non-`type: Task` document or an unclosed
  frontmatter fence (`None`). The `title` value is written through
  `yaml_quote` by the caller; `set_fields` writes values verbatim.
  `set_status` becomes a thin wrapper (`set_fields(content,
  &[("status", Some(new_status))])`) so the list/toggle agreement invariants
  keep one implementation.
- **`update_task_fields(root, path, updates) -> Result<(), String>`** — the
  disk write: same canonicalize root+path + containment + read + atomic
  REPLACING write as `set_task_status` (which now delegates to it with a
  one-entry update).
- **Sort stays clock-free.** `list_tasks` orders: open before done, then due
  ascending with no-due last, then priority (high < normal < low), then
  newest `created`, then title. Done tasks: newest `created`, then title.
  ("Today"/"Overdue" need a clock, so bucketing is NOT core's job.)

## Shell (`task_commands.rs`, registered in `lib.rs`)

- **`open_task(id, path) -> Result<(), String>`** — mirrors
  `open_recording_note`: resolve the vault + tasks root, canonicalize and
  require the path inside the root (read-safety, same as `set_task_status`),
  then `uri::launch(uri::open_file_uri(&vault.id,
  &uri::vault_relative_no_ext(...)))`. Read-only; the launch is
  `uri::launch`-logged like every other vault open.
- **`update_task(id, path, patch) -> Result<(), String>`** — patch is
  `{ title?: String, due?: String, clearDue?: bool, priority?: String }`
  (camelCase over IPC). Validation inline before any write: `due` must match
  `YYYY-MM-DD` (digits + hyphens, a real calendar check is NOT required —
  Obsidian tolerates e.g. `2026-02-31`, and the picker is a native date
  input); `priority` must be `high|normal|low` — with `normal` translating to
  **removing** the line; empty/whitespace `title` rejected. Builds the
  `set_fields` update list (`title` → `yaml_quote`d) and delegates to
  `tasks::update_task_fields`. One read-modify-write per call.
- **`add_task(id, title, due?: Option<String>, priority?: Option<String>)`**
  — extended with the same validation; passes through to `create_task`.
  Returns the created `TaskDto` including the new fields.
- **`TaskDto` gains `due: Option<String>`, `priority: Option<String>`**
  (serde camelCase keeps `due`/`priority` as-is).
- `set_task_status` / `count_open_tasks` / `list_tasks` are unchanged apart
  from the widened DTO.

## Frontend

### `types.ts`
`TaskItem` gains `due: string | null` and `priority: string | null`. A
`TaskPatch` type for `update_task`'s patch argument.

### Tasks view (`Tasks.vue`)

- **Buckets (presentation-only).** The sorted list from `list_tasks` is
  partitioned client-side using the local date (`new Date()`, formatted
  `YYYY-MM-DD` locally — never UTC, matching `add_task`'s local-date rule):
  **Overdue** (`due < today`, open), **Today** (`due == today`, open),
  **Upcoming** (`due > today`, open), **No date** (open, no parseable due),
  **Done** (`status == done`, any due). Section headers render only for
  non-empty buckets, and only once at least one open task has a parseable
  due date — a vault with no dated open tasks (however many done/undated
  tasks it has) keeps the flat, header-less list it had before this feature.
  Order within buckets comes from the
  backend sort (kept mirrored in `sortInPlace`).
- **Row.** The title becomes a click target that invokes
  `open_task {id, path}`. On success the panel closes (best-effort
  `close_panel` — Obsidian takes over, mirroring the vault-open and
  recording-open flows); on failure it stays open with an error toast. A due chip
  (`Jul 15` short format; red text when overdue) and a priority dot (red =
  high, muted = low, nothing for normal) render beside the title. Checkbox /
  pencil / archive are separate buttons (no propagation into the title
  click).
- **Inline editor.** The pencil toggles the row into edit mode: title text
  input, native `<input type="date">`, priority segmented control
  (High/Normal/Low), Save + Cancel. Save sends one
  `update_task {id, path, patch}` (only changed fields; clearing the date
  sends `clearDue: true`), optimistic (row updates + re-sort/re-bucket
  immediately, revert + toast on failure), sharing the per-row in-flight
  `busy` guard with toggle/archive so no two writes for one task race. Only
  one row can be in edit mode at a time; opening another closes the first
  (unsaved edits discarded). Edit mode also resets on panel reopen
  (`shownNonce` behavior unchanged at the view level — the component
  remounts per vault visit, which already clears transient state).
- **Add row.** Title input keeps Enter-to-add. A small toggle button beside
  Add reveals a second line with the optional due date input + priority
  control; `add_task` is sent with `due`/`priority` only when set, and the
  controls reset after a successful add.
- **Filter.** A search input above the list, shown only when the vault has
  more than 5 tasks (mirrors the vault-list filter threshold), narrowing all
  buckets by case-insensitive title substring. The query applies only while
  the input is shown: archiving/editing the vault down to 5 or fewer tasks
  hides the input AND deactivates any stale query (the text is kept and
  reactivates if the count climbs back over the threshold), so the user can
  never be stranded on a narrowed list with no visible control. Empty
  buckets under the filter hide with their headers; progress bar keeps
  counting the unfiltered list.

### Unchanged
Progress bar (done/total of the visible list), archive action, counter
badge, Vault-settings tasks folder — all as shipped in v0.5.1.

## Sanctioned writes (unchanged discipline)

Still exactly two task write paths: collision-safe **create** (now with two
more optional lines) and the surgical **frontmatter field write** (now
multi-key via `set_fields`, still touching only the named lines, still
atomic temp + fsync + REPLACING rename, still refusing non-Task/malformed
docs). `open_task` is read-only. No new write path.

## Testing

- **Core:** `set_fields` rewrites/inserts/removes only named keys preserving
  body + CRLF + unknown keys byte-for-byte; refuses non-task/unclosed
  fence; `set_status` parity via the wrapper; `render_task`/`create_task`
  with due+priority (and without — byte-identical to today's output); new
  sort order (due asc, no-due last, priority tiers, done last);
  `update_task_fields` escape rejection (reuses the `set_task_status`
  containment tests' pattern).
- **Shell contract (via Vitest `mockIPC`):** `update_task` patch shape;
  `open_task` args; `add_task` carries due/priority.
- **Vitest:** bucketing incl. the overdue/today boundary (mock the clock);
  bad hand-authored `due` lands in No date; title click invokes `open_task`;
  editor Save sends only changed fields and `clearDue`; editor failure
  reverts; add-with-due/priority; filter narrows and hides empty buckets;
  priority dot/due chip render rules; existing toggle/archive/progress tests
  keep passing.

## Out of scope (YAGNI)

Recurring tasks, subtasks, tags/projects, reminders/notifications,
drag-reorder, a "show archived" view, calendar-validity checking of dates,
natural-language date parsing.
