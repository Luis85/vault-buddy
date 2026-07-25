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
- **`parent` is the Obsidian affordance**: a link to the parent's file so the
  relationship is clickable inside Obsidian and resolvable by Dataview. It is
  written as the parent's **vault-relative path without the `.md` extension** —
  `[[Tasks/Work/2026-07-04-prepare-release-cutover]]` — not the bare filename
  stem. Lists are separate folders and collision suffixing is folder-local, so
  two Lists can legitimately hold the same stem; a short-form `[[<stem>]]` would
  then be ambiguous and Obsidian could resolve it to the wrong Task, which is
  precisely the navigation this field exists to make reliable (Codex P2, PR #77).
  The path form is unambiguous by construction. The path is built against the
  **canonical** vault path, the same way `open_task` derives its vault-relative
  URI parameter (a lexical path would break `strip_prefix` against the canonical
  paths `list_tasks` hands out, notably Windows' `\\?\` form).
- **The link form adapts to the path, because wikilinks cannot express every
  legal List name.** A List is a folder, and `is_valid_list_name` rejects only
  empty / `/` / `\` / a leading dot — while a hand-created folder is a valid List
  and skips that check entirely. So `Project#1` or `A|B` are legitimate Lists,
  and inside `[[…]]` a `#` starts a heading target, `|` starts an alias, and `]]`
  terminates the link — silently pointing click-through at the wrong or a
  nonexistent note (Codex P2, PR #77). Wikilinks have no escape mechanism for
  these, so:
  - **Default — wikilink**: `[[<vault-relative path>]]`, used whenever the path
    contains none of ``# | [ ] ^``.
  - **Fallback — markdown link**: `[<escaped parent title>](<percent-encoded
    path>.md)` when it does. Obsidian resolves markdown links natively and
    percent-encoding represents every character unambiguously; the app already
    percent-encodes every `obsidian://` parameter in `uri.rs`, so this reuses an
    established convention rather than inventing one.

    **The label is escaped too, not just the target.** A title carrying `[`, `]`,
    or `\` would otherwise produce a malformed link even with the path encoded —
    YAML quoting protects the surrounding *scalar*, but the Markdown inside it is
    parsed after YAML decoding (Codex P2, PR #77). Each of `\`, `[`, `]` in the
    title is backslash-escaped when composing the label.

  Both forms are — critically — **always YAML-quoted**: unquoted, `parent:
  [[foo]]` parses as a nested flow *sequence*, not a string. Both are emitted
  through the existing `yaml_quote`, exactly as the document-import frontmatter
  does for paths. Task *filenames* need no such care: `slugify` reduces them to
  ASCII alphanumerics and hyphens, so only the List folder segments can carry
  metacharacters.
- **Drift is bounded, and the link is never repaired in bulk.** A parent that is
  moved between Lists, renamed in Obsidian, or landed under a collision suffix
  leaves the link recorded on its children stale. VB does **not** refresh those
  links — `move_task_to_list` is unchanged by this increment (§7), because
  repairing them would mean bolting a batch write onto the move path, the same
  thing this spec declines to do to delete. Because `parent-id` is authoritative,
  a stale link degrades only Obsidian's click-through — never VB's hierarchy —
  and it self-heals the next time that parent is set. This asymmetry is the whole
  point of the hybrid.

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

`parent-id` is emitted **bare when the id is a plain-safe token** (`[A-Za-z0-9_-]+`,
which every generated base36 id is — matching how the id property itself is
written, so the two lines look consistent in one file) and **YAML-quoted
otherwise**. The quoting is not hypothetical: `ensure_id` preserves *any* usable
non-empty existing value, not only generated ones, so a hand-authored
`task-id: "[legacy]"` resolves to the effective id `[legacy]`. Emitting
`parent-id: [legacy]` bare would make it a YAML flow *sequence*, which the lenient
reader then rejects — leaving the relationship unresolved the instant it was
written (Codex P2, PR #77). Both write paths — `set_task_parent` and the
create path — quote through the same helper.

Both keys join `RESERVED_TASK_KEYS` (`tasks/mod.rs`) so a template can never seed
them and neither can be configured as the task-id property. As with `description`
(GAP-77) and `scheduled` (GAP-68), this closes a formerly-settable edge: a vault
that had configured the literal `parent` as its id property would have id
generation turn off, remedied by re-pointing the property. Documented in Gaps,
not auto-migrated — the established, precedent-consistent call.

### 2. Addressing the parent by PATH, and Task IDs auto-enable

**A parent is named by its path, never by its id.** This is forced, not
stylistic: `services::list_tasks` resolves the id property through
`id_property_for_generation(cfg.task_id_enabled, …)`, which yields `None` when
the feature is off — the per-vault **default** — so `tasks::list_tasks` never
reads the property and **every `TaskItem.id` is `None`**. A frontend that had to
name the parent by id could therefore never name one in a fresh vault, and the
advertised first-use auto-enable could never fire: a bootstrap deadlock (Codex
P1, PR #77). Paths have no such problem — `TaskItem.path` is always populated,
and path is already the identity every other task write takes (`update_task`,
`move_task_to_list`, `delete_task`, `duplicate_task`).

So the frontend sends the **parent's path**; the service resolves it to an
authoritative id, stamping one if needed, and returns that id so the row can
reflect it without a reload (the `update_task` precedent).

The hierarchy is keyed on Task IDs, so a vault with IDs off cannot express it.
Rather than blocking the user behind a settings trip, the first parent-set in a
vault turns IDs on.

**Ordering: every validation precedes every side effect.** `set_task_parent(root,
child_path, parent_path, cfg)` runs in three strictly separated phases. The
earlier draft of this spec stamped ids *before* the cycle check, so a rejected
self-parent could still have enabled IDs and written a stamp — contradicting this
spec's own guarantee that a cycle rejection writes nothing (Codex P2, PR #77).

**Phase 1 — validate (no writes, nothing mutated):**

- Canonicalize both paths and require containment inside the vault's tasks root;
  both must be `type: Task` documents. A vanished or escaping path fails here.
- Refuse a **self-parent** by comparing the two canonical *paths* — no ids
  required, so this rejection is available before anything is stamped.
- Resolve the id property name and reject an **invalid or reserved** one
  (`tasks::is_valid_id_property`) with an inline error naming it — the service
  never silently rewrites the user's setting, matching `set_task_id_config`'s
  write-strict posture. A property that is merely *disabled* is expected here and
  is enabled in phase 2.
- Build the validation index by **reading that property unconditionally — even
  while generation is disabled** — then run the **ambiguity and cycle checks
  against the pre-stamp state**.

  Reading dormant ids is required for soundness, not an optimization. A
  hand-authored Task document is explicitly part of this domain (`type: Task` is
  the identity), so a vault with IDs *off* can still contain ids and parent links
  that a user or a previous configuration wrote. The ordinary `list_tasks` walk
  suppresses ids in that state, so an index built from it would be empty and the
  cycle check would pass vacuously: with `A(id=a, parent-id=b)` and `B(id=b)`,
  making A the parent of B would be accepted, and phase 3 would then write
  `B.parent-id=a` — a genuine A↔B cycle created by a check that reported none
  (Codex P2, PR #77). "Generation is off" never proves "no ids exist."

  This is deliberately **not** the decoupling §2a rejects. That rejection is about
  what `list_tasks` *surfaces* to the panel and MCP, which stays gated on the
  enabled flag and is unchanged here. This is an internal read inside
  `set_task_parent`, used only to validate one write.

  The check runs **over paths** (§3), so an id-less task's own outgoing edge is
  still represented and the check is never skipped for want of an id. With
  dormant ids read and the graph keyed on paths, every recorded edge is visible
  and the pre-stamp check is complete.

  **The scan must include archived Tasks.** `list_tasks` deliberately drops
  `status: archived` documents — it is a *presentation* function — but an
  archived Task's file still carries its `parent-id`. Building the validation
  index from the filtered list would silently erase every edge through an
  archived Task, so a cycle routed through one (`A → B(archived) → C`) would be
  invisible and the write would create it. Hierarchy scans therefore use an
  **unfiltered** walk of every `type: Task` document; the archived filter belongs
  to the views, not to a structural invariant (Codex P2, PR #77 — raised against
  the §2a guard, which has the same root cause and the same fix).

**Phase 2 — enable (idempotent, additive):** if IDs are off, enable them (a
`ConfigWriteLock`-serialized read-modify-write of `task_id_enabled`, the
`set_task_id_config` pattern) using the vault's configured — or defaulted
`task-id` — property name. Enabling is surfaced honestly: the Detail surface
notes that setting a parent turned Task IDs on for the vault, so the config
change is never silent.

**Phase 3 — write:**

1. **The parent** — `update_task_fields(root, parent_path, &[], Some(prop))`.
   The existing `ensure_id` stamps only when the property has no usable value and
   **returns the effective id**; with an empty `updates` slice and an id already
   present it short-circuits without a write. This is the id that goes into the
   child, and the parent's canonical path is what the link is derived from, so
   both halves of the pair come from this one resolution.
2. **The child** — the same call that writes its `parent-id`/`parent` passes
   `ensure_id`, so a legacy child picks up its own id in the same write.

**Phases 2–3 run under one `ConfigWriteLock`, held across both, and the config is
re-checked once it is held.** A concurrent `set_task_id_config` would otherwise
interleave: it could scan, see no parent links yet, allow a disable or property
re-point, and commit it while this assignment is still mid-flight — orphaning the
new hierarchy the instant it is written. Serializing only phase 2's config
read-modify-write is not enough, and an assignment that finds IDs *already
enabled* would not take the lock at all (Codex P2, PR #77).

Holding the lock across the writes closes the write-vs-write race but not a
read-then-write staleness: phase 1 reads the config and picks the id property
*before* the lock exists, so a settings save can commit a different property in
between and phase 3 would then write both ids under the stale one (Codex P2,
PR #77). Two options — acquiring the lock before phase 1 would hold it across a
full vault walk, serializing every unrelated config write for the duration of a
scan. So instead: **after acquiring the lock, re-read the config and compare it
to the phase-1 snapshot; if `(task_id_enabled, property)` changed, re-run phase
1's property validation and cycle check under the lock before proceeding.** This
costs nothing in the common case (unchanged → proceed), and is cheap exactly when
it can happen: `set_task_id_config` refuses a change once parent links exist
(§2a), so the interleaving is only reachable on a vault's *first* hierarchy
operation, where the index is empty and re-validation is trivial.

**Implementation note — do not re-acquire.** The lock is not reentrant, so the
enable step inside the held scope must be a `*_locked` helper that performs the
read-modify-write *without* taking the lock again; a nested acquire would
self-deadlock. This briefly serializes unrelated config writes (capture settings
and the like) for the duration of two small task-file writes — an accepted,
bounded cost for closing the race.

**One shared resolve-the-parent path, used by set AND create.** Phases 1–3's
parent half — validate, lock, re-check, enable, stamp the parent, compose the
link — is factored into a single helper returning `(parent_id, link)` with the
lock still held. `set_task_parent` then writes the pair onto an existing child;
`add_task` passes the same pair into `create_task` for a brand-new one. Creating
a subtask must run the **whole** path, not just the read-only validation:
"Add subtask" is very often a vault's *first* hierarchy operation, where IDs are
off and the parent is unstamped, so validation alone would leave no authoritative
`parent-id` to write (Codex P1, PR #77).

**On transactionality.** Phases 2–3 are still three separate writes and this app
has no journal, so they are not atomic even under the lock; the ordering is
chosen so that **every partial state is benign and self-correcting**. Enabling
IDs and stamping an id are both additive and idempotent — exactly what any later
edit would do anyway — and the one meaningful write, the child's pair, comes
last. A failure or crash mid-sequence can therefore leave an enabled flag and/or
a stamped parent id, but it can **never** leave a `parent-id` that no task
answers to. Claiming true transactionality here would be dishonest; guaranteeing
no dangling reference is both achievable and the property that actually matters.

### 2a. The ID configuration is locked while hierarchies exist

Resolution reads each task's own id through
`id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name())`,
so **both** inputs to that gate can break every `parent-id` reference at once:

- **Re-pointing the property** (`task-id` → `uid`) makes `list_tasks` read a key
  no task carries — every id reads `None`.
- **Disabling IDs** makes the gate return `None`, so the property is never read
  at all — every id reads `None`, identically.

Either way no task answers to the recorded ids and the whole hierarchy silently
renders as orphaned while the data sits intact on disk (Codex P2 ×2, PR #77 — the
first revision of this spec carved out disabling as "unaffected", which
contradicted its own reasoning: the gate treats the two inputs the same, so the
guard must too).

`set_task_id_config` therefore **refuses any change to the vault's ID
configuration — the property name or the enabled flag — while the vault has tasks
carrying `parent-id`**, with an inline error naming the count and the remedy. That
scan is the **unfiltered** one (§2): an archived Task's file still carries its
`parent-id`, so reusing the presentation-filtered `list_tasks` would let the
settings change through and orphan those links the moment the Task is
unarchived. It
holds the `ConfigWriteLock` across **both** the parent-link scan and the write,
so a concurrent `set_task_parent` (which holds the same lock through its phases
2–3, §2) cannot slip a new hierarchy in between this scan and its commit.
Rationale, weighed against the alternatives:

- **Auto-migrating** (rewriting every task's id property and every reference) is
  the mass vault mutation this app forbids — the same reasoning that made
  GAP-68/GAP-77 document-only rather than migrate.
- **Warning and proceeding** silently breaks a structure the user built; the
  house posture on settings writes is strict-and-inline, not best-effort.
- **Decoupling hierarchy reads from the enabled flag** (always read the property
  when parent links exist) was considered and rejected: it makes `list_tasks`'
  id semantics depend on vault *content*, so the same vault config would surface
  ids to MCP or not depending on whether a parent happens to exist — a subtler
  surprise than a refused setting.
- **Refusing** is recoverable and honest — one rule, both inputs, and the user is
  told exactly why and how to proceed (clear the parent links first).

The lock is scoped to vaults that actually use hierarchies; a vault with no
parent links keeps today's fully-editable ID settings. A guided migration remains
the tracked future option in Gaps if this ever proves too strict in practice.

### 3. Core: hierarchy resolution and cycle prevention

All of it is pure and Linux-testable, in a new `core/src/tasks/hierarchy.rs`:

```rust
/// Child PATH -> parent PATH, for ONE vault's tasks. Keyed on paths, with the
/// edges resolved THROUGH ids (each task's `parent-id` is looked up against the
/// set's own ids). An id carried by more than one task resolves to nothing (see
/// "Ambiguous ids" below).
pub type ParentIndex<'a> = std::collections::HashMap<&'a Path, &'a Path>;

pub fn parent_index(tasks: &[TaskItem]) -> ParentIndex<'_>;

/// Ids carried by more than one task in the set — ambiguous, so unusable as a
/// parent reference. Callers surface these; the index resolves no edge to them.
pub fn ambiguous_ids(tasks: &[TaskItem]) -> std::collections::HashSet<&str>;

/// Ancestor paths of `start`, nearest first, EXCLUDING `start` itself. Bounded
/// by a visited set so a pre-existing hand-authored cycle terminates instead of
/// looping forever.
pub fn ancestors<'a>(index: &ParentIndex<'a>, start: &'a Path) -> Vec<&'a Path>;

/// True when making `parent` the parent of `child` would create a cycle:
/// `parent == child`, or `child` is an ancestor of `parent`.
pub fn would_create_cycle(index: &ParentIndex<'_>, child: &Path, parent: &Path) -> bool;
```

**Why the graph is keyed on paths, not ids.** Ids are how an edge is *recorded*
on disk, but they are not a total addressing scheme: a task can lack an id while
still naming a parent. Consider a hand-authored or legacy `P` with no id of its
own but carrying `parent-id: c`, alongside `C(id: c)`. An id-keyed index — keyed
on each task's *own* id — would drop P's outgoing edge entirely, and a cycle
check would additionally be *skipped* for want of a parent id. Making P the
parent of C would then pass, and phase 3 would stamp P and write `C.parent-id =
P` — closing a P↔C cycle the check had just called clean (Codex P2, PR #77).

Every task has a path, so a path-keyed graph represents every node — id-less
ones included — and resolving each `parent-id` through the id→path map keeps
ids as what they are: the on-disk encoding of an edge, not the identity of a
node. Lacking an id only means a task cannot be *referenced* as a parent; it
never means the task has no parent of its own.

`would_create_cycle` is the gate `set_task_parent` consults before writing. The
bounded walk matters twice: the vault is user-editable, so a cycle can already
exist on disk before VB ever sees it, and `ancestors` is also what the detail
surface would use for any future breadcrumb.

**Ambiguous ids.** Two Task documents can carry the same id — not from anything
VB does (`duplicate_task` regenerates, and `ensure_id` never overwrites) but from
a file copied in Explorer/Finder, a sync conflict, or a hand edit. VB's own
never-overwrite discipline then *preserves* the duplicate. A naive
id→parent-id `HashMap` would silently collapse the two into whichever was
indexed last, so a child referencing that id would resolve to an arbitrary
parent and a cycle walk could follow the wrong edge — potentially clearing a
write that does create a cycle (Codex P2, PR #77).

So an id carried by more than one task is treated as **unresolvable**, matching
the vault domain's defensive-read posture everywhere else (ambiguous or
malformed reads as absent; never guess):

- `parent_index` omits ambiguous ids entirely, so no edge is invented and the
  cycle walk can never traverse a wrong one.
- A child referencing an ambiguous id renders as a top-level orphan.
- `set_task_parent` **refuses** a parent whose id is ambiguous, with an inline
  error saying two Tasks share that id — the one case where staying silent would
  let the user build a hierarchy on an identity that cannot be resolved back.

The condition is self-healing: the user edits or clears one of the duplicate ids
and the link resolves normally.

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
- **`update_task`'s `TaskPatchDto`** gains `parent_path: Option<String>` +
  `clear_parent: bool`, following the established `due`/`scheduled`/`description`
  set-or-clear shape — but keyed on the parent's **path**, for the bootstrap
  reason in §2. The command owns everything derived: it validates, resolves the
  path to an authoritative id (stamping if needed), and composes the link in
  whichever form that path requires (§1). The frontend never composes a link and
  never needs an id.
- **`update_task`'s return** extends from the task's own id to also carry the
  **effective `parentId`/`parentLink`** actually written, so the detail row
  reflects a freshly-stamped parent without a reload — the same reason it already
  returns the stamped id.
- **A parent-only patch must not be mistaken for an empty one.** `update_task`
  no-ops an empty patch, and the Parent picker's Change/Clear sends exactly
  `{parentPath}` or `{clearParent: true}` with no ordinary field updates — so the
  relationship fields have to count toward "is there anything to do?", and be
  dispatched even when no scalar field changed. Otherwise every picker action is
  a silent no-op (Codex P1, PR #77).
- **A combined patch validates the parent BEFORE writing any scalar field.** An
  IPC caller may legally send a title change *and* a parent assignment in one
  patch. Writing the fields first and then failing parent validation would commit
  the title while returning an error — the frontend reverts its whole optimistic
  patch and reports failure, yet the title is changed on disk, surfacing as
  divergence on the next reload (Codex P2, PR #77). Running the read-only
  validation first means a rejected parent writes *nothing*, preserving the §2
  guarantee. A parent failure that happens later, at the *write* stage, is a
  genuine partial state no ordering can remove without a journal; it is reported
  in the fields-saved form ("Saved fields, but couldn't set the parent: …"),
  reusing the exact pattern `useTaskDetail` already applies to a failed list move.
- **`add_task`** gains an optional `parent_path`, so "Add subtask" is one call —
  running the full shared resolve path (§2), not validation alone.

Both write commands stay `async` (`spawn_blocking`), like every other task write.
Both resolve `parent_path` under the same canonical-containment gate as the child
path, so a parent outside the vault's tasks root is refused rather than linked.

### 5. Frontend: the Task Detail surface

Two additions to `TaskDetail.vue`, both driven by a new `useTaskHierarchy`
composable (keeping `TaskDetail.vue` under its LOC cap and the logic unit-testable):

- **Parent row.** Shows the current parent as a clickable chip (opening *its*
  detail, so you can walk up the tree) plus a **Change** / **Clear** control. The
  picker is a searchable list of the vault's other Tasks with the cycle-invalid
  ones (self + descendants) disabled and labelled, so the rule is visible rather
  than only enforced on save. Picking one sends that Task's **`path`** — the
  field every row already carries whether or not IDs are on (§2) — so the picker
  works identically in a fresh vault and an ID-enabled one.

  Cycle-invalid options are computed from the ids the frontend *can* see. In a
  vault with IDs still off no row has an id, so the index is empty and nothing is
  pre-disabled — correctly, since a vault with no ids has no parent links and
  therefore no reachable cycle. The core check in §3 remains the authority in
  every case; the disabling is an affordance, never the gate.
- **Subtasks section.** A progress line (`2 / 5 done`), each child as a compact
  row (status checkbox + title, clicking the title drills into that child's
  detail), and **Add subtask** — an inline title input that creates a child
  inheriting the parent's List and vault.

Every write rides the surface's existing discipline: the one shared `busy` guard
serializing save/delete/duplicate/parent-set, optimistic update with revert +
toast on failure, and `taskDetailBusy` gating the header Back and the panel's
`refresh()`. Child rows reuse the row-level busy set so a child's status toggle
can't race a parent write.

**A parent-set must write the stamped id back onto the PARENT's cached row.** In
the default IDs-off state the loaded parent row has `id: null`; the backend
enables IDs, stamps the parent, and returns its `parentId`. Updating only the
child's optimistic fields would leave the parent's cached `id` null — and since
the frontend resolves parents and children by comparing ids, the relationship the
user just created would be **invisible until a reload** (Codex P1, PR #77). So the
response's `parentId` is written onto the selected parent row as well as the
child's `parentId`. The regression test must run with IDs **off** — a fixture with
pre-stamped parents cannot catch this.

**Drilling detail→detail must remount the surface.** This increment introduces
the first navigation from one Task Detail to another (a parent chip or a child
row). `openTaskDetail` only swaps `store.taskDetailTask`, the view stays
`taskDetail`, and `ActionPanel` renders `<TaskDetail>` **unkeyed** — while
`TaskDetail.vue` seeds all seven draft refs once in `setup` with no watcher on
`props.task`. Without a change, drilling would keep showing the previous task's
fields while `useTaskDetail`'s `toRef` already points at the new path, so Save
would write the old task's values onto the newly opened one (Codex P1, PR #77).
Today that is unreachable only because every detail open comes via the list,
which unmounts the component.

The fix is to **key the component by task path** in `ActionPanel`
(`:key="store.taskDetailTask.path"`), forcing a full remount: fresh drafts, a
fresh list load, a reset delete-confirm, and the on-mount focus. Keying beats a
props watcher because it resets *every* piece of local state, including the ones
a future field would add — no watcher to keep in sync. Tests must assert the
**rendered field values** change on drill-through, not merely the store path.

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
- **Move between Lists** — unchanged, with one honest consequence of the
  vault-relative link form (§1): moving a **parent** changes its path, so the
  `parent` wikilink recorded on each of its children goes stale. `parent-id` is
  authoritative, so **VB's hierarchy is completely unaffected** — only Obsidian's
  click-through degrades, and it degrades *visibly* (an unresolved link) rather
  than silently navigating to the wrong Task, which is exactly the failure the
  path form exists to prevent. The link self-heals the next time that parent is
  set. Refreshing every child's link on a parent move would mean bolting a batch
  write onto the move path — the same thing this spec declines to do to delete —
  so it is recorded in Gaps as the tracked option instead.

  This is the deliberate trade of the two link forms: a bare stem survives moves
  but can silently resolve to the wrong Task; a path is move-fragile but never
  wrong. Since the id already guarantees correctness, the link is optimized for
  *never misleading*, not for surviving every edit.

## Architecture

```
core/src/tasks/
  hierarchy.rs   NEW  parent_index / ambiguous_ids / ancestors /
                      would_create_cycle (pure)
  parse.rs       +    lenient parent-id / parent readers
  list.rs        +    parent_id / parent_link on TaskItem
  mod.rs         +    parent-id + parent in RESERVED_TASK_KEYS
  create.rs      +    render_task writes an optional parent pair
core/src/services/tasks/
  mod.rs         +    set_task_parent, 3 phases: VALIDATE (containment, is_task,
                      self-parent by path, id-property, ambiguity + cycle on the
                      pre-stamp index) -> ENABLE ids -> WRITE (stamp parent,
                      then the child's pair). No side effect before validation.
                 +    parent_path on add_task; parentId/parentLink on TaskDto
                 +    set_task_id_config lock: refuse ANY id-config change
                      (property name or enabled flag) while any task carries
                      parent-id (2a)
src-tauri/src/
  task_commands.rs +  parent_path/clear_parent on TaskPatchDto; parent_path on
                      add_task; update_task returns the effective parent pair
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

- A cycle-creating assignment (including self-parent, caught by path) → inline
  error naming the conflict; **nothing written and no config touched**, since all
  validation runs in phase 1 before any side effect (§2).
- An invalid/reserved configured id property → inline error naming the property;
  IDs are not silently re-pointed and no parent is written.
- An **ID-configuration change** while parent links exist — the property name or
  the enabled flag — → inline error naming the count and the remedy (§2a); the
  setting is unchanged.
- A parent whose id is **ambiguous** (shared with another Task) → inline error
  naming the collision; nothing written (§3).
- A parent path outside the vault's tasks root → refused by the same containment
  gate as the child path; nothing written.
- A parent that vanished between load and write → the write fails like any other
  missing-file write ("never a silent success"); the child is untouched. Because
  the parent is stamped before the child is written, a failure to stamp aborts
  the whole set — the child never gets a `parent-id` no task answers to.
- A failed parent-set → optimistic state reverts and a toast names the failure,
  matching every other detail write.
- A pre-existing on-disk cycle → bounded walks terminate; the affected rows
  render as top-level rather than hanging or recursing.

## Testing

**Core (Rust, Linux):** lenient reads of both keys (quoted, unquoted, empty,
block, flow, missing); the paired write and the paired clear; link quoting **and
the vault-relative path form** (including two Lists holding the same stem, the §1
ambiguity case); **the link-form switch — a wikilink for an ordinary path, and a
percent-encoded markdown link for a List containing `#`, `|`, `[`, `]`, or `^`,
with the fallback's LABEL backslash-escaping `\`, `[`, `]` in the parent title**;
`RESERVED_TASK_KEYS` filtering for both keys;
`would_create_cycle` (self, direct, transitive, and a pre-existing on-disk cycle
terminating); `ancestors` bounded by its visited set; `parent_index` ignoring
unresolvable ids; **`ambiguous_ids` detecting a duplicated own-id, `parent_index`
omitting it, and a cycle walk therefore never following a collapsed edge**;
**an ARCHIVED task's edges present in the hierarchy index and counted by the
settings guard (the unfiltered scan, §2) — including a cycle routed
`A → B(archived) → C`**; **a `parent-id` whose value is not plain-safe
(a hand-authored `task-id: "[legacy]"`) written QUOTED and reading back
equal**; **an ID-LESS task's outgoing edge still present in the path-keyed index — the
`P(no id, parent-id: c)` + `C(id: c)` case, where making P the parent of C must
be refused as a cycle rather than skipped for want of a parent id**;
`render_task` with and without a parent (byte-identical to today when absent);
duplicate preserving the parent pair.

**Service:** **the IDs-off bootstrap — set a parent in a vault with IDs disabled
and assert it enables IDs, stamps BOTH tasks, and writes a resolvable link**
(the P1 regression); the no-op when IDs are already on; the invalid-property
error path; **the ID-configuration lock while parent links exist — refusing BOTH
a property re-point AND a disable — and both still allowed in a vault with no
parent links**; a parent whose id is ambiguous refused; a parent path outside the
tasks root refused; a failed parent stamp leaving the child unwritten.
**Phase separation (§2): a rejected self-parent and a rejected cycle each leave
IDs still disabled and NO id stamped on either task** — the regression that
proves validation precedes every side effect. **Dormant ids (§2): with IDs
disabled but hand-authored `A(id=a, parent-id=b)` / `B(id=b)` on disk, making A
the parent of B is REFUSED as a cycle** — the regression proving validation reads
the id property even while generation is off. **Lock scope: `set_task_parent`
holds the `ConfigWriteLock` across phases 2–3 and `set_task_id_config` holds it
across scan-and-write, so the two serialize in either interleaving — and the
enable step inside the held scope must NOT re-acquire it (a nested acquire
self-deadlocks; assert the happy path completes rather than hanging).**
**The post-lock config re-check: a property changed between phase 1 and the lock
must not be written under the stale property. Add-subtask with IDs OFF must
enable, stamp the parent, and create a child carrying a resolvable `parent-id` —
the bootstrap regression for the create path.**

**Frontend (Vitest):** **an IDs-OFF parent-set writes the returned id onto the
parent's cached row, so the new relationship renders without a reload**;
**drilling from one detail to another re-seeds the drafts
— assert the rendered title/description inputs show the NEW task, not the
previous one, and that a Save after drilling writes the new task's path**;
**a parent-only patch (`{parentPath}` / `{clearParent}`) actually dispatches
rather than no-opping as an empty patch**; parent chip navigation; the picker
sending a **path**
(and working with no ids surfaced); picker disabling cycle-invalid options when
ids exist; add-subtask creating with the parent and inheriting the List;
progress counting; the shared busy guard covering the parent write; orphan
rendering as top-level; per-vault scoping of the index in aggregate mode; list
badge/chip rendering.

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

The one **restriction** this increment introduces is §2a: once a vault has parent
links, its Task ID configuration is locked — neither the property name nor the
enabled flag can change until those links are cleared, since either would make
every recorded reference unresolvable. A vault with no hierarchy keeps today's
fully-editable ID settings.

## Suggested phasing for the plan

1. Core reads + `TaskItem`/`TaskDto` fields + reserved keys (additive, no UI).
2. `hierarchy.rs`: index (omitting ambiguous ids), `ambiguous_ids`, bounded
   ancestors, cycle check.
3. Service `set_task_parent`, in the §2 phase order — **VALIDATE** (containment,
   `is_task`, self-parent by path, id-property validity, then ambiguity + cycle
   on an index built by reading the id property *unconditionally*) → **ENABLE**
   ids → **WRITE** (stamp the parent, then the child's pair). No enable and no
   stamp may precede the cycle gate. Then the `update_task` / `add_task` IPC
   surface (path-keyed) and the extended return.
4. The §2a `set_task_id_config` lock (property name AND enabled flag).
5. `useTaskHierarchy` + the Detail Parent row and path-sending picker.
6. The Subtasks section + Add subtask.
7. The list badge + parent chip (incl. per-vault scoping in aggregate mode).
8. Docs sweep (AGENTS.md, CONTEXT.md, PRD, use-case, Gaps) + final review.
