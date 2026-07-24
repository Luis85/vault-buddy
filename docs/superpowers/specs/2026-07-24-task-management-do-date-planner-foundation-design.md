# Task-management UX program — do-date foundation & cross-vault Plan-my-day — design

Date: 2026-07-24
Status: accepted (user request: "the next big ux/ui improvement … heavily
invest in task-management … align with popular tools … make it a joy … manage
tasks across many vaults without losing oversight … every vault treated like a
project … the user should not need to open Obsidian … plan its day … a
configurable ToDo List the user can pin to the desktop like a floating
sidepanel". Chosen path: **foundation first** — the do-date model + the
cross-vault planner — with the floating widget, fast capture, and rituals as
their own following increments.)

## Context

Task management has grown into a capable per-vault + cross-vault feature:
`type: Task` markdown notes with `due`/`priority`/`tags`/`order`/opt-in id,
Lists (as folders) with a full lifecycle, Lists/Dates/Tags grouping, a sort
selector, manual drag-to-reorder, an inline editor, and the aggregated
"All tasks" view (spec: `2026-07-11-task-lists-sorting-ordering-design.md` and
the aggregation/polish specs beside it). What it is **not yet** is a place you
*plan and run your day* from — the current "Dates" grouping buckets purely by
**deadline** (`taskSections.ts::dateBuckets` keys off `dueOf(t)`), which
conflates "when it's due" with "when I'll do it," the exact conflation that
every popular tool has moved away from.

Web research across Todoist, Things 3, TickTick, Microsoft To Do, Sunsama,
Akiflow, Motion and Linear (2026-07-24) produced one dominant finding: **the
single highest-leverage change that makes a task system feel calm is separating
a "do / scheduled date" from the "due / deadline."** Things 3 is built on it;
Todoist shipped it in 2025 after years of demand; the conflating apps produce
the "wall of overdue tasks → anxiety" failure mode users complain about most.
For a markdown model it is nearly free — the note already carries `due:`; add a
`scheduled:`, and Today / Upcoming / Anytime buckets fall out of it. That, plus
the multi-vault-as-projects oversight the user asked for, is this increment.

The design system (tokens + primitives) and the larger `comfortable`/`large`
panel are already in place (specs merged through 2026-07-23), so this is model +
views, not new visual vocabulary or window mechanics.

## The program (roadmap)

Four increments; this spec designs the **whole arc** briefly and **increment 1**
in full. Each later increment gets its own spec/plan when it is picked up.

| # | Increment | Core of it |
| --- | --- | --- |
| **1** | **Do-date foundation + Plan-my-day dashboard** *(this spec)* | A `scheduled` (do) date distinct from the `due` deadline; the date grouping becomes a do-date **Planner** (Overdue / Today / Upcoming / Anytime / Done); a cross-vault plan-my-day surface where each vault reads as a project; one-click **reschedule overdue → today**; quick-schedule (Today / Tomorrow / pick) on any row; deadlines become calm countdown chips that never move a task between buckets. |
| **2** | **Pinned Focus widget** | A new always-on-top mini window showing Today in your manual order — complete / reorder / reschedule inline — with a configurable source. Reuses the three-window discipline (positioned-while-hidden, own webview/root). Its own spec, informed by the floating-widget research. |
| **3** | **Fast capture & keyboard flow** | Natural-language quick-add (`ship draft tomorrow p1 #work`) with a live highlighted preview; a Ctrl-K command palette that lists actions *with* their shortcuts; single-key row verbs (`t`=today, `s`=schedule, `x`=complete…) with optimistic sub-100 ms feel; completion delight. |
| **4** | **Rituals & smart lists** | A guided *plan- / shutdown-my-day* flow that reflects a 1–2 sentence summary into the Obsidian **daily note** (the one thing a daily-note-native companion can do better than any standalone app); a "Won't Do" status; saved smart filters; optional time-estimates + a "realistic day" nudge. |

Dependencies: the widget (2) and the daily workflow both stand on the do-date
model from (1); (3) and (4) can interleave after (1). The `scheduled` field and
the planner buckets are designed so a **separate local Focus set** (pin
anything, even unscheduled, to the widget) can be layered into (2) with no
rework — for the foundation, "on my Focus / desktop" simply means "scheduled for
today," one durable concept.

## Increment 1 — goals & scope

### Goals

- **One new, Obsidian-native field:** `scheduled: YYYY-MM-DD` — "when I plan to
  do it," distinct from `due`. Read defensively, written never-clobber, absent
  by default. Fully backward-compatible: a task with no `scheduled` behaves
  exactly as today.
- **Do-date Planner buckets:** the date grouping keys off the **effective plan
  date** (`scheduled ?? due`) so Overdue / Today / Upcoming / Anytime reflect
  *when you'll work on a task*, while a task that only has a deadline keeps
  bucketing by that deadline exactly as it does today.
- **Cross-vault Plan-my-day:** the aggregate view is the hero planning surface —
  every vault's tasks in one Overdue/Today/Upcoming/Anytime plan, each row
  showing its vault (its "project"); a per-vault filter narrows to one project.
- **Plan-my-day verbs without Obsidian:** quick-schedule a task to Today /
  Tomorrow / pick-a-date / clear, from a row; **reschedule all overdue → today**
  as one section action; schedule on create (composer) and on edit (inline
  editor).
- **Calm deadlines:** `due` renders as a countdown chip on the row (red once
  passed) wherever the task sits; a passed deadline never yanks a task between
  planner buckets.

### Non-goals (this increment — each is a later increment)

- The floating Focus widget (increment 2).
- Natural-language quick-add, the command palette, single-key verbs
  (increment 3).
- The guided plan/shutdown ritual + daily-note reflection, "Won't Do", saved
  filters, recurring tasks, time estimates (increment 4).
- "This Evening" sub-bucket and a distinct "Someday" horizon — deliberately
  deferred; both want a second signal (a time-of-day, a someday marker) and the
  foundation is intentionally *one* new field. Called out here so a reviewer
  knows the omission is chosen, not missed.
- Any change to the task file format beyond adding `scheduled`; any change to
  status-toggle / list / tag / order behavior.

## Design

### 1. The `scheduled` field (data model)

- **Frontmatter key `scheduled`**, a plain `YYYY-MM-DD` string, emitted right
  after `due` in `render_task` (so the two dates read together). Chosen over an
  inline Obsidian-Tasks `⏳` emoji because this app's tasks are *frontmatter
  documents* — every structured field (`due`/`priority`/`tags`) already lives in
  frontmatter, and `scheduled:` is the Dataview / Obsidian-Tasks-frontmatter
  convention, so a Dataview query (`WHERE scheduled = date(today)`) and the
  Tasks plugin's frontmatter reading both work if the user ever opens Obsidian.
- **Read = defensive**, identical posture to `due`: valid only when it matches
  `^\d{4}-\d{2}-\d{2}$` (no calendar validity — `2026-02-31` is tolerated like
  Obsidian's own picker and like `is_valid_due`); anything else (a
  hand-authored `next week`) degrades to "unscheduled" rather than erroring.
  Core reads it via the existing lenient scalar reader; the frontend gets a
  `scheduledOf(t)` accessor beside `dueOf` in `taskFields.ts`.
- **Write = strict + never-clobber**, riding the existing surgical multi-key
  writer (`set_fields` / `update_task_fields`): `Some(date)` rewrites or inserts
  the `scheduled:` line, `None`/clear removes it, everything else byte-preserved.
  No new write machinery — `scheduled` is just another key the generalized
  writer already supports.
- **Reserved for templates:** `scheduled` joins the reserved task-key set that
  `render_task`'s extra-frontmatter template filter drops, so a per-vault
  template can never redefine the managed do-date (same guard `due`/`priority`
  already have).
- **DTO:** `TaskItem`/`TaskDto` gains `scheduled: string | null` (camelCase
  across Rust↔TS, the existing precedent); `add_task` and `update_task` accept
  an optional `scheduled` plus a `clearScheduled` flag mirroring `clearDue`.

### 2. Planner buckets (do-date aware)

The date grouping evolves from "bucket by `due`" to "bucket by the **effective
plan date** = `scheduled ?? due`":

- `plannerDateOf(t) = scheduledOf(t) ?? dueOf(t)`.
- Buckets: **Overdue** (effective date `< today`, open), **Today** (`== today`),
  **Upcoming** (`> today`), **Anytime** (no effective date, open), **Done**
  (unchanged). "No date" is relabeled **Anytime** (the do-able-backlog framing;
  the research's most-praised label for "nothing's blocking you from starting").
- **Why `scheduled ?? due` and not `scheduled` alone:** it is strictly
  non-regressing. A task that only has a deadline buckets by that deadline
  *exactly as it does today* (existing behavior preserved, existing bucket tests
  stay green); a task that gains a `scheduled` date buckets by *that* instead —
  so setting a do-date is what moves a task's plan, and a deadline alone still
  shows up on the day it's due. This is the Things model: the deadline informs
  the plan only until you make an explicit do-date decision.
- **Deadline never moves a scheduled task.** If `scheduled = tomorrow` and
  `due = yesterday`, the effective date is `scheduled` (tomorrow) → the task
  sits in **Upcoming** with a **red overdue-deadline chip**. The plan follows
  the do-date; the chip carries the urgency. This is the calm-deadline behavior
  users praise in Things and is the point of the split.
- The bucket engine stays a **pure, Linux-unit-tested** function
  (`plannerBuckets` beside `dateBuckets` in `taskSections.ts`; `dateBuckets` is
  replaced by it for this grouping, not kept in parallel); within-bucket order is
  the caller's existing global sort, untouched. Existing bucket-*placement* tests
  stay green (scheduled-less tasks land where they do today); the only assertion
  changes are the deliberate label renames below.
- The grouping control's "Dates" entry is **relabeled "Plan"** so the mode reads
  as "plan my day," not "sort by date," and its "No date" bucket becomes
  **"Anytime."** These are **display-label changes only** — the internal grouping
  key stays `dates` (`taskGrouping.ts`'s `Grouping` value is unchanged), so
  persisted `vault-buddy:task-grouping` prefs need **no migration** and every
  test keyed on the grouping value stays valid. Lists and Tags groupings are
  untouched.

### 3. Cross-vault Plan-my-day (each vault = a project)

- The **aggregate "All tasks" view is the hero**: it defaults to the **Plan**
  grouping, so opening it lands you on Overdue / Today / Upcoming / Anytime
  spanning every vault — "plan my day across projects" in one screen, no
  Obsidian. (Per-vault mode offers the same Plan grouping; only the default
  landing grouping differs — aggregate → Plan, per-vault keeps its remembered
  choice.)
- Each aggregate row keeps its **vault-attribution chip** (already shipped) — the
  vault *is* the project, so oversight is "which project is this task in" at a
  glance. The existing per-vault filter (and future saved filters) narrow to one
  project.
- No new IPC for aggregation — it stays the frontend fan-out over `list_vaults`
  + per-vault `list_tasks` the aggregate already uses; `scheduled` simply rides
  the `TaskItem` payload.

### 4. Plan-my-day verbs

- **Quick-schedule on a row:** a small schedule affordance (calendar glyph) on
  each `TaskRow` opens Today / Tomorrow / This weekend / Pick a date… / Clear.
  Picking writes `scheduled` optimistically through `update_task` (resolving the
  row's own `task.vaultId` in aggregate mode, exactly like toggle/archive), then
  re-sorts and re-buckets in place; failure reverts and toasts. This is the
  primary "plan" gesture and the thing that makes the whole feature usable
  without a full editor open.
- **Reschedule all overdue → Today:** a section action on the Overdue header
  that stamps `scheduled = today` on every task in the bucket — the single
  biggest anxiety-reliever the research found (Todoist's overdue-rebalance).
  Implemented as a serialized batch of `update_task` writes with **batch
  revert on failure** (the exact pattern `useTaskReorderCommit` already uses for
  materializing ranks), optimistic in the UI, best-effort per task with a
  summary toast naming any that failed. Per-vault it targets that vault; in the
  aggregate it spans vaults, each write against its row's vault.
- **Schedule on create:** the composer's options row gains a "When / Do date"
  control (Today / Tomorrow / pick / none) beside the existing
  due/priority/tags/list, threaded into `add_task`.
- **Schedule on edit:** the inline `TaskEditor` gains a "Do date" field beside
  "Due," sent in the same changed-fields patch (`clearScheduled` for an emptied
  value), through the one `update_task` call.

### 5. Row presentation (calm, scannable)

- The row shows, in priority order: title, tag chips, then a **do-date chip**
  ("Today", "Tomorrow", "Sat", a date) and a **deadline chip** (a countdown /
  "Due <date>", red once passed). When do-date and deadline coincide, collapse
  to one chip to avoid noise. Chips reuse the `Chip` primitive + status tokens
  (`danger` for a passed deadline). This is display-only over the new fields —
  no logic beyond formatting.
- Empty/degraded states reuse `EmptyState`; the planner's "nothing scheduled"
  Today bucket gets a friendly empty hint ("Nothing planned for today — pull
  something from Anytime or Upcoming") rather than a blank, nudging the plan-my-
  day loop.

## Architecture

- **`core` (Linux-testable):** `parse` gains the `scheduled` scalar read (reuse
  the `due` validator, generalized to `is_valid_date` or an `is_valid_scheduled`
  alias); the task doc struct + `render_task` emit `scheduled`; the template
  reserved-key set includes it; `create_task`/`update_task_fields` thread it. No
  Tauri types — all of it unit-tested on Linux, mirroring how `due`/`priority`
  landed.
- **Shell (`src-tauri/src/task_commands.rs`):** `add_task` and `update_task`
  gain the optional `scheduled` + `clearScheduled` in their argument structs and
  pass them through; `list_tasks` returns the field via the DTO. No new
  commands, so the IPC-surface count is unchanged (AGENTS.md table needs only the
  field-level note). Compile-gated on Linux (`npx tauri build --no-bundle`).
- **Frontend:** `taskFields.ts` (`scheduledOf`), `taskSections.ts`
  (`plannerBuckets`), `useTaskDisplay.ts` (Plan grouping wired to the new
  bucket + aggregate default), `TaskRow.vue` (do/deadline chips + schedule
  menu), `TaskComposer.vue` / `TaskEditor.vue` (do-date field), `Tasks.vue`
  (quick-schedule + reschedule-overdue wiring, reusing the batch/optimistic
  patterns already present), `TaskViewControls.vue` (the "Dates"→"Plan" label).
  The quick-schedule + reschedule batch logic goes in a small composable
  (`useTaskSchedule`) so `Tasks.vue` stays under its LOC cap — the same
  split discipline as `useTaskActions`/`useTaskReorderCommit`.

## Domain language

This increment adds terms to CONTEXT.md's ubiquitous language (via the
`domain-modeling` skill): **Do Date / Scheduled Date** (when the user plans to
work a Task, distinct from its **Due Date / Deadline**), **Planner** (the do-date
grouping), **Today / Upcoming / Anytime** (planner buckets), and **Focus** (the
set scheduled for today that will feed the widget). Code, UI copy, and commits
use these terms.

## Error handling

No new error surface. Every write is an existing `update_task`/`add_task` path
(atomic, containment-gated, never-clobber); a failed quick-schedule or batch
reschedule reverts the optimistic UI and toasts, best-effort per task (the batch
names failures in one toast, like the delete-list relocation). A malformed
`scheduled` value is read as unscheduled, never an error. Window/placement code
is untouched.

## Testing

- **Rust (`core`):** `scheduled` parse (valid / invalid-degrades-to-none /
  quoted / commented), `render_task` emits it in the right position and omits it
  when absent, the template filter drops a `scheduled` key, the field-write
  round-trips (set / rewrite / clear) byte-preserving the rest — the exact
  battery `due` already has, extended.
- **Frontend (Vitest):** `plannerBuckets` (effective `scheduled ?? due`;
  overdue/today/upcoming/anytime/done placement; a scheduled-future +
  overdue-deadline task lands in Upcoming with the red chip); quick-schedule
  writes `scheduled` and re-buckets; reschedule-overdue batch stamps today +
  reverts on a failed write; composer/editor send `scheduled`/`clearScheduled`;
  the aggregate defaults to Plan grouping. **Behavior is preserved for
  scheduled-less tasks** — one with no `scheduled` lands in the same bucket and
  renders the same as today (the `scheduled ?? due` fallback). The only
  deliberate assertion changes are the display-label renames (No date → Anytime,
  the Dates → Plan control label); beyond those, a test that must change a
  bucket-*placement* assertion signals a behavior slip, not a layout change.
- **Windows** remains where the end-to-end write + Obsidian round-trip are
  eyeballed — called out for the reviewer, not gating.

## Quality gates & docs

- `npm run lint && npm run check:loc && npm run check:quality &&
  npm run test:coverage`; `cargo fmt` / clippy `-D warnings` / tests for `core`
  and the shell compile-gate. Frontend coverage floors and the LOC/quality
  baselines shrink-only as usual.
- Docs updated in the same PR: AGENTS.md (the tasks-domain section — the new
  `scheduled` field, the Planner grouping, quick-schedule/reschedule writes; the
  IPC table's `add_task`/`update_task`/`list_tasks` field notes), CONTEXT.md
  (the new domain terms), the task-management PRD + the aggregated-dashboard use
  case (do-date + planner shipped), and docs/Gaps.md if any new gap surfaces
  (e.g. the deferred This-Evening/Someday, timezone-at-midnight bucketing).

## Rollout / compatibility

Additive and migration-free: `scheduled` is a new optional field, and the
grouping key stays `dates` (label-only rename), so no config or localStorage
migration is needed. A vault with no scheduled tasks sees the same bucket
*placement* it sees today (the `scheduled ?? due` fallback guarantees it); the
only visible differences are the friendlier labels (Anytime, Plan) and the new,
dormant affordances (schedule menu, reschedule-overdue, do-date chip) that have
nothing to act on until the user starts scheduling. Existing per-vault and
aggregate behavior is otherwise preserved; the one default change is the
aggregate view opening on the (do-date-aware) Plan grouping.

## Suggested phasing for the plan

1. `core`: `scheduled` parse + validate + render + template-reserve + field
   write + tests.
2. Shell: `add_task`/`update_task`/`list_tasks` carry `scheduled` /
   `clearScheduled`; compile-gate.
3. Frontend model: `scheduledOf`, `plannerBuckets`, Plan grouping + aggregate
   default; unit tests (existing suite green).
4. Frontend verbs: quick-schedule menu, reschedule-overdue batch
   (`useTaskSchedule`), composer + editor do-date fields.
5. Row presentation: do-date + deadline chips, the Today empty-state nudge.
6. Docs + baselines (AGENTS.md, CONTEXT.md, PRD, use case, Gaps).

Phases 1–3 (the model) and 4–5 (the verbs + UI) are independently reviewable — a
natural PR split if the branch merges early.
