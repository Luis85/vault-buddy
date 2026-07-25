# Task-management UX program — Task Detail surface: description + delete + duplicate — design

Date: 2026-07-24
Status: accepted (user request: bring task management "up to a ux/ui powerhouse
to make daily work enjoyable … built upon what we have." Direction chosen:
**task depth** — make each Task a richer knowledge object. First increment of
that sub-program chosen: **the Task Detail surface** (the container), with a
structured **description** field, and the two missing lifecycle verbs **delete**
and **duplicate**. Sub-decisions the user made during brainstorming: description
= a structured frontmatter field, *not* body editing (lower risk, rides the
existing surgical writer); delete = **permanent + confirm**, not a recoverable
trash.)

## Context

Task management is a capable per-vault + cross-vault feature: `type: Task`
markdown notes with `due`/`scheduled`/`priority`/`tags`/`order`/opt-in id, Lists
(as folders) with a full lifecycle, Lists/Plan/Tags grouping, a sort selector,
manual drag-to-reorder, an inline editor, and the aggregated "All tasks" view.
The most recent increment (PR #75,
`2026-07-24-task-management-do-date-planner-foundation-design.md`) shipped the
do-date foundation + the cross-vault Plan-my-day grouping.

What each Task is **not yet** is a *knowledge object with depth*: it has no home
where you see and manage everything about it, no description/context beyond its
title, and no way to remove or copy it — the only lifecycle verbs are
create / edit-fields / status-toggle / archive / move. Concretely, against the
real code:

- The panel edits **frontmatter only**. Every reader stops at the closing `---`
  fence (`capture_note::note_field`, `tasks/parse.rs`); the **note body is never
  read, never edited** — written once at create from an optional
  `task_body_template` and otherwise empty (`tasks/disk.rs:129-144`). The one
  write path is `update_task_fields → set_fields`, which is frontmatter-scoped
  by construction (`tasks/writer.rs:16-133`).
- There is **no delete or duplicate** of a Task document anywhere — the only
  "delete" is `delete_task_list` for list *folders*.
- A task's title click opens it **in Obsidian** (`open_task`,
  `task_commands.rs:600-617`); there is no in-app detail view.

This makes "task depth" the natural next direction. Depth is really **four
independent capabilities**, too much for one spec:

| Unit | What it is | This spec |
| --- | --- | --- |
| **A. Subtask hierarchy** | a `parent` frontmatter field → a Task tree (the PRD's `Parent Task`), nested display, reparenting, "add subtask" | later increment |
| **B. Description / detail** | a home surface for each Task + an editable description | **yes** |
| **C. Delete + duplicate** | the two missing lifecycle verbs | **yes** |
| **D. Multi-select + bulk ops** | select N rows → complete / schedule / move / prioritize / archive / delete together | later increment |

B, C, and eventually A all want a **detail "home"** — a compact row can't hold a
description, a subtask list, or a full action set. So this increment builds that
container first (B + C); A and D land inside it next. This is the sequencing the
user chose ("Task Detail surface first").

## Goals & scope (this increment)

- **A Task Detail panel surface** (`taskDetail` view) — the calm, full-height
  home for one Task: all its metadata editable in one place, its description,
  the lifecycle verbs, and a one-click jump to Obsidian. Works identically in
  per-vault and aggregate ("All tasks") mode, since each row already carries its
  own `vaultId` (`AggTask`, `types.ts:194`).
- **A structured `description` field** — a multi-line free-text description
  stored in frontmatter, edited in the detail view, riding the **existing**
  surgical field writer (no new body-write path). Read defensively, written
  never-clobber, absent by default — a task with no description behaves exactly
  as today.
- **Delete** — permanently remove a Task file, behind an unmissable inline
  confirm. The app's first destructive vault write; documented as a deliberate,
  bounded departure from "the vault is sacred."
- **Duplicate** — a faithful, collision-safe copy of a Task (body and all
  frontmatter preserved), with a fresh id and reset status, landing in the same
  List.

### Non-goals (each is a later increment or a deliberate omission)

- **Subtask hierarchy (unit A)** and **multi-select / bulk ops (unit D)** — the
  detail surface is designed to host both, but neither is built here.
- **Note-body editing.** The description lives in frontmatter; the note body
  stays untouched (no read, no write). Rich prose remains Obsidian's job, one
  click away via the detail view's "Open in Obsidian."
- **Description on create.** Description is edited in the detail view only; the
  composer and the compact inline editor are unchanged (no new create field).
- **Recoverable delete** (system trash / vault `.trash` + Undo). The user chose
  permanent+confirm; a recoverable model is noted as a future refinement in Gaps,
  not built here.
- **Any change to status-toggle / list / tag / order / do-date behavior**, or to
  the task file format beyond adding `description`.

## Design

### 1. The `taskDetail` view & navigation

A new `"taskDetail"` entry in the panel view union (`stores/vaults.ts:26-37`),
whose parent in the fixed one-parent-per-view tree is the **tasks list** — the
same shape as `recordMode`/`recordings`.

- **State.** The store gains `taskDetailTask: AggTask | null`. `openTaskDetail(task)`
  stashes the row's own `AggTask` (which carries `vaultId`/`vaultName`, so
  per-vault and aggregate opens are identical) and sets `view = "taskDetail"`.
  The detail view renders instantly from that object — no fetch on open.
- **The `tasksVaultId` preservation gotcha (from the code map).** `showList()`
  and the `tasks`→list `back()` path both **clear** `tasksVaultId`
  (`vaults.ts:222`, `:307-323`). So `openTaskDetail` must **not** clear it, and
  the `back()` case for `taskDetail` returns to the tasks list *in the mode it
  came from*: `view = "tasks"` with the still-intact `tasksVaultId` (null →
  aggregate, an id → that vault). This is the one navigation subtlety; without
  it a detail opened from the aggregate would return to a per-vault (or empty)
  list.
- **`ActionPanel.vue`.** Add a `VIEW_TITLES["taskDetail"]` entry — the header
  shows the **task's title** (truncated), a nicer touch than a static "Task" —
  computed from `store.taskDetailTask?.title` alongside the existing "All tasks"
  special-case (`:74-78`). Add a `<Transition>` branch (`:282-407`) rendering
  `<TaskDetail>`. The existing `v-else` back button (`:218-228`) covers the new
  view automatically (it calls `store.back()`).
- **No cross-view state sync.** A `taskDetail` view is a **sibling** of
  `Tasks.vue`, so it does not share `Tasks.vue`'s in-memory `tasks` array or its
  optimistic mutators. It doesn't need to: `Tasks.vue` is keyed by `tasksKey`
  and **remounts on return**, re-running its `onMounted` fetch — so any
  edit / delete / duplicate performed in the detail view is reflected in the
  list the moment you go back. The detail view operates on the single Task file
  via IPC and keeps its own local copy consistent for its own display.

### 2. Opening the detail view

- **Title click → detail view.** `TaskRow` currently emits `open` on a title
  click, wired to `useTaskActions.openInObsidian` (`useTaskActions.ts:86-98`).
  Repoint that: a title click now calls `store.openTaskDetail(task)`. "Manage
  without opening Obsidian" is the product principle, and the Obsidian jump
  stays one click away inside the detail view.
- **Ctrl/⌘-click title → straight to Obsidian.** An additive power-user
  shortcut that preserves today's direct-jump muscle memory:
  `TaskRow` passes the modifier state on its `open` emit; the container routes a
  modified click to `openInObsidian` and a plain click to `openTaskDetail`.
  Cheap, non-regressing, and the only behavior a heavy title-click user could
  miss otherwise.

### 3. The `description` field (data model)

- **Frontmatter key `description`**, a free-text, possibly multi-line string,
  edited as a textarea in the detail view and sent through the **existing**
  `update_task` patch — **no new write path**. It is added to `TaskPatchDto`
  (`task_commands.rs:476-497`) as `description: Option<String>` +
  `clear_description: bool`, mirroring `due`/`clear_due`, and threaded into the
  `updates` list for `set_fields` exactly like every other field
  (`Some(v)` rewrites/inserts the `description:` line, clear removes it).
- **Multi-line storage = a single-line double-quoted YAML scalar with escaped
  newlines** (`description: "line one\nline two"`). This is the crux
  implementation decision, and it is what lets a multi-line value ride the
  line-oriented `set_fields` untouched (it is one physical line the surgical
  writer already handles). It requires a small, well-contained **escape/unescape
  pair** — a NEW `yaml_quote_multiline` / `yaml_unquote_multiline`, NOT a change
  to the shared `yaml_quote` (which deliberately flattens newlines to spaces for
  the single-line managed fields and is used by every renderer):
  - **Write:** `yaml_quote_multiline` escapes `\` → `\\`, `"` → `\"`, newline →
    `\n`, tab → `\t` (CR dropped) — producing a valid double-quoted single-line
    scalar for *any* string, including multi-line.
  - **Read:** `yaml_unquote_multiline` — strip matching surrounding double
    quotes and unescape in a **single left-to-right pass** (so `\\` consumes
    both chars before an `n` could be misread as a newline). An **unquoted**
    plain value passes through as-is; anything malformed degrades to empty
    (defensive read, the vault-domain posture). The description read does NOT
    reuse `scalar_field` (which strips inline `#` comments — a description may
    contain `#`) nor `note_field` (which unescapes only `\"`/`\\`, not `\n`); it
    is its own top-level-key reader (`parse::description_field`).
  - *Alternative considered and rejected for this increment:* a true YAML
    **block scalar** (`description: |` + indented lines) — cleaner in raw
    markdown, but it needs real block-scalar **read and write** machinery in
    `set_fields`/`parse.rs`, a materially bigger change than a short escape
    helper. The escaped-single-line form is the lower-risk fit for the "rides
    the existing writer" intent.
- **Two documented caveats (→ Gaps):** (a) Obsidian's Properties UI has no
  native multi-line "long text" type, so it renders `description` as a
  single-line text property (the value round-trips correctly; only its
  in-Obsidian *presentation* is single-line). (b) A **hand-authored** block
  scalar (`description: |`) in Obsidian will not round-trip through our
  single-line reader — it reads as its raw marker and editing it via the detail
  view may leave orphaned continuation lines (the app only ever writes escaped
  single-line scalars). Both are acceptable because the app owns the write
  format and reads defensively; both are recorded in `docs/Gaps.md`, with
  extending `set_fields` to consume block scalars named as the future hardening.
- **Reserved in BOTH reserved-key sets** (`tasks/disk.rs::RESERVED_TASK_KEYS`
  the template filter, `tasks/id.rs::RESERVED_TASK_KEYS` via `is_valid_id_property`).
  `description` is a **managed detail-view field**: the detail surface owns it
  via `set_fields` (single-line escaped scalar), exactly as `due`/`status`/
  `priority` are managed and set via the composer/toggle, not templates. So a
  template must not seed it (id-set: a `description` id property would let an id
  write clobber it; template-set: `render_extra_frontmatter` would emit whatever
  YAML shape the template used — e.g. a **block scalar** `description: |` — which
  `description_field` reads back as its bare marker and a later `set_fields` save
  orphans the indented content; Codex P2, PR #76). Reserving it in both, like
  every other managed field, is the stable, consistent choice — an earlier draft
  un-reserved it from the template set to let templates seed it, which reopened
  exactly that block-scalar corruption. The two constants stay identical. (The
  pre-existing edge — a vault that had already set its id property to
  `description`, which older releases accepted — is the same shape as GAP-68's
  `scheduled`-as-id case and is documented at GAP-77, not migrated.)
- **DTO:** `TaskItem` (`tasks/list.rs`) and `TaskDto` (`services/tasks/mod.rs`)
  gain `description: Option<String>`, filtered through `yaml_unquote_multiline`
  at the read boundary; it rides `list_tasks` like every other field, and
  `AggTask`/`TaskItem` in `types.ts` gains `description: string | null`
  (camelCase, the existing precedent). **Trade-off:** every `list_tasks` row now
  carries its description, so the detail view opens instantly with no
  read-one-task command. Descriptions are short in practice, so the payload cost
  is negligible; if it ever bites, a `get_task(id, path)` command is the clean
  fallback (noted, not built).
- **`render_task` is unchanged** — description is not a create field this
  increment, so create output stays byte-identical (regression-tested).

### 4. The detail view contents

`TaskDetail.vue` is a self-contained, full-height surface fed the target
`AggTask`. It shows and edits, in one calm place:

- **Title** (editable), **Description** (the signature multi-line textarea),
  and the full metadata set — **Due**, **Do date** (`scheduled`), **Priority**,
  **Tags**, **List** — the same fields the inline editor edits, given room to
  breathe. Editing submits one `update_task` patch (plus a `move_task_to_list`
  when `list` changed, ordered fields-then-move exactly as
  `useTaskActions.onEditorSave` does today, `:198-216`).
- **Header actions:** **Open in Obsidian** (the existing `open_task`),
  **Duplicate**, and **Delete** (inline confirm — §6), plus the standard
  ← back.
- **Relationship to the inline editor.** The compact inline `TaskEditor` stays —
  it's the fast, edit-in-place-without-leaving-the-list path. The detail view is
  the deliberate "open it up" home (adds description + the verbs + the Obsidian
  jump, and later subtasks). To avoid drift, the **pure patch-diff helper**
  (`buildTaskPatch(task, draft)` — the "only changed keys" logic currently
  inside `TaskEditor.buildPatch`, `TaskEditor.vue:37-56`) is extracted into a
  shared util used by both editors; the detail view augments the result with
  `description` separately.
- **Write layer.** Because the detail view can't reuse `useTaskActions` (which
  operates on `Tasks.vue`'s shared array), its writes live in a small
  `useTaskDetail` composable — its own optimistic-update-plus-toast handling for
  the single Task (simpler than the list case: no re-sort, no bucket), calling
  `update_task` / `move_task_to_list` / `delete_task` / `duplicate_task` and
  keeping its local copy consistent. The list re-fetches on back.

### 5. Duplicate

A new core fn + async command `duplicate_task(id, path)`. The design goal is a
**faithful** copy — body, extra frontmatter, description, unknown hand-added
keys all preserved — with only identity fields changed:

1. Resolve the vault + tasks root and **canonicalize + containment-assert** the
   source path (reuse the `update_task_fields`/`open_task` guard).
2. Read the source **bytes** (this is what preserves the body and every
   frontmatter key).
3. Read the source `title`, falling back to the source **filename stem** when
   absent (matching `list_tasks`' display); compute the new title
   `"<title> (copy)"`.
4. Decide the new id. Touch the configured id property ONLY when its name is a
   valid, non-reserved id key (never a foreign/reserved field). When present:
   **regenerate** it (`tasks::new_task_id()`) if IDs are enabled, else **strip**
   it — a copy must never inherit the source id, or the two would collide if the
   user later re-enables IDs (the ensure-id path never overwrites an existing
   value). (Codex P2, PR #76 — an earlier "leave it untouched when IDs are off"
   was wrong for the re-enable case.)
5. Apply `set_fields` to the source content with
   `[("title", Some(quoted new title)), ("status", Some("new")),
   (id_property, regenerate ? Some(new_id) : None)]` (the id entry only when the
   property is valid) — a valid Task always satisfies `set_fields`' `type: Task`
   + closed-fence precondition.
6. Write via the **collision-safe never-clobber create writer**
   (`write_note_collision_safe`, the same path `create_task` uses at
   `disk.rs:169`), deriving the filename from the new title (`task_basename`)
   into the **source's own directory** so the copy lands in the same List.
7. Return the landed **path** (which may carry a ` (N)` collision suffix); the
   detail view toasts success and its "Open" action launches that path in
   Obsidian. (Returning the path rather than a full `TaskDto` avoids a
   read-one-task helper; the list re-fetches on back regardless.)

### 6. Delete (permanent + hardened confirm)

A new core fn + async command `delete_task(id, path) -> Result<(), String>`:
resolve the vault + tasks root, **canonicalize + containment-assert** (reuse
`open_task`'s guard — a delete must never escape the tasks root), **re-read the
file and require `is_task`** (task folders may hold foreign files, and a listed
row could be swapped for a non-task file before the confirm lands — identity is
re-validated immediately before this irreversible write; Codex P1, PR #76), then
`std::fs::remove_file`. Async, off the main thread, mirroring the other task
writes.

Frontend: the detail view's Delete uses a **two-step inline confirm** (not a
native dialog), mirroring `TaskSectionMenu`'s delete-with-confirm and its
focus/Escape discipline (the GAP-27 class — focus the confirm on open,
`stopPropagation` on Escape so it doesn't bubble to the panel's own close). The
confirm **names the intent** and requires a deliberate second click. On confirm
→ `delete_task` → on success `back()` (the list re-fetches without the row) + a
"Task deleted" toast; on failure, toast and stay on the detail view.

**Deliberate departure, documented.** This is the app's first removal of a vault
file — every prior write creates/edits/moves, never deletes, and "the vault is
sacred" is a core principle. The choice (permanent + confirm) is the user's,
made with the trade-off explicit; it is recorded in `docs/Gaps.md` as a bounded,
intentional exception, with a recoverable model (system trash via the `trash`
crate, or a vault `.trash` move + Undo toast) named as the future refinement if
the irreversibility ever bites. The hardened confirm is the mitigation.

## Architecture

- **`core` (Linux-testable):**
  - `template.rs`: `yaml_quote_multiline` + `yaml_unquote_multiline` (leave
    `yaml_quote` untouched — it flattens newlines by design).
  - `tasks/parse.rs`: `description_field` reader (unescape, `#`-tolerant).
  - `tasks/list.rs` + `services/tasks/mod.rs`: `description` on `TaskItem` /
    `TaskDto` (+ `add_task`'s hand-built DTO literal), filtered through the
    unescape at the read boundary.
  - `tasks/disk.rs`: reserve `description` in `RESERVED_TASK_KEYS`; new
    `delete_task` and `duplicate_task` core fns (reusing the canonicalize +
    containment guard, the collision-safe create writer, and `set_fields`).
    `tasks/id.rs`: reserve `description` in its `RESERVED_TASK_KEYS`.
  - All unit-tested on Linux (no Tauri types), mirroring how `due`/`scheduled`
    landed.
- **Shell (`src-tauri/src/task_commands.rs`):** `TaskPatchDto` gains
  `description` / `clear_description`; `update_task` threads them through
  (`capture_note::yaml_quote_multiline`); `TaskDto` gains `description`; **two
  new async commands** `delete_task`, `duplicate_task`, registered in
  `lib.rs::generate_handler`. **IPC surface 71 → 73** (AGENTS.md table updated).
  Compile-gated on Linux (`npx tauri build --no-bundle`).
- **Frontend:** new `TaskDetail.vue` + `useTaskDetail` composable (+ the
  extracted `buildTaskPatch` util); store `openTaskDetail` / `taskDetailTask` +
  the `taskDetail` `back()` case; a one-line repoint of `TaskRow`'s title emit
  (plus the modifier pass-through). **`Tasks.vue` must not grow** — it is
  grandfathered over the 500-LOC cap at 521 and is the tracked GAP-65 split
  candidate; all new logic goes in the new files, and the title-repoint is
  net-neutral.

## Domain language

Adds one term to CONTEXT.md's ubiquitous language (via the `domain-modeling`
skill): **Description** — a Task's free-text detail, a frontmatter property of
the Task document (distinct from the note **body**, which the app still does not
edit, and from a **Todo**, an inline checklist line). Also names the **Task
Detail** surface. Code, UI copy, and commits use these terms.

## Error handling

- Every metadata/description write is an existing `update_task` path (atomic,
  containment-gated, never-clobber); a failed edit reverts the detail view's
  optimistic change and toasts.
- `delete_task` and `duplicate_task` are new but reuse the canonicalize +
  containment guard and (duplicate) the collision-safe writer; both return
  `Result<_, String>` and surface a toast on failure. A duplicate that would
  collide suffixes, never clobbers; a delete that fails leaves the file and the
  view intact.
- A malformed on-disk `description` reads as empty, never an error.
- No window/placement code is touched.

## Testing

- **Rust (`core`):** `yaml_quote_multiline`↔`yaml_unquote_multiline` for
  multi-line / embedded-quote / tab / literal-backslash-n values, and the
  unquoted-passthrough read; `description_field` decodes and ignores a `#`;
  `description` round-trip through `set_fields` (set / rewrite / clear,
  byte-preserving the rest); `render_task` still omits `description` (create
  byte-identical); the template filter drops a `description` key;
  `is_valid_id_property("description")` is now `false`. `delete_task` removes the
  file and **refuses a path outside the tasks root**; `duplicate_task` produces a
  collision-safe copy with a fresh-or-inherited id, `status: new`, `"(copy)"`
  title, and a **preserved body + extra frontmatter + description**.
- **Frontend (Vitest):** title click opens the detail view (plain) vs. Obsidian
  (Ctrl/⌘); the detail view renders the passed `AggTask` including description;
  a description edit sends `description` / `clearDescription`; delete's inline
  confirm gates the command, and success navigates back (with the list
  re-fetch); duplicate calls the command and toasts; "Open in Obsidian" calls
  `open_task`; **aggregate mode** writes target the row's own `vaultId`; the
  `back()` case restores aggregate-vs-per-vault mode.
- **Windows** remains where the end-to-end delete/duplicate + Obsidian
  round-trip are eyeballed — called out for the reviewer, not gating.

## Quality gates & docs

- `npm run lint && npm run check:loc && npm run check:quality &&
  npm run test:coverage`; `cargo fmt` / clippy `-D warnings` / tests for `core`
  and the shell compile-gate. Frontend coverage floors and the LOC/quality
  baselines are shrink-only as usual; **`Tasks.vue` stays at or below its 521
  baseline** (new code lives in new files).
- Docs updated in the same PR: **AGENTS.md** (the tasks-domain section — the
  Task Detail view, the `description` field, delete/duplicate as new sanctioned
  task writes with their guards — delete being the app's first vault-file
  *removal*; the IPC table's two new commands + the `update_task`/`list_tasks`
  field notes + the 71→73 count), **CONTEXT.md** (Description; Task Detail), the
  **task-management PRD** + the per-vault-task-list use case (detail /
  description / delete / duplicate shipped), and **docs/Gaps.md** (the
  permanent-delete departure; the two description-storage caveats; the
  description-in-`list_tasks`-payload trade-off).

## Rollout / compatibility

Additive and migration-free: `description` is a new optional field; the view
union and `back()` gain a case; no config or localStorage migration. A vault
with no descriptions and no use of the new verbs sees an unchanged list — the
only new surface is the detail view a title click now opens (with Obsidian one
click away, or Ctrl/⌘-click for the direct jump). The one intentional new
capability with real consequence is permanent delete, gated behind the confirm
and documented in Gaps.

## Suggested phasing for the plan

1. `core`: `yaml_quote_multiline`/`yaml_unquote_multiline`; `description` read +
   reserve in both key-sets + field-write round-trip; `delete_task` +
   `duplicate_task` core fns; tests.
2. Shell: `TaskPatchDto`/`TaskDto` gain `description`; `update_task` threads it;
   register `delete_task` / `duplicate_task`; compile-gate.
3. Frontend model: store `taskDetail` view + `openTaskDetail`/`taskDetailTask` +
   `back()` case; `ActionPanel` title + transition branch; `description` on the
   TS types; the `buildTaskPatch` extraction.
4. Frontend surface: `TaskDetail.vue` + `useTaskDetail` (metadata + description
   editing, Open-in-Obsidian, duplicate, delete-with-confirm); repoint
   `TaskRow`'s title emit (+ modifier).
5. Docs + baselines (AGENTS.md, CONTEXT.md, PRD, use case, Gaps).

Phases 1–2 (the model + verbs in Rust) and 3–4 (the surface) are independently
reviewable — a natural PR split if the branch merges early.
