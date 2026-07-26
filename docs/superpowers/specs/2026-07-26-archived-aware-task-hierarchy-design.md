# Task-management UX program — Archived-aware task hierarchy — design

Date: 2026-07-26
Status: accepted (user request: "let us tackle open gaps from the GAPS.md".
Scope chosen during brainstorming: the **archived-aware hierarchy** bundle —
GAP-90, GAP-91 (all three facets) and GAP-92, which `docs/Gaps.md` explicitly
records as one piece of work. Sub-decisions the user made: the
archived-**status** parent rule is enforced in **core** as the authority with a
frontend hint (not frontend-only), and Add Subtask **keeps inheriting** the
parent's list while **disclosing** when that list is archived.)

## Context

The subtasks & parent-tasks increment (PR #77,
`2026-07-25-task-subtasks-and-parent-tasks-design.md`) shipped the hierarchy:
a `parent-id`/`parent` pair on a child Task, `set_task_parent` / `add_subtask`
behind one shared validate → enable → write path, a Parent row and Subtasks
section on Task Detail, and a parent chip + open-subtask badge on the main list.

That increment's own final review filed four gaps in one family. They share a
single root cause: **the hierarchy code — frontend and core alike — has no
notion of archiving**, neither a Task's own `status: archived` nor a List
sitting in the vault's `archivedLists` set.

Archiving already has a well-established rule everywhere else in the app.
`archivedMatcher` (`src/utils/taskSections.ts`) is the one frontend
implementation of "is this list archived", consumed by the Lists grouping
(`useTaskDisplay`), the composer/editor list pickers (`useTaskLists`), and the
settings card; `count_open_tasks` (core) enforces the equivalent
case-insensitive exclusion so the vault-row badge agrees with the default Lists
view. The hierarchy surfaces shipped without joining that set.

What exists to build on, against the real code:

- **One shared phase-1 validator.** `validate_parent_assignment`
  (`core/src/services/tasks/parent/mod.rs`) is the single validation entry point
  for **both** `set_task_parent` and `add_subtask`. It already loads
  `all = tasks::list_tasks_structural(root, Some(&prop))` — archived-inclusive
  and fallible — and already looks a task up by path against it
  (`reject_ambiguous_parent`). Every refusal it raises happens before any side
  effect: nothing stamped, Task IDs still off.
- **A per-vault archived set on the frontend.** `TaskDetail.vue` already loads
  `archivedLists` from `get_tasks_config` for its List picker;
  `useTaskLists` caches a `vaultConfigs` map keyed by vault id.
- **One shared hierarchy rule.** `src/utils/taskHierarchy.ts` mirrors
  `core::tasks::hierarchy` exactly and is the only resolution implementation the
  frontend has — Task Detail (`useTaskDetailTaskSet`) and the main list
  (`useTaskListHierarchy`) both read it, so a rule added there lands on both
  surfaces at once.

### The four gaps

| Gap | Surface | What is wrong |
| --- | --- | --- |
| **GAP-90** | Add Subtask | An **archived Task** can be assigned as a brand-new parent, though the Parent picker exists specifically to prevent that from the other direction |
| **GAP-91** (picker) | Parent picker | `pickerCandidates` filters on `t.status` only, so an active Task in an **archived list** is still offered as a new parent |
| **GAP-91** (count facet) | Main list badge | `openSubtaskCounts` never sees `archivedLists`, so an open child in an archived list keeps inflating its parent's badge — disagreeing with the open counts beside it |
| **GAP-92** | Add Subtask | A new subtask silently inherits its parent's **archived** list, landing hidden from the Lists view with no disclosure |

`docs/Gaps.md` is explicit that these must be fixed together: "fixing any one
alone leaves the surfaces disagreeing in a different place rather than
agreeing."

## Goals & scope (this increment)

Two rules, each with exactly one authority.

### Rule A — write-side: an archived Task cannot become a NEW parent

Enforced in **core**, so every caller present and future agrees without
re-implementing the check.

`validate_parent_assignment` gains an archived-parent refusal, placed beside
`reject_ambiguous_parent` — the same shape, the same `all` slice, the same
find-by-path lookup. Because it is the shared phase-1 for both entry points,
one check covers `set_task_parent` (the Parent row's Change) and `add_subtask`
(Add Subtask), plus `update_task`'s combined-patch path which routes through
the same resolve.

It stays in **phase 1**, before every side effect — a refused assignment must
leave Task IDs off and nothing stamped, exactly as the self-parent and cycle
refusals already do.

**Inheritance is deliberately untouched.** The rule governs *assigning* a
parent, never *having* one:

- An existing on-disk `parent-id`/`parent` pair is never rewritten by an
  ordinary field edit (`update_task` only writes the pair when the patch
  carries `parentPath`/`clearParent`).
- `duplicate_task` copies bytes and keeps the pair without calling this path.
- Resolution and rendering are read-side (`parent_index`,
  `buildParentIndex`) and are not touched here.

So a child whose parent was archived *after* the relationship was set keeps
resolving and rendering its real parent — the behavior PR #77 deliberately
introduced, and which this increment must not regress. That non-regression gets
its own test.

**Frontend hint.** `TaskSubtasks.vue`'s Add-subtask input is disabled when the
open Task's own `status` is `archived`, with a stated reason, so the user meets
an affordance rather than an error toast. This follows the pattern
`useTaskDetailTaskSet` already documents for the picker's descendant
pre-disabling: *"A UI HINT ONLY … Core re-validates on write regardless and
remains the authority."* The hint is not the enforcement.

### Rule B — display-side: archived Tasks and archived-list Tasks are not offered, and are not counted

Enforced in the **frontend**, where `archivedMatcher` and the rest of the
list-archiving rule already live.

*A Task that is archived **or** filed in an archived list is neither offered as
an assignable new parent nor counted toward a parent's open-subtask badge.*

1. **`pickerCandidates`** (`useTaskDetailTaskSet`) applies
   `archivedMatcher(archivedLists)` alongside its existing `status` test — the
   same two-part test `useTaskDisplay` already applies. `TaskDetail.vue` already
   loads `archivedLists`, so it is threaded into the composable as a parameter
   rather than fetched a second time.
2. **`openSubtaskCounts`** (`taskHierarchy.ts`) excludes a child whose list is
   archived, alongside its existing `done` / `status === "archived"` test.
3. **Per-vault keying is mandatory.** The aggregate ("All tasks") view renders
   many vaults at once, and both ids and archived sets are vault-scoped. The
   archived data therefore travels as a **map keyed by vault id**, exactly as
   the parent index already is — never flattened into one set, which would let
   one vault's archived list name silently suppress another vault's identically
   named live list.
4. **Aggregate mode must actually load the configs.** Today `Tasks.vue`'s
   aggregate branch fans out `loadVaultLists(v.id)` but never
   `loadVaultConfig(v.id)`, so `vaultConfigs` is empty there and
   `useTaskLists`' `archivedLists` computed returns `[]` for the aggregate by
   construction. The config load joins the existing best-effort fan-out, with
   its own catch — a failed config read must not mark the vault's *tasks* as
   failed, matching how `loadVaultLists` already rides that loop.

### Rule C — GAP-92: keep inheritance, disclose the archived landing

Add Subtask continues to create the child with `list: props.task.list`,
preserving the existing and correct "a subtask inherits its parent's list"
design — the child stays beside its parent, and it is not lost in any case (it
renders in the parent's Subtasks section and under Plan/Tags grouping).

The defect is the silence, so the fix is disclosure: when the inherited list is
archived, the success path also notifies that the subtask landed in an archived
list hidden from the Lists view.

Routing the child elsewhere (the vault default, or No list) was considered and
rejected: it splits parent from child, breaking the inheritance design to solve
a visibility problem the user can already resolve by unarchiving.

## Non-goals

- **Core does not learn about archived lists.** Rule A covers archived
  *status* only. Extending core's parent path to read `archivedLists` was
  considered and rejected: it couples the write path to list-display config, and
  it would block Add Subtask on an active, visible Task that merely happens to
  sit in an archived list — harsher than the problem warrants. List-archiving
  stays a frontend display rule, which is where `archivedMatcher` lives.
- **No unarchive-on-assign, and no cascade.** Assigning a parent never changes
  any Task's status or any list's archived state.
- **No change to resolution or rendering of existing relationships.** An
  archived parent still resolves and still renders (PR #77's Fix 1).
- **The aggregate Lists grouping stays unfiltered.** `useTaskLists`' existing
  per-vault-only `archivedLists` computed keeps its current behavior for
  grouping; this increment adds the per-vault *map* for hierarchy consumers
  without redefining that separate, deliberate simplification.

## Testing

Frontend (Vitest — the whole of Rules B and C, plus Rule A's hint):

- `pickerCandidates` excludes an archived Task **and** an active Task in an
  archived list; still includes an active Task in a live list.
- `openSubtaskCounts` excludes an open child in an archived list; a
  same-named list archived in vault X does not suppress the count in vault Y
  (the per-vault keying regression).
- `Tasks.vue`'s aggregate branch loads each vault's config; a failing config
  read does not mark that vault's tasks as failed.
- The Add-subtask input is disabled on an archived Task and enabled otherwise.
- Add Subtask into an archived list raises the disclosure; into a live list
  raises none.

Core (Rust unit tests in the parent service):

- `set_task_parent` refuses an archived parent; `add_subtask` refuses one too
  (both entry points, since the value of a shared validator is that neither can
  drift).
- The refusal leaves Task IDs **off** and nothing stamped — the phase-ordering
  invariant this module exists to encode.
- **Non-regression:** a relationship set while the parent was active continues
  to resolve after the parent is archived.

Each regression test names its failure mode in a comment, per the repo's TDD
convention.

## Risks & mitigations

- **A new refusal in a write path.** Mitigated by scoping it to *assignment*
  only and by the explicit inheritance non-regression test above. The refusal
  message names the reason and the remedy (unarchive the task), matching the
  module's existing refusal copy.
- **Per-vault map threading touches the aggregate view.** Mitigated by keying
  the map exactly as the existing parent index is keyed, and by a test that
  cross-vault list names do not bleed.
- **An extra `get_tasks_config` per vault on the aggregate view.** Bounded by
  vault count, already the shape of the neighbouring `list_task_lists` fan-out,
  best-effort and non-blocking.

## Gaps closed

GAP-90, GAP-91 (all three facets), GAP-92 — removed from `docs/Gaps.md` on
landing, per the file's own convention.
