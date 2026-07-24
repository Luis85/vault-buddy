# Do-date foundation & cross-vault Plan-my-day — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `scheduled` (do) date distinct from the `due` deadline, turn the date grouping into a do-date "Planner" (Overdue / Today / Upcoming / Anytime / Done) that spans vaults, and add plan-my-day verbs (quick-schedule a row, reschedule all overdue → today, schedule on create/edit) — all additive and non-regressing.

**Architecture:** `scheduled` rides the exact rails `due` already uses: a lenient frontmatter scalar read in `core::tasks`, the surgical never-clobber `set_fields` writer, the `TaskItem`/`TaskDto` DTO, and the `add_task`/`update_task` IPC. The date grouping's bucket engine is replaced by `plannerBuckets`, keyed off the *effective plan date* = `scheduled ?? due`, so a deadline-only task buckets exactly as today. New frontend verbs (`useTaskSchedule`, `TaskScheduleMenu`) reuse the optimistic-with-revert + shared busy-guard pattern of `useTaskActions`/`useTaskReorderCommit`.

**Tech Stack:** Rust (Tauri v2 shell + pure `vault_buddy_core` crate), Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + `mockIPC`), `cargo test`.

## Global Constraints

- **`scheduled` is a plain `YYYY-MM-DD` frontmatter scalar**, emitted **after `due`** in `render_task`; read leniently (reuse `is_valid_due`'s shape check — unparseable → treated as unscheduled).
- **`scheduled` MUST be reserved in BOTH reserved-key constants** — `disk.rs::RESERVED_TASK_KEYS` (template filter) AND `id.rs::RESERVED_TASK_KEYS` (task-ID-property validator). These are two separate `const` arrays today; add it to both.
- **New Rust params are APPENDED** to `render_task`/`create_task`/`services::add_task` (last position), never inserted mid-list — this minimizes call-site churn and matches how `task_id`/templates were added.
- **DTO fields are camelCase across Rust↔TS** (`#[serde(rename_all = "camelCase")]` already on the structs); TS `scheduled: string | null`.
- **The grouping localStorage key stays `dates`** — only its display *label* becomes "Plan" and the "No date" bucket label becomes "Anytime". No pref migration.
- **Aggregate default grouping is Plan only when unset**: `loadGrouping("all", "dates")` — a persisted `"all"` value still wins. Per-vault default stays `"lists"`.
- **Content rendering is preserved for scheduled-less tasks**: a task with no `scheduled` buckets exactly as today and its EXISTING content (title / tags / `due` / priority / vault chip) renders unchanged — the do-date chip appears only when `scheduled` is present, and the existing `due` element is untouched. NOT literally byte-identical: Task 6 adds one always-present affordance to every row (the schedule trigger), which is how you schedule an unscheduled task — the invariant is that existing *content* + the `due` presentation don't change, not that the row's DOM is unchanged.
- **Vault writes never clobber** (the whole increment rides `set_fields` / `update_task_fields`) — with the ONE documented exception in Task 8's Gaps entry (d): a vault that hand-set its id property to the literal `scheduled` can have that on-disk id overwritten by a schedule write (accepted edge, remedy = re-point the id property first).
- **Commits:** Conventional Commits (`feat(core)`, `feat(ui)`, `fix(shell)`, `docs`, `test`). Imperative subject; body explains the *why*. Do NOT put any model identifier in commits/PRs/code.
- **CI gates that must stay green:** `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test` for core + shell; `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`.

---

### Task 1: Core — `scheduled` read, render, and reserve (pure `tasks` module)

**Files:**
- Modify: `src-tauri/core/src/tasks/list.rs` (add `TaskItem.scheduled`; read it)
- Modify: `src-tauri/core/src/tasks/disk.rs` (`render_task`/`create_task` scheduled param; emit; reserve; fix test call sites)
- Modify: `src-tauri/core/src/tasks/id.rs` (reserve `scheduled`; tests)
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (`TaskDto.scheduled`; `from_item`; pass `None` to `create_task` for now)

**Interfaces:**
- Produces: `TaskItem.scheduled: Option<String>`; `render_task(title, created, due, priority, tags, task_id, extra_frontmatter, body_template, scheduled: Option<&str>) -> String`; `create_task(root, title, today, due, priority, tags, task_id, extra_frontmatter, body_template, scheduled: Option<&str>) -> io::Result<PathBuf>`; `TaskDto.scheduled: Option<String>`.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing read test** in `src-tauri/core/src/tasks/list.rs` (inside `mod tests`, using the existing `write` helper):

```rust
    #[test]
    fn list_tasks_reads_scheduled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "t.md",
            "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\nscheduled: 2026-07-20\n---\n",
        );
        write(
            root,
            "u.md",
            "---\ntype: Task\nstatus: new\ntitle: \"U\"\ncreated: 2026-07-08\n---\n",
        );
        // A malformed value must degrade to None IN CORE (not just the
        // frontend) so TaskDto/MCP never expose it (Codex, PR #75).
        write(
            root,
            "m.md",
            "---\ntype: Task\nstatus: new\ntitle: \"M\"\ncreated: 2026-07-08\nscheduled: next week\n---\n",
        );
        let items = list_tasks(root, None);
        let sched = |title: &str| items.iter().find(|t| t.title == title).unwrap().scheduled.clone();
        assert_eq!(sched("T"), Some("2026-07-20".to_string()));
        assert_eq!(sched("U"), None); // absent → None
        assert_eq!(sched("M"), None); // malformed → None (filtered in core)
    }
```

- [ ] **Step 2: Run it — expect a compile error** (`scheduled` field missing):

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::list::tests::list_tasks_reads_scheduled`
Expected: FAIL — `no field 'scheduled' on type 'TaskItem'`.

- [ ] **Step 3: Add the field and read it.** In `list.rs`, in the `TaskItem` struct, insert after the `due` field:

```rust
    pub due: Option<String>,
    /// The do/plan date (`YYYY-MM-DD`) — when the user plans to WORK the task,
    /// distinct from `due` (the deadline). Read then validator-filtered, so it is
    /// `None` when absent OR unparseable (an honest DTO/MCP boundary).
    pub scheduled: Option<String>,
    pub priority: Option<String>,
```

In `collect_task_file`, after `let due = scalar_field(&content, "due");` add (`is_valid_due` is already imported in `list.rs` for `due_key`):

```rust
    // Filter through the date validator so a malformed value (e.g. "next week")
    // becomes None at the DTO/MCP boundary, honoring the "invalid → None"
    // contract in CORE — not only in the frontend's scheduledOf (Codex, PR #75).
    let scheduled = scalar_field(&content, "scheduled").filter(|s| is_valid_due(s));
```

In the `out.push(TaskItem { ... })` initializer, add `scheduled,` immediately after `due,`.

- [ ] **Step 4: Run the read test — expect PASS:**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::list::tests::list_tasks_reads_scheduled`
Expected: PASS.

- [ ] **Step 5: Write the failing render test** in `src-tauri/core/src/tasks/disk.rs` (`mod tests`):

```rust
    #[test]
    fn render_includes_scheduled_after_due_only_when_present() {
        // Absent → byte-identical to the pre-scheduled output (no scheduled line).
        let plain = render_task("A", "2026-07-09", Some("2026-07-15"), Some("high"), &[], None, None, None, None);
        assert!(plain.contains("due: 2026-07-15\npriority: high\n"));
        assert!(!plain.contains("scheduled"));
        // Present → emitted right after due, before priority.
        let sched = render_task("A", "2026-07-09", Some("2026-07-15"), Some("high"), &[], None, None, None, Some("2026-07-20"));
        assert!(sched.contains("due: 2026-07-15\nscheduled: 2026-07-20\npriority: high\n"));
        // Scheduled with no due lands right after created.
        let no_due = render_task("A", "2026-07-09", None, None, &[], None, None, None, Some("2026-07-20"));
        assert!(no_due.contains("created: 2026-07-09\nscheduled: 2026-07-20\n---\n"));
    }
```

- [ ] **Step 6: Run it — expect a compile error** (arity mismatch): 

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::disk::tests::render_includes_scheduled_after_due_only_when_present`
Expected: FAIL — `render_task` takes 8 arguments but 9 were supplied (every other call site errors too).

- [ ] **Step 7: Add the param, emit it, reserve it.** In `disk.rs`:

Add `"scheduled"` to `RESERVED_TASK_KEYS` (after `"due"`):

```rust
const RESERVED_TASK_KEYS: &[&str] = &[
    "type", "status", "title", "created", "due", "scheduled", "priority", "tags", "tag", "order",
];
```

Append `scheduled: Option<&str>` as the **last** parameter of `render_task` (keep `#[allow(clippy::too_many_arguments)]`), update its doc comment noting "`scheduled` (the last param) is emitted after `due`", and inside the body insert a scheduled emit block **between** the `due` block and the `priority` block:

```rust
    if let Some(d) = due {
        extra.push_str(&format!("due: {d}\n"));
    }
    if let Some(s) = scheduled {
        extra.push_str(&format!("scheduled: {s}\n"));
    }
    if let Some(p) = priority {
        extra.push_str(&format!("priority: {p}\n"));
    }
```

Do **NOT** add `scheduled` to the `vars` array — leave it
`[("title", …), ("date", …), ("due", …), ("priority", …)]` unchanged. Exposing a
new `{{scheduled}}` template placeholder would oblige us to update
`TaskTemplateSettings.vue`'s user-visible `TEMPLATE_PLACEHOLDER_HINT` and the
template-placeholder contract in AGENTS.md, which is out of this increment's
scope (Codex, PR #75). `scheduled` only needs to be RESERVED (done above), not a
substitutable variable; an author who types `{{scheduled}}` gets the existing
unknown-token behavior (renders empty), same as any other unsupported token.

Append `scheduled: Option<&str>` as the **last** parameter of `create_task`, and pass it through to the `render_task` call as the last argument.

- [ ] **Step 8: Fix every existing call site in `disk.rs` tests.** In `disk.rs`'s `mod tests`, every `render_task(...)` and `create_task(...)` call (the ones NOT added in this task) needs `None` appended as its final argument. There are ~10 `render_task` and ~8 `create_task` calls. Example — this existing call:

```rust
        let doc = render_task("Buy milk", "2026-07-08", None, None, &[], None, None, None);
```

becomes:

```rust
        let doc = render_task("Buy milk", "2026-07-08", None, None, &[], None, None, None, None);
```

Do the same (`, None`) for every `create_task(&root, "…", "…", None, None, &[], None, None, None)` → `…, None, None)`.

- [ ] **Step 9: Reserve `scheduled` in the task-ID validator (`id.rs`) — write the failing assertions first.** In `src-tauri/core/src/tasks/id.rs`, extend the two existing tests: in `is_valid_id_property_charset_and_reserved` add `"scheduled"` to the reserved loop array, and in `id_property_for_generation_gates_on_enabled_and_validity` add:

```rust
        assert_eq!(id_property_for_generation(true, "scheduled"), None); // reserved (do-date)
```

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::id`
Expected: FAIL — `is_valid_id_property("scheduled")` currently returns `true`.

- [ ] **Step 10: Add `scheduled` to `id.rs::RESERVED_TASK_KEYS`** (a separate constant from disk.rs's — add a `// keep in sync with disk.rs::RESERVED_TASK_KEYS` comment on both):

```rust
const RESERVED_TASK_KEYS: &[&str] = &[
    "type", "status", "title", "created", "due", "scheduled", "priority", "tags", "tag", "order",
];
```

Run: `cd src-tauri/core && cargo test -p vault_buddy_core tasks::id`
Expected: PASS.

- [ ] **Step 11: Thread `scheduled` through the services DTO.** In `src-tauri/core/src/services/tasks/mod.rs`:

In `TaskDto`, add after `due`:

```rust
    pub due: Option<String>,
    /// The do/plan date, distinct from `due`. `None` when unset. Additive for
    /// the frontend and MCP `list_tasks` alike.
    pub scheduled: Option<String>,
    pub priority: Option<String>,
```

In `TaskDto::from_item`, add `scheduled: t.scheduled,` after `due: t.due,`.

In `add_task`, the `create_task(...)` call gains a **trailing `None`** argument (add_task can't set a scheduled date until Task 2 — this is the transitional passthrough), and the final `Ok(TaskDto { ... })` construction gets `scheduled: None,` after `due: due.map(str::to_string),`, with a comment `// wired in Task 2 (schedule-on-create)`.

- [ ] **Step 12: Run the whole core test suite — expect PASS:**

Run: `cd src-tauri/core && cargo test -p vault_buddy_core && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS, no clippy warnings, formatted.

- [ ] **Step 13: Commit:**

```bash
git add src-tauri/core/src/tasks/list.rs src-tauri/core/src/tasks/disk.rs src-tauri/core/src/tasks/id.rs src-tauri/core/src/services/tasks/mod.rs
git commit -m "feat(core): read/render a scheduled (do) date on tasks

Adds the scheduled frontmatter field alongside due — read leniently, emitted
after due, reserved in BOTH the template and task-ID-property reserved-key
sets. TaskItem/TaskDto carry it; add_task passes None until schedule-on-create."
```

---

### Task 2: Shell + services + MCP — `scheduled` on `add_task` & `update_task` IPC

**Files:**
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (`add_task` gains `scheduled` param)
- Modify: `src-tauri/core/src/services/tasks/tests.rs` and `src-tauri/core/src/services/tasks/lists.rs` (append `None` to every `add_task(...)` call)
- Modify: `src-tauri/mcp/src/service.rs` (its `services::add_task(...)` call passes `None`)
- Modify: `src-tauri/src/task_commands.rs` (`validated_scheduled`; `add_task` cmd; `TaskPatchDto`; `update_task`)

**Interfaces:**
- Consumes: `create_task`/`TaskDto` from Task 1.
- Produces: `services::add_task(paths, id, title, today, due, priority, tags, list, scheduled: Option<&str>)`; IPC `add_task` gains `scheduled?`; IPC `update_task` patch gains `scheduled?` / `clearScheduled?`.

- [ ] **Step 1: Write the failing shell unit test** for `validated_scheduled` in `src-tauri/src/task_commands.rs` (`mod tests`):

```rust
    #[test]
    fn validated_scheduled_accepts_dates_and_rejects_junk() {
        assert_eq!(validated_scheduled(None).unwrap(), None);
        assert_eq!(
            validated_scheduled(Some("2026-07-20".to_string())).unwrap(),
            Some("2026-07-20".to_string())
        );
        assert!(validated_scheduled(Some("next week".to_string())).is_err());
    }
```

- [ ] **Step 2: Run it — expect a compile error** (`validated_scheduled` undefined):

Run: `cd src-tauri && cargo test -p vault-buddy --lib task_commands::tests::validated_scheduled_accepts_dates_and_rejects_junk`
Expected: FAIL — cannot find function `validated_scheduled`. (Requires the shell to build; if GUI deps aren't installed run `npm ci && npm run build && npm run setup:linux` once, per AGENTS.md.)

- [ ] **Step 3: Add `validated_scheduled`** in `task_commands.rs`, right after `validated_due`:

```rust
/// Validate an optional do/plan date for a write (same shape as `due`).
/// Ok(None) when absent.
fn validated_scheduled(scheduled: Option<String>) -> Result<Option<String>, String> {
    match scheduled {
        Some(d) if !tasks::is_valid_due(&d) => {
            Err(format!("Do date must be YYYY-MM-DD, got: {d}"))
        }
        other => Ok(other),
    }
}
```

- [ ] **Step 4: Run the test — expect PASS:**

Run: `cd src-tauri && cargo test -p vault-buddy --lib task_commands::tests::validated_scheduled_accepts_dates_and_rejects_junk`
Expected: PASS.

- [ ] **Step 5: Add the `scheduled` param to `services::add_task`.** In `src-tauri/core/src/services/tasks/mod.rs`, append `scheduled: Option<&str>` as the **last** parameter of `add_task`; pass it as the trailing argument to the `tasks::create_task(...)` call (replacing the `None` placeholder from Task 1); and set the returned `TaskDto`'s field to `scheduled: scheduled.map(str::to_string),` (replacing the `scheduled: None` placeholder). Update the doc comment to mention `scheduled` beside `due`.

- [ ] **Step 6: Fix every `add_task(...)` call site in the core service tests.** In `src-tauri/core/src/services/tasks/tests.rs` (~25 calls) and `src-tauri/core/src/services/tasks/lists.rs` (~5 calls), append `None` as the final argument to each `add_task(...)` invocation. Example:

```rust
    let created = add_task(&paths, VAULT, "Buy milk", "2026-07-09", None, None, &[], None).unwrap();
```

becomes:

```rust
    let created = add_task(&paths, VAULT, "Buy milk", "2026-07-09", None, None, &[], None, None).unwrap();
```

(Every one of these calls already passes `None` for `list` as its last arg; add one more `None` for `scheduled`.)

- [ ] **Step 7: Fix the MCP call site.** In `src-tauri/mcp/src/service.rs`, the `services::add_task(...)` call (~line 394) gets a trailing `None` argument (the MCP `add_task` tool does not expose scheduling this increment; add an inline comment saying so).

- [ ] **Step 8: Wire the shell `add_task` command.** In `task_commands.rs`, in the `#[tauri::command] pub async fn add_task`, add `scheduled: Option<String>` to the parameter list (after `list`), validate it inline before the thread hop (`let scheduled = validated_scheduled(scheduled)?;` beside the existing `let due = validated_due(due)?;`), and pass `scheduled.as_deref()` as the final argument to the `services::add_task(...)` call.

- [ ] **Step 9: Wire the shell `update_task` patch.** In `task_commands.rs`, in `TaskPatchDto`, add after the `due`/`clear_due` fields:

```rust
    #[serde(default)]
    pub scheduled: Option<String>,
    #[serde(default)]
    pub clear_scheduled: bool,
```

In `update_task`, after the `due` handling block, add the scheduled handling (mirrors `due`):

```rust
    if patch.clear_scheduled {
        updates.push(("scheduled", None));
    } else if patch.scheduled.is_some() {
        updates.push(("scheduled", validated_scheduled(patch.scheduled.clone())?));
    }
```

- [ ] **Step 10: Build the shell + run its tests + fmt/clippy — expect PASS:**

Run:
```bash
cd src-tauri && cargo test -p vault-buddy --lib && cargo test -p vault_buddy_core -p vault_buddy_mcp && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check
```
Expected: PASS. (If the shell won't build for lack of GUI deps, run `npm run setup:linux` and ensure `../dist` exists via `npm run build`, then `npx tauri build --no-bundle` as the compile gate.)

- [ ] **Step 11: Commit:**

```bash
git add src-tauri/core/src/services/tasks/ src-tauri/mcp/src/service.rs src-tauri/src/task_commands.rs
git commit -m "feat(shell): carry scheduled on add_task and update_task

services::add_task + the add_task command accept a do-date; update_task's
patch gains scheduled/clearScheduled, validated like due. MCP add_task passes
None (no scheduling exposed there this increment)."
```

---

### Task 3: Frontend — `scheduled` types + `scheduledOf`/`plannerDateOf`

**Files:**
- Modify: `src/types.ts` (`TaskItem.scheduled`; `TaskPatch.scheduled`/`clearScheduled`)
- Modify: `src/utils/taskFields.ts` (`scheduledOf`, `plannerDateOf`)
- Modify: `tests/helpers/taskMount.ts` (default `scheduled: null` in the shared task factory)
- Create: `tests/task-fields.test.ts`

**Interfaces:**
- Produces: TS `TaskItem.scheduled: string | null`; `TaskPatch.scheduled?: string`, `TaskPatch.clearScheduled?: boolean`; `scheduledOf(t): string | null`; `plannerDateOf(t): string | null`.

- [ ] **Step 1: Write the failing test** — create `tests/task-fields.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import type { TaskItem } from "../src/types";
import { plannerDateOf, scheduledOf } from "../src/utils/taskFields";

function task(p: Partial<TaskItem>): TaskItem {
  return {
    path: "p", title: "t", status: "new", created: "2026-07-01", done: false,
    due: null, scheduled: null, priority: null, tags: [], list: "", order: null, id: null, ...p,
  };
}

describe("scheduledOf", () => {
  it("returns a plain YYYY-MM-DD, null otherwise", () => {
    expect(scheduledOf(task({ scheduled: "2026-07-20" }))).toBe("2026-07-20");
    expect(scheduledOf(task({ scheduled: "next week" }))).toBeNull();
    expect(scheduledOf(task({ scheduled: null }))).toBeNull();
  });
});

describe("plannerDateOf", () => {
  it("prefers scheduled, falls back to due", () => {
    expect(plannerDateOf(task({ scheduled: "2026-07-20", due: "2026-07-10" }))).toBe("2026-07-20");
    expect(plannerDateOf(task({ scheduled: null, due: "2026-07-10" }))).toBe("2026-07-10");
    expect(plannerDateOf(task({ scheduled: null, due: null }))).toBeNull();
  });
});
```

- [ ] **Step 2: Run it — expect FAIL** (imports/type don't exist yet):

Run: `npx vitest run tests/task-fields.test.ts`
Expected: FAIL — `scheduledOf`/`plannerDateOf` not exported; `TaskItem` has no `scheduled`.

- [ ] **Step 3: Add the type fields.** In `src/types.ts`, in `TaskItem`, after `due: string | null;`:

```ts
  due: string | null;
  /** The do/plan date (YYYY-MM-DD) — when the user plans to work the task,
   * distinct from `due`. null when unset. */
  scheduled: string | null;
  priority: string | null;
```

In `TaskPatch`, after `clearDue?: boolean;`:

```ts
  clearDue?: boolean;
  /** Set the do/plan date. */
  scheduled?: string;
  /** Clear the do/plan date (mirrors clearDue). */
  clearScheduled?: boolean;
  priority?: string;
```

- [ ] **Step 4: Add the accessors.** In `src/utils/taskFields.ts`, after `dueOf`:

```ts
// A scheduled (do) date counts only when it's a plain YYYY-MM-DD (defensive
// read, same shape gate as dueOf).
export const scheduledOf = (t: TaskItem): string | null =>
  t.scheduled && DUE_RE.test(t.scheduled) ? t.scheduled : null;

// The effective PLAN date the planner buckets by: the do-date if set, else the
// deadline. Setting a scheduled date is what moves a task's plan; a
// deadline-only task still buckets by its deadline (non-regressing).
export const plannerDateOf = (t: TaskItem): string | null => scheduledOf(t) ?? dueOf(t);
```

- [ ] **Step 5: Default the field in EVERY typed fixture the required field breaks.** `scheduled: string | null` is REQUIRED (matching `due`), so every raw `TaskItem`/`AggTask` object literal in the test suite must gain `scheduled: null` or `vue-tsc` fails and the branch won't typecheck. First the shared factory: in `tests/helpers/taskMount.ts`, add `scheduled: null,` beside the existing `due:` default. Then find the raw-literal fixtures — grep is the reliable way:

```bash
grep -rln "due:" tests/ | xargs grep -l "priority:" 
```

Known offenders that build typed task literals (verify against the grep + the build output, don't assume this list is exhaustive): `tests/task-sort.test.ts`, `tests/task-order.test.ts`, `tests/task-reorder.test.ts`. Add `scheduled: null` to each raw literal in every file `npm run build` flags.

- [ ] **Step 6: Run the new test + typecheck — expect PASS:**

Run: `npx vitest run tests/task-fields.test.ts && npm run build`
Expected: PASS; `vue-tsc` typecheck clean (a remaining type error names another fixture file — add `scheduled: null` there and re-run).

- [ ] **Step 7: Commit — stage EVERY file you touched, including all flagged fixtures:**

```bash
git add src/types.ts src/utils/taskFields.ts tests/task-fields.test.ts tests/helpers/taskMount.ts
# plus every fixture the required field forced you to update, e.g.:
git add tests/task-sort.test.ts tests/task-order.test.ts tests/task-reorder.test.ts
# then confirm nothing is left behind:
git status --porcelain   # must be empty for the tracked tests/src you changed
git commit -m "feat(ui): scheduled type + scheduledOf/plannerDateOf helpers"
```

---

### Task 4: Frontend — planner buckets, Plan grouping, aggregate default

**Files:**
- Modify: `src/utils/taskSections.ts` (replace `dateBuckets` with `plannerBuckets`)
- Modify: `src/composables/useTaskDisplay.ts` (use `plannerBuckets`; aggregate default grouping)
- Modify: `src/utils/perViewStore.ts` (`load` gains an optional default override)
- Modify: `src/utils/taskGrouping.ts` (`loadGrouping` gains an optional default override)
- Modify: `src/components/TaskViewControls.vue` (label "Dates" → "Plan")
- Modify: `tests/task-sections.test.ts` (dateBuckets describe → plannerBuckets)
- Modify: `tests/task-grouping.test.ts` (aggregate default)

**Interfaces:**
- Consumes: `plannerDateOf` (Task 3).
- Produces: `plannerBuckets(tasks, today): Bucket[]`; `loadGrouping(viewKey, defaultOverride?)`.

- [ ] **Step 1: Write the failing bucket tests.** In `tests/task-sections.test.ts`, replace the existing `dateBuckets` describe block with a `plannerBuckets` one (import `plannerBuckets` from `../src/utils/taskSections`; reuse the file's existing task factory, adding `scheduled` where needed):

```ts
describe("plannerBuckets", () => {
  const today = "2026-07-10";
  it("buckets by effective plan date = scheduled ?? due", () => {
    const rows = [
      task({ title: "OverdueByDue", due: "2026-07-01" }),
      task({ title: "TodayByScheduled", scheduled: "2026-07-10", due: "2026-08-01" }),
      task({ title: "UpcomingByScheduled", scheduled: "2026-07-20" }),
      task({ title: "Anytime" }),
      task({ title: "Done", done: true }),
    ];
    const b = plannerBuckets(rows, today);
    const byKey = (k: string) => b.find((x) => x.key === k)?.tasks.map((t) => t.title) ?? [];
    expect(byKey("overdue")).toEqual(["OverdueByDue"]);
    expect(byKey("today")).toEqual(["TodayByScheduled"]);
    expect(byKey("upcoming")).toEqual(["UpcomingByScheduled"]);
    expect(byKey("anytime")).toEqual(["Anytime"]);
    expect(byKey("done")).toEqual(["Done"]);
  });
  it("a future scheduled date beats an overdue deadline (Things model)", () => {
    // scheduled = tomorrow, due = yesterday → Upcoming (plan follows the do-date).
    const rows = [task({ title: "T", scheduled: "2026-07-20", due: "2026-07-01" })];
    const b = plannerBuckets(rows, today);
    expect(b.find((x) => x.key === "upcoming")?.tasks.map((t) => t.title)).toEqual(["T"]);
    expect(b.find((x) => x.key === "overdue")).toBeUndefined();
  });
  it("keeps a flat, header-less list when no dated open task exists", () => {
    const b = plannerBuckets([task({ title: "A" }), task({ title: "B", done: true })], today);
    // Anytime + Done present, but labels null (no headers) — the existing rule.
    expect(b.every((x) => x.label === null)).toBe(true);
  });
  it("labels the empty-date bucket 'Anytime'", () => {
    const b = plannerBuckets([task({ title: "A" }), task({ title: "D", due: "2026-07-01" })], today);
    expect(b.find((x) => x.key === "anytime")?.label).toBe("Anytime");
  });
});
```

- [ ] **Step 2: Run — expect FAIL** (`plannerBuckets` not exported):

Run: `npx vitest run tests/task-sections.test.ts`
Expected: FAIL.

- [ ] **Step 3: Replace `dateBuckets` with `plannerBuckets`.** In `src/utils/taskSections.ts`, change the import line `import { dueOf } from "./taskFields";` to `import { plannerDateOf } from "./taskFields";`, and replace the whole `dateBuckets` function with:

```ts
/** Do-date planner buckets: Overdue / Today / Upcoming / Anytime / Done, keyed
 * off the EFFECTIVE plan date = scheduled ?? due, so setting a do-date moves a
 * task's plan while a deadline-only task still buckets by its deadline
 * (non-regressing). Headers render only once a dated open task exists — a vault
 * that never uses dates keeps the flat list it always had. */
export function plannerBuckets(tasks: AggTask[], today: string): Bucket[] {
  const groups: Record<string, AggTask[]> = {
    overdue: [], today: [], upcoming: [], anytime: [], done: [],
  };
  for (const t of tasks) {
    if (t.done) groups.done.push(t);
    else {
      const d = plannerDateOf(t);
      if (!d) groups.anytime.push(t);
      else if (d < today) groups.overdue.push(t);
      else if (d === today) groups.today.push(t);
      else groups.upcoming.push(t);
    }
  }
  const showHeaders =
    groups.overdue.length + groups.today.length + groups.upcoming.length > 0;
  return [
    { key: "overdue", label: "Overdue" },
    { key: "today", label: "Today" },
    { key: "upcoming", label: "Upcoming" },
    { key: "anytime", label: "Anytime" },
    { key: "done", label: "Done" },
  ]
    .map(({ key, label }) => ({ key, label: showHeaders ? label : null, tasks: groups[key] }))
    .filter((b) => b.tasks.length > 0);
}
```

- [ ] **Step 4: Point `useTaskDisplay` at it + set the aggregate default.** In `src/composables/useTaskDisplay.ts`:

Change the taskSections import to bring in `plannerBuckets` instead of `dateBuckets`:

```ts
import { archivedMatcher, type Bucket, listSections, plannerBuckets, tagSections } from "../utils/taskSections";
```

Change the grouping ref initializer to pass an aggregate default override:

```ts
  const grouping = ref<Grouping>(loadGrouping(sortViewKey, isAggregate.value ? "dates" : undefined));
```

In the `buckets` computed, change the final `return dateBuckets(...)` line to:

```ts
    return plannerBuckets(filteredTasks.value, localToday());
```

- [ ] **Step 5: Add the optional default override to the per-view store + `loadGrouping`.** In `src/utils/perViewStore.ts`, change the `PerViewStore<T>` interface's `load` signature and the returned `load` impl:

```ts
  /** ...a missing or corrupt entry degrades to `defaultOverride ?? defaultValue`. */
  load(viewKey: string, defaultOverride?: T): T;
```

```ts
    load(viewKey, defaultOverride) {
      return sanitize(readAll()[viewKey]) ?? cloneDefault(defaultOverride ?? defaultValue);
    },
```

In `src/utils/taskGrouping.ts`, change `loadGrouping`:

```ts
export function loadGrouping(viewKey: string, defaultOverride?: Grouping): Grouping {
  return store.load(viewKey, defaultOverride);
}
```

- [ ] **Step 6: Relabel the grouping control.** In `src/components/TaskViewControls.vue`, change the `GROUPINGS` `dates` label (key stays `dates`):

```ts
const GROUPINGS = [
  { key: "lists", label: "Lists" },
  { key: "dates", label: "Plan" },
  { key: "tags", label: "Tags" },
] as const;
```

- [ ] **Step 7: Write/adjust the grouping default test.** In `tests/task-grouping.test.ts`, add (and clear localStorage between cases as the file already does):

```ts
  it("aggregate default is Plan (dates) only when unset; a stored choice wins", () => {
    expect(loadGrouping("all", "dates")).toBe("dates"); // unset → override
    saveGrouping("all", "lists");
    expect(loadGrouping("all", "dates")).toBe("lists"); // persisted wins
    expect(loadGrouping("vault-1")).toBe("lists"); // per-vault default unchanged
  });
```

- [ ] **Step 8: Run the affected suites + typecheck — expect PASS:**

Run: `npx vitest run tests/task-sections.test.ts tests/task-grouping.test.ts && npm run build`
Expected: PASS.

- [ ] **Step 9: Run the full frontend suite to catch label fallout** (any test asserting the visible "Dates" label or a `dateBuckets` import must move to "Plan"/`plannerBuckets`):

Run: `npm test`
Expected: PASS — fix any assertion that referenced the old `dateBuckets`/"Dates" label/"No date" label; a *bucket-placement* assertion that changes signals a real slip and must be investigated, not blindly updated.

- [ ] **Step 10: Commit:**

```bash
git add src/utils/taskSections.ts src/composables/useTaskDisplay.ts src/utils/perViewStore.ts src/utils/taskGrouping.ts src/components/TaskViewControls.vue tests/task-sections.test.ts tests/task-grouping.test.ts
git commit -m "feat(ui): do-date Planner buckets + aggregate default grouping

The date grouping buckets by the effective plan date (scheduled ?? due) as
Overdue/Today/Upcoming/Anytime/Done; label becomes Plan (key stays dates, no
migration); the aggregate view defaults to Plan only when unset."
```

---

### Task 5: Frontend — the do-date chip on the row (additive, non-regressing)

**Files:**
- Modify: `src/components/TaskRow.vue` (do-date chip)
- Modify: `tests/tasks.test.ts` (assert the chip appears only for scheduled tasks)

**Interfaces:**
- Consumes: `scheduledOf`, `dueOf` (Task 3).

- [ ] **Step 1: Write the failing row test.** In `tests/tasks.test.ts`, add a case (use the file's existing mount helper; render one scheduled task and one due-only task):

```ts
  it("shows a do-date chip only for scheduled tasks, leaving due-only rows unchanged", async () => {
    // A far-past scheduled date always renders as a short date ("Jan 15")
    // regardless of when the suite runs — the relative-label branches (Today/
    // Tomorrow/weekday) are unit-tested deterministically in task-fields.test.ts.
    const wrapper = await mountTasks([
      task({ title: "Sched", scheduled: "2020-01-15" }),
      task({ title: "DueOnly", due: "2026-07-20" }),
    ]);
    const chips = wrapper.findAll('[data-testid="task-scheduled"]');
    expect(chips).toHaveLength(1); // only the scheduled task
    expect(chips[0].text()).toContain("Jan 15");
  });
```

(Adjust `mountTasks`/`task` to whatever the file's existing helpers are named; the assertion is what matters.)

- [ ] **Step 2: Run — expect FAIL** (no `task-scheduled` element):

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL.

- [ ] **Step 3: Add the chip.** In `src/components/TaskRow.vue`:

Extend the taskFields import:

```ts
import { dueOf, localToday, scheduledOf } from "../utils/taskFields";
```

**First, single-source the date formatters in `src/utils/taskFields.ts`** (the do-date chip's relative label and the row's existing due label must share one implementation — the repo's duplicate-code gate would flag two `MONTHS` arrays). Add:

```ts
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

// A short, locale-independent date label ("Jul 15"). Shared by the row's due
// element and the do-date chip's far-date fallback.
export function shortDate(date: string): string {
  const [, m, day] = date.split("-");
  const month = MONTHS[Number(m) - 1];
  return month ? `${month} ${Number(day)}` : date;
}

// A friendly relative label for a plan date, given today (both YYYY-MM-DD):
// Today / Tomorrow / a weekday within the next 6 days ("Sat") / else shortDate.
// `today` is injected so it's deterministic and unit-testable (no clock mock).
export function relativeDateLabel(date: string, today: string): string {
  if (date === today) return "Today";
  const d = new Date(`${date}T00:00:00`);
  const t = new Date(`${today}T00:00:00`);
  const diffDays = Math.round((d.getTime() - t.getTime()) / 86_400_000);
  if (diffDays === 1) return "Tomorrow";
  if (diffDays > 1 && diffDays < 7) return WEEKDAYS[d.getDay()];
  return shortDate(date);
}
```

Add tests to `tests/task-fields.test.ts`:

```ts
import { relativeDateLabel, shortDate } from "../src/utils/taskFields";

describe("relativeDateLabel", () => {
  it("labels Today / Tomorrow / weekday / short date", () => {
    const today = "2026-07-24"; // a Friday
    expect(relativeDateLabel("2026-07-24", today)).toBe("Today");
    expect(relativeDateLabel("2026-07-25", today)).toBe("Tomorrow");
    expect(relativeDateLabel("2026-07-27", today)).toBe("Mon"); // within the next 6 days
    expect(relativeDateLabel("2026-08-10", today)).toBe(shortDate("2026-08-10")); // far → short date
    expect(relativeDateLabel("2026-07-20", today)).toBe("Jul 20"); // past → short date
  });
});
```

**Then in `src/components/TaskRow.vue`:** extend the taskFields import and **delete the local `dueLabel` function** (its `MONTHS` array + body now live in `shortDate`):

```ts
import { dueOf, localToday, relativeDateLabel, scheduledOf, shortDate } from "../utils/taskFields";
```

Update the existing due `<span>`'s interpolation from `dueLabel(dueOf(task)!)` to `shortDate(dueOf(task)!)` — byte-identical output, just single-sourced (the deadline element's rendering is unchanged).

Add the chip helper (after `isOverdue`):

```ts
// The do-date chip label — shown only when a scheduled date is set AND differs
// from the deadline (when they coincide, the existing due element already shows
// the date). Additive: a task with no scheduled date renders exactly as before.
function scheduledChip(t: AggTask): string | null {
  const s = scheduledOf(t);
  if (!s || s === dueOf(t)) return null;
  return relativeDateLabel(s, localToday());
}
```

In the template, add the chip **before** the existing `task-due` span (after the tag chips), using the shared **`Chip` primitive** (already imported by `TaskRow`) with the `accent` variant — NOT a hand-rolled pill (§5 requires the primitive; AGENTS.md GAP-66 design-system discipline):

```html
        <Chip
          v-if="scheduledChip(task)"
          variant="accent"
          data-testid="task-scheduled"
          :title="`Scheduled for ${scheduledChip(task)}`"
        >{{ scheduledChip(task) }}</Chip>
        <span
          v-if="dueOf(task)"
          data-testid="task-due"
          ...
```

(The existing `task-due` span keeps its class + `isOverdue` red styling untouched — only its formatter call moved to the shared `shortDate`, output identical. If `Chip` renders with `inheritAttrs` off and does not forward `data-testid`/`title`, wrap it or pass them via a `label` prop — verify against `Chip.vue` when implementing; the tag chips already carry a `data-testid`, so fallthrough is the expected behavior.)

- [ ] **Step 4: Run — expect PASS:**

Run: `npx vitest run tests/tasks.test.ts tests/task-fields.test.ts && npm test`
Expected: PASS (at this step the row gains only the do-date chip, so a due-only row's existing content/assertions don't change — the always-present schedule trigger arrives in Task 6, which updates those expectations there).

- [ ] **Step 5: Commit:**

```bash
git add src/components/TaskRow.vue src/utils/taskFields.ts tests/task-fields.test.ts tests/tasks.test.ts
git commit -m "feat(ui): additive do-date chip on task rows

Shows the scheduled date (Today/short date) as a chip, only when set and
distinct from the deadline; the existing due element is untouched, so
scheduled-less rows render byte-identically."
```

---

### Task 6: Frontend — quick-schedule menu + reschedule-overdue verb

**Files:**
- Modify: `src/utils/taskFields.ts` (`localDatePlus`, `comingSaturday` date helpers)
- Create: `src/composables/useTaskSchedule.ts`
- Create: `src/components/TaskScheduleMenu.vue`
- Modify: `src/components/TaskRow.vue` (render `TaskScheduleMenu`, re-emit `schedule`)
- Modify: `src/components/Tasks.vue` (wire `useTaskSchedule`; `@schedule`; Overdue "Reschedule" button)
- Create: `tests/task-schedule.test.ts`
- Modify: `tests/tasks.test.ts` (Overdue reschedule button)

**Interfaces:**
- Consumes: `busy`, `sortInPlace` (from `useTaskActions`/`useTaskDisplay`); `reflectStampedId` (`taskMutations`); `localToday` (`taskFields`).
- Produces: `useTaskSchedule({tasks, sortInPlace, busy}) → { quickSchedule(task, date|null), rescheduleOverdue(overdue[]) }`; `TaskScheduleMenu` emitting `schedule(value: string|null)`; `localDatePlus(days)`, `comingSaturday()`.

- [ ] **Step 1: Write the failing composable test.** Create `tests/task-schedule.test.ts`:

```ts
import { mockIPC } from "@tauri-apps/api/mocks";
import { setActivePinia, createPinia } from "pinia";
import { ref } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useTaskSchedule } from "../src/composables/useTaskSchedule";
import type { AggTask } from "../src/types";
import { localToday } from "../src/utils/taskFields";

function agg(p: Partial<AggTask>): AggTask {
  return {
    path: "p", title: "t", status: "new", created: "2026-07-01", done: false,
    due: null, scheduled: null, priority: null, tags: [], list: "", order: null, id: null,
    vaultId: "v", vaultName: "V", ...p,
  };
}

beforeEach(() => setActivePinia(createPinia()));
afterEach(() => vi.restoreAllMocks());

describe("quickSchedule", () => {
  it("writes the do-date optimistically", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => { if (cmd === "update_task") { calls.push(args); return null; } });
    const t = agg({ path: "a" });
    const tasks = ref<AggTask[]>([t]);
    const { quickSchedule } = useTaskSchedule({ tasks, sortInPlace: () => {}, busy: ref(new Set()) });
    await quickSchedule(t, "2026-07-20");
    expect(t.scheduled).toBe("2026-07-20");
    expect(calls[0]).toMatchObject({ patch: { scheduled: "2026-07-20" } });
  });
  it("reverts on failure", async () => {
    mockIPC((cmd) => { if (cmd === "update_task") throw new Error("nope"); });
    const t = agg({ path: "a", scheduled: "2026-07-01" });
    const { quickSchedule } = useTaskSchedule({ tasks: ref([t]), sortInPlace: () => {}, busy: ref(new Set()) });
    await quickSchedule(t, "2026-07-20");
    expect(t.scheduled).toBe("2026-07-01"); // reverted
  });
});

describe("rescheduleOverdue", () => {
  it("stamps today on all, best-effort — one failure reverts only its own task", async () => {
    mockIPC((cmd, args) => {
      if (cmd !== "update_task") return;
      if ((args as { path: string }).path === "bad") throw new Error("nope");
      return null;
    });
    const ok = agg({ path: "ok", title: "OK", scheduled: "2026-07-01" });
    const bad = agg({ path: "bad", title: "BAD", scheduled: "2026-07-02" });
    const { rescheduleOverdue } = useTaskSchedule({ tasks: ref([ok, bad]), sortInPlace: () => {}, busy: ref(new Set()) });
    await rescheduleOverdue([ok, bad]);
    expect(ok.scheduled).toBe(localToday()); // landed
    expect(bad.scheduled).toBe("2026-07-02"); // reverted (only this one)
  });
  it("holds back a busy row and never silently drops it", async () => {
    const calls: string[] = [];
    mockIPC((cmd, args) => { if (cmd === "update_task") { calls.push((args as { path: string }).path); return null; } });
    const free = agg({ path: "free", title: "Free", scheduled: "2026-07-01" });
    const busyRow = agg({ path: "busy", title: "Busy", scheduled: "2026-07-02" });
    const busy = ref(new Set(["busy"]));
    const { rescheduleOverdue } = useTaskSchedule({ tasks: ref([free, busyRow]), sortInPlace: () => {}, busy });
    await rescheduleOverdue([free, busyRow]);
    expect(calls).toEqual(["free"]); // the busy row is NOT written…
    expect(free.scheduled).toBe(localToday());
    expect(busyRow.scheduled).toBe("2026-07-02"); // …left untouched (still overdue) and named in the toast
  });
});
```

- [ ] **Step 2: Run — expect FAIL** (`useTaskSchedule` missing):

Run: `npx vitest run tests/task-schedule.test.ts`
Expected: FAIL.

- [ ] **Step 3: Add the date helpers.** In `src/utils/taskFields.ts`, after `localToday`:

```ts
// N days from local today, as YYYY-MM-DD (local calendar — never UTC slicing,
// matching localToday's rule so a near-midnight schedule doesn't slip a day).
export function localDatePlus(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() + days);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// The coming Saturday (today if today is Saturday), as YYYY-MM-DD.
export function comingSaturday(): string {
  const dow = new Date().getDay(); // 0=Sun … 6=Sat
  return localDatePlus((6 - dow + 7) % 7);
}
```

- [ ] **Step 4: Create the composable** `src/composables/useTaskSchedule.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { type Ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { AggTask } from "../types";
import { localToday } from "../utils/taskFields";
import { reflectStampedId } from "../utils/taskMutations";

// The plan-my-day verbs: quick-schedule (set/clear a task's do-date) and
// reschedule-overdue. Optimistic with per-row revert, serialized through the
// SAME busy guard the other row writes share (threaded in from useTaskActions),
// so a schedule can't race a toggle/edit on the same task. update_task stamps
// an id on an id-enabled vault (any non-empty patch), so each write reflects a
// freshly-stamped id like the edit/reorder paths.
export function useTaskSchedule(opts: {
  tasks: Ref<AggTask[]>;
  sortInPlace: () => void;
  busy: Ref<Set<string>>;
}) {
  const { sortInPlace, busy } = opts;
  const notifications = useNotificationsStore();

  // `date` = a YYYY-MM-DD do-date, or null to clear. Optimistic + revert.
  async function quickSchedule(task: AggTask, date: string | null): Promise<void> {
    if (busy.value.has(task.path)) return;
    const prev = task.scheduled;
    task.scheduled = date;
    sortInPlace();
    busy.value.add(task.path);
    try {
      const patch = date === null ? { clearScheduled: true } : { scheduled: date };
      reflectStampedId(
        task,
        await invoke<string | null>("update_task", { id: task.vaultId, path: task.path, patch }),
      );
    } catch (e) {
      task.scheduled = prev;
      sortInPlace();
      notifications.error(String(e));
      logWarning(`quickSchedule failed: ${String(e)}`);
    } finally {
      busy.value.delete(task.path);
    }
  }

  // Reschedule EVERY task in `overdue` to today — genuinely best-effort:
  // independent per-task writes that do NOT stop on a rejection (unlike the
  // rank materialize's fail-fast batch), reverting only the failed task. A row
  // with a write already in flight can't be safely re-written here (two
  // read-modify-write saves would race), so it's held out of the batch — but it
  // is NOT dropped silently: it's named in the summary alongside any failures,
  // so the "reschedule all" action never reports success while quietly leaving a
  // task overdue (Codex, PR #75). The user can retry once its save lands.
  async function rescheduleOverdue(overdue: AggTask[]): Promise<void> {
    const today = localToday();
    const skipped = overdue.filter((t) => busy.value.has(t.path)).map((t) => t.title);
    const targets = overdue.filter((t) => !busy.value.has(t.path));
    if (targets.length > 0) {
      const prev = new Map(targets.map((t) => [t.path, t.scheduled] as const));
      for (const t of targets) {
        t.scheduled = today;
        busy.value.add(t.path);
      }
      sortInPlace();
      for (const t of targets) {
        try {
          reflectStampedId(
            t,
            await invoke<string | null>("update_task", {
              id: t.vaultId, path: t.path, patch: { scheduled: today },
            }),
          );
        } catch (e) {
          t.scheduled = prev.get(t.path) ?? null;
          skipped.push(t.title); // write-failed → still overdue → named
          logWarning(`rescheduleOverdue failed for ${t.title}: ${String(e)}`);
        } finally {
          busy.value.delete(t.path);
        }
      }
      sortInPlace();
    }
    // One honest summary: every task still overdue — write-failed OR held back
    // for a busy save — is named; nothing is silently left behind.
    if (skipped.length > 0) {
      notifications.error(`Couldn't reschedule (still overdue): ${skipped.join(", ")}.`);
    }
  }

  return { quickSchedule, rescheduleOverdue };
}
```

- [ ] **Step 5: Run the composable test — expect PASS:**

Run: `npx vitest run tests/task-schedule.test.ts`
Expected: PASS.

- [ ] **Step 6: Create the menu component** `src/components/TaskScheduleMenu.vue` (modeled on `TaskSectionMenu.vue`'s popover + Escape/focus/outside-click handling):

```vue
<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { comingSaturday, localDatePlus, localToday } from "../utils/taskFields";
import AppIcon from "./AppIcon.vue";
import IconButton from "./ui/IconButton.vue";

// A per-row schedule popover: Today / Tomorrow / This weekend / a native date
// pick / Clear. Presentational — it emits the chosen do-date (or null to
// clear); the container runs the write. Escape/focus/outside-click follow
// TaskSectionMenu's pattern (GAP-27: swallow own Escape so it doesn't bubble
// to PanelRoot's panel-close handler).
const props = defineProps<{ title: string; scheduled: string | null; busy: boolean }>();
const emit = defineEmits<{ (e: "schedule", value: string | null): void }>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const popover = ref<HTMLElement | null>(null);
const pick = ref("");

function toggle() {
  open.value = !open.value;
  if (open.value) pick.value = props.scheduled ?? "";
}
function close() { open.value = false; }
function choose(value: string | null) {
  if (props.busy) return;
  emit("schedule", value);
  close();
}
function onPick() { if (pick.value) choose(pick.value); }

watch(open, (o) => { if (o) void nextTick(() => popover.value?.focus()); });
function onRootKeydown(e: KeyboardEvent) {
  if (e.key !== "Escape" || e.isComposing || !open.value) return;
  e.preventDefault();
  e.stopPropagation();
  close();
}
function onWindowPointerDown(e: PointerEvent) {
  if (!open.value) return;
  if (root.value && !root.value.contains(e.target as Node)) close();
}
onMounted(() => window.addEventListener("pointerdown", onWindowPointerDown));
onBeforeUnmount(() => window.removeEventListener("pointerdown", onWindowPointerDown));

const itemClass =
  "cursor-pointer rounded px-1.5 py-0.5 text-left text-micro text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-40";
</script>

<template>
  <div ref="root" class="relative inline-flex" @keydown="onRootKeydown">
    <!-- Reuse the shared IconButton (size sm) — it sits beside TaskRow's own
         IconButtons and owns the hover/focus/disabled treatment (GAP-66). It
         forwards the native click (TaskRow binds @click on it), so @click.stop
         applies; if the built Chip/IconButton turns out to declare a `click`
         emit instead, stop propagation inside toggle() rather than via the
         modifier. `label` is IconButton's aria-label prop (the TaskRow usage). -->
    <IconButton
      size="sm"
      :data-testid="`task-schedule-${title}`"
      :disabled="busy"
      :label="`Schedule ${title}`"
      title="Schedule"
      @click.stop="toggle"
    >
      <AppIcon :size="14">
        <rect x="3" y="4" width="18" height="18" rx="2" />
        <path d="M16 2v4M8 2v4M3 10h18" />
      </AppIcon>
    </IconButton>
    <div
      v-if="open"
      ref="popover"
      tabindex="-1"
      :data-testid="`task-schedule-popover-${title}`"
      class="absolute right-0 top-full z-10 mt-1 flex min-w-40 flex-col gap-0.5 rounded-control border border-white/10 bg-slate-800 p-1 shadow-lg focus:outline-none"
      @click.stop
    >
      <button type="button" data-testid="task-schedule-today" :class="itemClass" @click="choose(localToday())">Today</button>
      <button type="button" data-testid="task-schedule-tomorrow" :class="itemClass" @click="choose(localDatePlus(1))">Tomorrow</button>
      <button type="button" data-testid="task-schedule-weekend" :class="itemClass" @click="choose(comingSaturday())">This weekend</button>
      <input
        v-model="pick"
        type="date"
        data-testid="task-schedule-pick"
        aria-label="Pick a do date"
        class="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-micro text-fg focus:border-focus focus:outline-none"
        @change="onPick"
      >
      <button v-if="scheduled" type="button" data-testid="task-schedule-clear" :class="itemClass" @click="choose(null)">Clear</button>
    </div>
  </div>
</template>
```

- [ ] **Step 7: Render the menu in the row + re-emit.** In `src/components/TaskRow.vue`:

Import it: `import TaskScheduleMenu from "./TaskScheduleMenu.vue";`

Add to `defineEmits`: `(e: "schedule", value: string | null): void;`

In the template, add the menu just before the `task-edit` IconButton (so the schedule glyph sits with the row actions):

```html
      <TaskScheduleMenu
        :title="task.title"
        :scheduled="task.scheduled"
        :busy="busy"
        @schedule="$emit('schedule', $event)"
      />
      <IconButton
        size="sm"
        data-testid="task-edit"
        ...
```

- [ ] **Step 8: Wire the container.** In `src/components/Tasks.vue`:

Import the composable: `import { useTaskSchedule } from "../composables/useTaskSchedule";`

After the `useTaskActions` destructure (which exposes `busy`), add:

```ts
const { quickSchedule, rescheduleOverdue } = useTaskSchedule({ tasks, sortInPlace, busy });
```

On the `<TaskRow>`, add the handler: `@schedule="quickSchedule(task, $event)"`.

In the bucket header `<div>`, add a Reschedule button that shows only on the Overdue bucket (place it after the `<h3>`):

```html
            <button
              v-if="bucket.key === 'overdue'"
              type="button"
              data-testid="task-reschedule-overdue"
              class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-1.5 py-0.5 text-micro text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
              @click="rescheduleOverdue(bucket.tasks)"
            >
              Reschedule → Today
            </button>
```

- [ ] **Step 9: Add the container test** in `tests/tasks.test.ts` (the Overdue reschedule button calls `update_task` for each overdue row):

```ts
  it("reschedules all overdue to today from the Overdue header", async () => {
    const calls: { path: string }[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "update_task") { calls.push(args as { path: string }); return null; }
      if (cmd === "list_tasks") return [];
      if (cmd === "list_vaults") return [];
      return undefined;
    });
    const wrapper = await mountTasks([
      task({ path: "a", title: "A", due: "2026-01-01" }),
      task({ path: "b", title: "B", due: "2026-01-02" }),
    ]);
    await wrapper.get('[data-testid="task-reschedule-overdue"]').trigger("click");
    await flushPromises();
    expect(calls.map((c) => c.path).sort()).toEqual(["a", "b"]);
  });
```

(Adapt to the file's mount helper and imports — `flushPromises` from `@vue/test-utils`. Two `due`-in-the-past tasks land in Overdue given a real `localToday()`.)

- [ ] **Step 10: Run the affected suites + full suite + typecheck — expect PASS:**

Run: `npx vitest run tests/task-schedule.test.ts tests/tasks.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 11: Commit:**

```bash
git add src/utils/taskFields.ts src/composables/useTaskSchedule.ts src/components/TaskScheduleMenu.vue src/components/TaskRow.vue src/components/Tasks.vue tests/task-schedule.test.ts tests/tasks.test.ts
git commit -m "feat(ui): quick-schedule menu + reschedule-overdue verb

A per-row schedule popover (Today/Tomorrow/This weekend/pick/Clear) writes the
do-date optimistically; the Overdue header gets a genuinely best-effort
reschedule-all-to-today. Both serialize through the shared busy guard."
```

---

### Task 7: Frontend — schedule on create (composer) and on edit (inline editor)

**Files:**
- Modify: `src/components/TaskComposer.vue` (do-date input + payload)
- Modify: `src/components/TaskEditor.vue` (do-date input + patch)
- Modify: `src/composables/useTaskActions.ts` (`applyFieldPatch` handles `scheduled`)
- Modify: `src/components/Tasks.vue` (`add()` threads `scheduled`; `AddPayload` type)
- Modify: `tests/task-editor.test.ts`, `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `scheduledOf` (Task 3); `TaskPatch.scheduled`/`clearScheduled` (Task 3); the `scheduled` IPC arg (Task 2).

- [ ] **Step 1: Write the failing editor test.** In `tests/task-editor.test.ts`, add:

```ts
  it("sends scheduled when set and clearScheduled when emptied", async () => {
    // Set a do-date on a task that had none.
    const setW = mountEditor(task({ scheduled: null }));
    await setW.get('[data-testid="task-edit-scheduled"]').setValue("2026-07-20");
    await setW.get('[data-testid="task-edit-save"]').trigger("click");
    expect(setW.emitted("save")![0][0]).toMatchObject({ scheduled: "2026-07-20" });

    // Clear an existing do-date.
    const clrW = mountEditor(task({ scheduled: "2026-07-20" }));
    await clrW.get('[data-testid="task-edit-scheduled"]').setValue("");
    await clrW.get('[data-testid="task-edit-save"]').trigger("click");
    expect(clrW.emitted("save")![0][0]).toMatchObject({ clearScheduled: true });
  });
```

(Use the file's existing `mountEditor`/`task` helpers.)

- [ ] **Step 2: Run — expect FAIL** (no `task-edit-scheduled`):

Run: `npx vitest run tests/task-editor.test.ts`
Expected: FAIL.

- [ ] **Step 3: Add the editor do-date field.** In `src/components/TaskEditor.vue`:

Import `scheduledOf`: change the taskFields import to `import { dueOf, parseTagsInput, scheduledOf } from "../utils/taskFields";`

Add a draft ref (after `editDue`):

```ts
const editScheduled = ref(scheduledOf(props.task) ?? "");
```

In `buildPatch()`, after the `due` block, add (mirrors due):

```ts
  if (editScheduled.value !== (scheduledOf(props.task) ?? "")) {
    if (editScheduled.value === "") patch.clearScheduled = true;
    else patch.scheduled = editScheduled.value;
  }
```

In the template, add a "Do" date input next to the existing due input (inside the same `flex items-center gap-1` row that holds `task-edit-due`, or a new row):

```html
      <input
        v-model="editScheduled"
        data-testid="task-edit-scheduled"
        type="date"
        aria-label="Do date"
        title="Do date (when you plan to work on it)"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none"
      >
```

- [ ] **Step 4: Handle `scheduled` in the optimistic field apply.** In `src/composables/useTaskActions.ts`, in `applyFieldPatch`:

Extend the `before` snapshot to include scheduled:

```ts
    const before = { title: task.title, due: task.due, scheduled: task.scheduled, priority: task.priority, tags: task.tags };
```

After the `due` handling lines, add:

```ts
    if (patch.clearScheduled) task.scheduled = null;
    else if (patch.scheduled) task.scheduled = patch.scheduled;
```

- [ ] **Step 5: Run the editor test — expect PASS:**

Run: `npx vitest run tests/task-editor.test.ts`
Expected: PASS.

- [ ] **Step 6: Write the failing composer test.** In `tests/tasks.test.ts`, add a case asserting the composer's do-date reaches `add_task`:

```ts
  it("passes a do-date from the composer to add_task", async () => {
    let addArgs: Record<string, unknown> | undefined;
    mockIPC((cmd, args) => {
      if (cmd === "add_task") { addArgs = args as Record<string, unknown>; return { path: "n", title: "N", status: "new", created: "2026-07-10", done: false, due: null, scheduled: (args as { scheduled?: string }).scheduled ?? null, priority: null, tags: [], list: "", order: null, id: null }; }
      if (cmd === "list_tasks") return [];
      if (cmd === "list_vaults") return [{ id: "v", name: "V", path: "/v", open: false }];
      if (cmd === "count_open_tasks") return 0;
      return undefined;
    });
    const wrapper = await mountTasks([], { vaultId: "v" });
    await wrapper.get('[data-testid="task-input"]').setValue("New");
    await wrapper.get('[data-testid="task-add-options"]').trigger("click");
    await wrapper.get('[data-testid="task-add-scheduled"]').setValue("2026-07-22");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(addArgs).toMatchObject({ scheduled: "2026-07-22" });
  });
```

- [ ] **Step 7: Run — expect FAIL** (no `task-add-scheduled`):

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL.

- [ ] **Step 8: Add the composer do-date field + payload.** In `src/components/TaskComposer.vue`:

Add a draft ref (after `addDue`): `const addScheduled = ref("");`

Extend the `submit` emit type to include `scheduled: string;` and add `scheduled: addScheduled.value,` to the emitted payload object in `submit()`.

In `reset()`, add `addScheduled.value = "";`.

In the options row template (the `showAddOptions` block with `task-add-due`), add a do-date input beside the due input:

```html
      <input
        v-model="addScheduled"
        data-testid="task-add-scheduled"
        type="date"
        aria-label="Do date"
        title="Do date"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none"
      >
```

Also update the **options-toggle (⋯) button's accessible name** — it now reveals a do-date field too, so its stale `aria-label`/`title` must mention scheduling. The button currently reads `:aria-label="showAddOptions ? 'Hide task options' : 'Set due date or priority'"` and `title="Due date / priority"`; change the closed-state label to `'Set do date, due date or priority'` and the title to `'Do date / due date / priority'`. Add a `tests/tasks.test.ts` assertion that the `task-add-options` button's `aria-label` (closed state) includes "do date".

Add `scheduled: string;` to the `AddPayload` type.

In `add(payload)`, after the `due` arg line, add:

```ts
    if (payload.scheduled) args.scheduled = payload.scheduled;
```

(The optimistic `tasks.value.unshift({ ...created, ... })` already carries `scheduled` from the returned `TaskDto` — no change needed there.)

- [ ] **Step 10: Run the affected suites + full suite + typecheck — expect PASS:**

Run: `npx vitest run tests/task-editor.test.ts tests/tasks.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 11: Commit:**

```bash
git add src/components/TaskComposer.vue src/components/TaskEditor.vue src/composables/useTaskActions.ts src/components/Tasks.vue tests/task-editor.test.ts tests/tasks.test.ts
git commit -m "feat(ui): schedule a do-date on create and on edit

The composer and the inline editor gain a do-date field; the editor sends
scheduled/clearScheduled in the changed-fields patch, and applyFieldPatch
applies it optimistically."
```

---

### Task 8: Docs + quality baselines

**Files:**
- Modify: `AGENTS.md` (tasks-domain section + the IPC-surface table field notes)
- Modify: `CONTEXT.md` (new domain terms)
- Modify: `docs/prds/task-management.md` (do-date + planner shipped)
- Modify: `docs/use-cases/aggregated-task-dashboard-and-lists.md` and/or `docs/use-cases/per-vault-task-list.md` (planner shipped)
- Modify: `docs/Gaps.md` (deferred This-Evening/Someday; the two duplicated reserved-key lists cleanup)
- Modify (only if a gate reports drift): `scripts/loc-baseline.json`, `scripts/quality-baseline.json`, `vite.config.ts` coverage floors

**Interfaces:** none (docs).

- [ ] **Step 1: Update AGENTS.md — the tasks domain.** In the tasks-domain section, document: the `scheduled` (do) date field (read-lenient, written never-clobber, emitted after `due`, reserved in BOTH `disk.rs` and `id.rs` reserved-key sets); the do-date **Planner** grouping (effective `scheduled ?? due`; label-only "Dates→Plan"/"No date→Anytime", grouping key unchanged, no migration); the aggregate Plan default (only when unset); and the plan-my-day verbs (`quickSchedule`, `rescheduleOverdue` best-effort, schedule-on-create/edit). In the IPC-surface table, annotate `add_task` (now takes optional `scheduled`), `update_task` (patch carries `scheduled`/`clearScheduled`), and `list_tasks` (returns `scheduled`) — no new commands.

- [ ] **Step 2: Update CONTEXT.md — ubiquitous language.** Add: **Do Date / Scheduled Date** (when the user plans to work a Task, distinct from **Due Date / Deadline**), **Planner** (the do-date grouping), **Today / Upcoming / Anytime** (planner buckets), **Focus** (the set scheduled for today — will feed the future widget). Use the `domain-modeling` skill if available.

- [ ] **Step 3: Update the PRD + use case.** In `docs/prds/task-management.md`, note the do-date/Planner increment shipped (Task Model gains `scheduled`; the Dashboard's Today/Overdue/Upcoming now derive from the do-date). In the aggregated-dashboard use case, note the cross-vault planner landed.

- [ ] **Step 4: Update docs/Gaps.md.** Record: (a) "This Evening" sub-bucket + a distinct "Someday" horizon are deferred (each needs a second signal); (b) the two duplicated `RESERVED_TASK_KEYS` constants (`disk.rs` + `id.rs`) should be single-sourced (small cleanup); (c) if not already tracked, the local-midnight bucketing edge (buckets use `localToday()`, so a task's bucket can shift a day at local midnight — acceptable, matches `add_task`'s local-date rule); (d) **the `scheduled`-as-id-property clobber edge** — a vault that had hand-configured its task-id property to the literal `scheduled` (now a reserved managed field) still has `scheduled: <stable-id>` on disk; reserving stops future gen/read and the read is harmless (`scheduledOf` rejects a non-date), but a schedule/clear write on such a task overwrites the id. Accepted edge: no auto-migration (mass vault mutation is out of policy) and no hard block (near-zero exposure); remedy = re-point the id property to a non-reserved name before scheduling. Note it explicitly as a non-never-clobber case so the invariant docs stay honest; (e) the pre-existing asymmetry that `due` stores the RAW frontmatter scalar in `TaskItem`/`TaskDto` (filtered only frontend-side by `dueOf`), whereas `scheduled` is validator-filtered in core — a small cleanup would filter `due` in core too, but changing `due`'s DTO/MCP contract is out of scope here.

- [ ] **Step 5: Run every quality gate; update baselines only where a gate instructs.**

Run:
```bash
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault_buddy_core -p vault_buddy_mcp && cargo test -p vault-buddy --lib
```
Expected: all PASS. If `check:loc` or `check:quality` reports an *improved* metric, re-run that gate with `--update` and stage the baseline. If a coverage floor rose, bump it in `vite.config.ts`. If a LOC baseline needs raising because a file legitimately grew (e.g. `Tasks.vue`), that is a reviewed change — note the justification in the commit body. Do NOT loosen a baseline without justification.

- [ ] **Step 6: Commit:**

```bash
git add AGENTS.md CONTEXT.md docs/ scripts/ vite.config.ts
git commit -m "docs: do-date foundation + planner — AGENTS/CONTEXT/PRD/Gaps + baselines"
```

---

## Self-Review (completed while writing)

- **Spec coverage:** §1 field → Tasks 1–3; §2 planner buckets → Task 4; §3 aggregate default → Task 4 (`loadGrouping` override) + no-new-IPC noted; §4 verbs → Task 6 (quick-schedule + best-effort reschedule) and Task 7 (create/edit); §5 row chip (additive, due untouched) → Task 5; reserved-key catch (id.rs) → Task 1 Steps 9–10; MCP call-site → Task 2 Step 7; docs/domain/baselines → Task 8. No spec section is unimplemented.
- **Type/signature consistency:** `render_task`/`create_task`/`services::add_task` all gain a trailing `scheduled: Option<&str>`; `TaskItem.scheduled`/`TaskDto.scheduled`/TS `TaskItem.scheduled` are `Option<String>`/`string | null`; `TaskPatch`/`TaskPatchDto` use `scheduled`/`clearScheduled` (camelCase ↔ `clear_scheduled`); the composable helpers (`quickSchedule`, `rescheduleOverdue`) and menu emit (`schedule`) names match across Tasks 6–7.
- **No placeholders:** every code step shows the code; every run step shows the command + expected result.
