# Task-management UX program — Subtasks & parent tasks — design

Date: 2026-07-25
Status: accepted (user request: "I want to build on this" — build on the just-merged
Task Detail surface (PR #76). Direction chosen: **subtask hierarchy** — unit A of
the four "task depth" units the Task Detail spec deferred. Sub-decisions the user
made during brainstorming: the parent reference is a **hybrid** (a stable Task ID
as the source of truth PLUS an Obsidian wikilink for navigation), and the
hierarchy is **Detail-first** — it lives on the Task Detail surface, with only a
light touch in the main task list.)

## Context

The task-detail increment (PR #76,
`2026-07-24-task-detail-surface-description-verbs-design.md`) shipped the Task
Detail surface: a full-height per-Task home with an editable `description`, the
lifecycle verbs Duplicate and permanent Delete, and the dates/priority/tags/List
editors. That spec named four "task depth" units and deliberately built only two
of them, noting that the detail surface exists precisely so the others can land
inside it:

| Unit | What it is | Status |
| --- | --- | --- |
| **A. Subtask hierarchy** | a parent field → a Task tree, "add subtask", reparenting | **this spec** |
| B. Description / detail | the detail home + editable description | shipped (PR #76) |
| C. Delete + duplicate | the two missing lifecycle verbs | shipped (PR #76) |
| D. Multi-select + bulk ops | select N rows → act together | later increment |

Subtask hierarchy is also the Task Management PRD's own cornerstone: the domain
model says a Task "optionally names a parent Task, so Tasks can form hierarchies
… without ever leaving the file system," and `Parent Task` is listed among the
Task properties. It is the one PRD-level Task property with no implementation.

What exists to build on, against the real code:

- **A stable, opt-in Task ID.** `task_id_enabled` + `task_id_property` per vault
  (default property name `task-id`), an 8-char base36 CSPRNG id
  (`tasks::new_task_id`), and an "ensure present, never overwrite" stamp built
  into the writer: `update_task_fields(root, path, updates, ensure_id)` generates
  the id internally only when the property has no usable value and **returns the
  effective id**. Every structural write path already stamps.
- **A surgical multi-key frontmatter writer.** `set_fields(content, &[(key,
  Option<value>)])` rewrites/inserts/removes whole keys, preserving CRLF, key
  order, unknown keys, and the body byte-for-byte; `update_task_fields` wraps it
  with canonical containment + an atomic replacing write.
- **A single recursive read.** `list_tasks` walks the tasks root once
  (`core::vault_walk`) and returns `TaskItem`s the frontend already loads for
  every view; `TaskDto` carries them over IPC (and to MCP).
- **A single-sourced reserved-key set.** `RESERVED_TASK_KEYS` (`tasks/mod.rs`,
  deduped in the PR #76 polish pass) is the one list both the
  template-frontmatter filter and the task-ID-property validator consult.
- **The detail surface itself** — `TaskDetail.vue` + `useTaskDetail` (one shared
  `busy` guard, optimistic-with-revert writes, `taskDetailBusy` gating the header
  Back and the panel's `refresh()`), and `openTaskDetail` / `back()` navigation.

Nothing in the task domain models a relationship between two Tasks today.

## Goals & scope (this increment)

- **A parent reference on a Task** — hybrid: `parent-id` (the parent's stable
  Task ID, authoritative) + `parent` (an Obsidian wikilink, for navigation and
  Dataview). Read leniently, written through the existing surgical writer,
  absent by default — a Task with no parent behaves exactly as today.
- **The Task Detail surface becomes the hierarchy home**: a Parent row (see /
  change / clear, with a picker) and a Subtasks section (progress, the child
  rows, and **Add subtask**). Arbitrary depth is navigated by drilling into a
  child's own detail.
- **Cycle prevention** in core: a parent assignment that would create a cycle is
  refused with an inline error, and every ancestor walk is bounded so a
  pre-existing hand-authored cycle can never hang the app.
- **Task IDs auto-enable** for a vault the first time a parent is set there
  (the link depends on them), stamping the parent's and child's ids.
- **A light touch in the main list**: a subtask-count badge on a parent and a
  parent chip on a child. Sorting, grouping (Lists / Plan / Tags), filtering,
  and drag-to-reorder are untouched.

### Non-goals (each is a later increment or a deliberate omission)

- **A nested tree in the main task list.** Children are not indented under
  parents, and there is no collapse/expand in the list. That interacts with
  every grouping mode, the sort selector, the filters, and the manual-rank drag
  — a much larger surface, deliberately deferred. The list stays flat.
- **A parent picker in the add composer.** Children are created from the parent's
  detail ("Add subtask") or by setting a parent on an existing Task. The
  composer and the compact inline editor gain no parent field.
- **Cascade delete / active orphan-clearing.** Deleting a parent leaves its
  children pointing at a now-unresolvable id; they simply render as top-level.
  `delete_task` stays exactly the single-file, identity-re-validated op PR #76
  built — no batch write is bolted onto the app's one destructive path.
- **Status coupling.** Completing a parent does not complete its children, and
  completing every child does not complete the parent. The parent shows
  *progress*, never enforcement.
- **Drag-to-reparent**, bulk subtask operations, and **cross-vault parents** (a
  parent is always in the same vault as its child).
- **Any change** to the task file format beyond the two additive keys, or to
  status-toggle / list / tag / order / date behavior.

## Design

### 1. The data model

A child Task carries two additive frontmatter keys:

```yaml
---
type: Task
status: new
title: "Write the migration guide"
created: 2026-07-25
task-id: k3m9x2qp
parent-id: ab12cd34
parent: "[[2026-07-04-prepare-release-cutover]]"
---
```

- **`parent-id` is authoritative.** All hierarchy resolution — children,
  ancestors, cycle checks — reads only this. Because a Task ID never changes once
  stamped, the hierarchy survives retitling, list moves, reordering, and
  duplication untouched.
- **`parent` is the Obsidian affordance**: a wikilink to the parent's file so the
  relationship is clickable inside Obsidian and resolvable by Dataview. It is
  written from the parent's **filename stem** in Obsidian's short form,
  `[[<stem>]]`, and — critically — **always YAML-quoted**: unquoted, `parent:
  [[foo]]` parses as a nested flow *sequence*, not a string. It is emitted
  through the existing `yaml_quote`, exactly as the document-import frontmatter
  does for paths.
- **Drift is benign by construction.** VB keeps the wikilink correct on the moves
  it performs, but a manual rename in Obsidian (or a rare collision-suffixed
  move) can stale it. Because `parent-id` is authoritative, a stale wikilink
  degrades only Obsidian's click-through — never VB's hierarchy — and is
  rewritten the next time the parent is set. This asymmetry is the whole point of
  the hybrid.

**Reading** is lenient, matching the rest of the vault domain: both keys are read
as single-line scalars via the `description`-style decode (`raw_scalar_field` +
`decode_scalar_lenient`); an empty, block (`|`/`>`), or flow (`[..]`/`{..}`)
value reads as **absent** rather than surfacing a partial/wrong value.

Resolution is a separate, later step from reading: `TaskItem.parent_id` carries
whatever id the file names, even one no task in the vault answers to. It is the
**index** that drops unresolvable ids, so an orphan (a child whose parent was
deleted) renders as top-level while its raw keys stay intact on disk.

**Writing** is strict and paired: `parent-id` and `parent` are always written
together and cleared together, so the two can never disagree about *whether*
there is a parent.

Both keys join `RESERVED_TASK_KEYS` (`tasks/mod.rs`) so a template can never seed
them and neither can be configured as the task-id property. As with `description`
(GAP-77) and `scheduled` (GAP-68), this closes a formerly-settable edge: a vault
that had configured the literal `parent` as its id property would have id
generation turn off, remedied by re-pointing the property. Documented in Gaps,
not auto-migrated — the established, precedent-consistent call.

### 2. Task IDs auto-enable

The hierarchy is keyed on Task IDs, so a vault with IDs off cannot express it.
Rather than blocking the user behind a settings trip, the first parent-set in a
vault turns IDs on:

- `services::set_task_parent` resolves the id property through the existing gate
  `tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name())`.
- When that yields `None` **because the feature is off**, the service enables it
  (a `ConfigWriteLock`-serialized read-modify-write of `task_id_enabled`, the
  `set_task_id_config` pattern) and proceeds with the vault's configured — or
  defaulted `task-id` — property name.
- When it yields `None` because the configured property is **invalid or
  reserved** (a hand-edited config), the service does **not** silently rewrite
  the user's setting: it returns an inline error naming the property, matching
  `set_task_id_config`'s write-strict posture.

Enabling is surfaced honestly: the Detail surface notes that setting a parent
turned on Task IDs for the vault, so the config change is never silent.

Both endpoints are then guaranteed to have ids:

1. **The parent** — `update_task_fields(root, parent_path, &[], Some(prop))`.
   The existing `ensure_id` stamps only when the property has no usable value and
   **returns the effective id**; with an empty `updates` slice and an id already
   present it short-circuits without a write. This is the id that goes into the
   child.
2. **The child** — the same call that writes its `parent-id`/`parent` passes
   `ensure_id`, so a legacy child picks up its own id in the same write.

### 3. Core: hierarchy resolution and cycle prevention

All of it is pure and Linux-testable, in a new `core/src/tasks/hierarchy.rs`:

```rust
/// Maps a task's own id -> its parent's id, for ONE vault's tasks. Tasks with
/// no id or no parent-id contribute no entry.
pub type ParentIndex<'a> = std::collections::HashMap<&'a str, &'a str>;

pub fn parent_index(tasks: &[TaskItem]) -> ParentIndex<'_>;

/// Ancestor ids of `start`, nearest first, EXCLUDING `start` itself. Bounded by
/// a visited set so a pre-existing hand-authored cycle terminates instead of
/// looping forever.
pub fn ancestors<'a>(index: &ParentIndex<'a>, start: &'a str) -> Vec<&'a str>;

/// True when making `parent` the parent of `child` would create a cycle:
/// `parent == child`, or `child` is an ancestor of `parent`.
pub fn would_create_cycle(index: &ParentIndex<'_>, child: &str, parent: &str) -> bool;
```

`would_create_cycle` is the gate `set_task_parent` consults before writing. The
bounded walk matters twice: the vault is user-editable, so a cycle can already
exist on disk before VB ever sees it, and `ancestors` is also what the detail
surface would use for any future breadcrumb.

Resolution needs the vault's task set. `set_task_parent` therefore runs one
`list_tasks` walk (already `spawn_blocking`-offloaded, the same walk every view
does) to build the index, checks the cycle, then writes. Children of a task are
derived the same way — no second scan, no index on disk.

### 4. IPC surface

Additive, no new read command:

- **`TaskItem` / `TaskDto`** gain `parent_id: Option<String>` and `parent_link:
  Option<String>` (camelCase `parentId` / `parentLink` over IPC), so every
  existing `list_tasks` caller — the panel and MCP alike — sees the hierarchy
  with no new round-trip.
- **`update_task`'s `TaskPatchDto`** gains `parent_id: Option<String>` +
  `clear_parent: bool`, following the established `due`/`scheduled`/`description`
  set-or-clear shape. The command resolves the wikilink and the cycle check
  itself — the frontend sends only the target parent's id (or the clear flag) and
  never composes a wikilink.
- **`add_task`** gains an optional `parent_id`, so "Add subtask" is one call.

Both write commands stay `async` (`spawn_blocking`), like every other task write.

### 5. Frontend: the Task Detail surface

Two additions to `TaskDetail.vue`, both driven by a new `useTaskHierarchy`
composable (keeping `TaskDetail.vue` under its LOC cap and the logic unit-testable):

- **Parent row.** Shows the current parent as a clickable chip (opening *its*
  detail, so you can walk up the tree) plus a **Change** / **Clear** control. The
  picker is a searchable list of the vault's other Tasks with the cycle-invalid
  ones (self + descendants) disabled and labelled, so the rule is visible rather
  than only enforced on save.
- **Subtasks section.** A progress line (`2 / 5 done`), each child as a compact
  row (status checkbox + title, clicking the title drills into that child's
  detail), and **Add subtask** — an inline title input that creates a child
  inheriting the parent's List and vault.

Every write rides the surface's existing discipline: the one shared `busy` guard
serializing save/delete/duplicate/parent-set, optimistic update with revert +
toast on failure, and `taskDetailBusy` gating the header Back and the panel's
`refresh()`. Child rows reuse the row-level busy set so a child's status toggle
can't race a parent write.

### 6. Frontend: the main list (light touch)

`TaskRow` gains two purely presentational affordances, both derived from the
already-loaded task set:

- a **subtask-count badge** on a parent showing its number of **open** (not-done)
  children, hidden entirely at zero — so a fully-completed parent carries no
  badge — built from the existing `CountBadge`/`Chip` primitives; the done/total
  progress line stays on the detail surface, and
- a **parent chip** on a child (the parent's title, click → the parent's detail).

In aggregate ("All tasks") mode the index is built **per vault** — a task's
parent is only ever resolved among tasks carrying the same `vaultId` — so two
vaults can never cross-link. Sorting, grouping, filtering, drag-to-reorder, and
the bucket logic are all unchanged: a child is an ordinary row wherever it
already sorted.

### 7. Interaction with the shipped lifecycle verbs

- **Duplicate** — a duplicated Task keeps its `parent-id`/`parent`, so the copy
  lands as a *sibling* under the same parent. This falls out of the existing
  "reset identity only" rule and is the right semantic. Duplicating a *parent*
  does not duplicate its children (they point at the original's id) — stated
  explicitly so the behavior is chosen, not incidental.
- **Delete** — unchanged. Children of a deleted parent become orphans that render
  as top-level (their stale keys are harmless and are rewritten on the next
  parent-set). The tidier alternative — best-effort clearing each child's parent
  keys, as `delete_task_list` relocates its tasks — is recorded in Gaps as the
  tracked option if orphan clutter ever proves real.
- **Move between Lists** — unchanged. The hierarchy is id-keyed and folder-blind,
  so a child may live in a different List from its parent; no move is implied or
  blocked.

## Architecture

```
core/src/tasks/
  hierarchy.rs   NEW  parent_index / ancestors / would_create_cycle (pure)
  parse.rs       +    lenient parent-id / parent readers
  list.rs        +    parent_id / parent_link on TaskItem
  mod.rs         +    parent-id + parent in RESERVED_TASK_KEYS
  create.rs      +    render_task writes an optional parent pair
core/src/services/tasks/
  mod.rs         +    set_task_parent (id auto-enable, cycle gate, paired write)
                 +    parent_id on add_task; parentId/parentLink on TaskDto
src-tauri/src/
  task_commands.rs +  parent_id/clear_parent on TaskPatchDto; parent on add_task
src/
  composables/useTaskHierarchy.ts  NEW  index, children, progress, verbs
  components/TaskDetail.vue        +    Parent row + Subtasks section
  components/TaskParentPicker.vue  NEW  searchable, cycle-aware picker
  components/TaskRow.vue           +    subtask badge / parent chip
```

The split follows the house rule: everything that doesn't need Tauri types lives
in `core` (so the cycle logic, the readers, and the writer are testable on
Linux), the service layer owns the config side-effect and the write orchestration
(so MCP and the panel share one chokepoint), and the shell only threads IPC.

## Domain language

Per CONTEXT.md, this increment adds **Parent Task** and **Subtask** to the
ubiquitous language, both referring to the *Task document* sense (never a Task
Tag or a Todo). CONTEXT.md gains both terms, and the PRD's "Parent Task" property
moves from envisioned to shipped.

## Error handling

- A cycle-creating assignment → inline error naming the conflict; nothing written.
- An invalid/reserved configured id property → inline error naming the property;
  IDs are not silently re-pointed and no parent is written.
- A parent that vanished between load and write → the write fails like any other
  missing-file write ("never a silent success"); the child is untouched.
- A failed parent-set → optimistic state reverts and a toast names the failure,
  matching every other detail write.
- A pre-existing on-disk cycle → bounded walks terminate; the affected rows
  render as top-level rather than hanging or recursing.

## Testing

**Core (Rust, Linux):** lenient reads of both keys (quoted, unquoted, empty,
block, flow, missing); the paired write and the paired clear; wikilink quoting;
`RESERVED_TASK_KEYS` filtering for both keys; `would_create_cycle` (self, direct,
transitive, and a pre-existing on-disk cycle terminating); `ancestors` bounded by
its visited set; `parent_index` ignoring unresolvable ids; `render_task` with and
without a parent (byte-identical to today when absent); duplicate preserving the
parent pair.

**Service:** id auto-enable on first parent-set (and the no-op when already on);
the invalid-property error path; parent id stamped when the parent lacked one.

**Frontend (Vitest):** parent chip navigation; picker disabling cycle-invalid
options; add-subtask creating with the parent and inheriting the List; progress
counting; the shared busy guard covering the parent write; orphan rendering as
top-level; per-vault scoping of the index in aggregate mode; list badge/chip
rendering.

## Quality gates & docs

All existing gates must stay green with no baseline loosening: ESLint,
`vue-tsc`, `check:loc`, `check:quality` (the ratchet), coverage floors,
`cargo fmt --check`, workspace clippy `-D warnings`, and the Rust coverage floor.
`useTaskHierarchy` + `TaskParentPicker` exist partly to keep `TaskDetail.vue`
inside its LOC cap. AGENTS.md (tasks domain, IPC table), CONTEXT.md (Parent Task,
Subtask), the Task Management PRD (Parent Task shipped), the per-vault task
use-case, and docs/Gaps.md (the reserved-`parent` edge, the orphan-clearing
option) are all updated in the same increment.

## Rollout / compatibility

Purely additive. A Task with no parent keys reads and writes exactly as today
(pinned by byte-identical `render_task` tests), so existing vaults are unchanged
until the user sets a first parent. The only config side-effect is enabling Task
IDs on first use, which is itself additive (ids are stamped, never overwritten)
and surfaced to the user. MCP's `list_tasks` gains two fields and no new tool.

## Suggested phasing for the plan

1. Core reads + `TaskItem`/`TaskDto` fields + reserved keys (additive, no UI).
2. `hierarchy.rs`: index, bounded ancestors, cycle check.
3. Service `set_task_parent`: id auto-enable, cycle gate, paired write; the
   `update_task` / `add_task` IPC surface.
4. `useTaskHierarchy` + the Detail Parent row and picker.
5. The Subtasks section + Add subtask.
6. The list badge + parent chip (incl. per-vault scoping in aggregate mode).
7. Docs sweep (AGENTS.md, CONTEXT.md, PRD, use-case, Gaps) + final review.
