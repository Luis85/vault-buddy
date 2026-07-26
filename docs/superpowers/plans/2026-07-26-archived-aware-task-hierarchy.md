# Archived-Aware Task Hierarchy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the task-hierarchy surfaces respect archiving — an archived Task can no longer be assigned as a new parent, an archived-list Task is neither offered as a parent nor counted in a subtask badge, and a subtask landing in an archived list says so.

**Architecture:** Two rules with one authority each. Rule A (archived *status* blocks a new parent assignment) lives in core as a shared helper called from both phase-1 sites, with a disabled frontend input as a hint only. Rule B (archived *status or list* excluded from the picker and the open-subtask count) lives in the frontend where `archivedMatcher` already lives, keyed per vault. Rule C discloses when a new subtask inherits an archived list.

**Tech Stack:** Rust (`vault_buddy_core`, no Tauri types) + Vue 3 / TypeScript / Pinia, tested with `cargo test` and Vitest (happy-dom, `mockIPC`).

**Spec:** `docs/superpowers/specs/2026-07-26-archived-aware-task-hierarchy-design.md`

## Global Constraints

- **Never widen the vault write surface.** This increment adds a refusal and a frontend filter; it adds no new write path.
- **Refusals stay in phase 1.** A refused parent assignment must leave Task IDs **off** and nothing stamped. Never place a check after `resolve_parent_for_write`.
- **Inheritance is untouched.** The rule governs *assigning* a parent, never *having* one. An already-set archived parent must keep resolving and rendering.
- **Core does NOT learn about archived lists.** Rule A is archived-*status* only. List-archiving stays frontend-side.
- **Per-vault keying is mandatory** for all archived data in hierarchy code — ids and archived sets are both vault-scoped, and the aggregate view renders many vaults at once.
- **One rule, one implementation.** `archivedMatcher` (`src/utils/taskSections.ts`) is the only "is this list archived" test; never write a second membership check.
- Every regression test names its failure mode in a comment (repo TDD convention).
- Commit style: Conventional Commits, imperative subject, body explains the *why* and the failure mode.
- Rust gates: `cd src-tauri && cargo fmt --check`; `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test`.
- Frontend gates: `npm test`, `npm run build`.

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `src-tauri/core/src/services/tasks/parent/mod.rs` | Parent-assignment write path | Add `reject_archived_parent`; call it from both phase-1 sites |
| `src-tauri/core/src/services/tasks/parent/tests/mod.rs` | Parent write-path tests | Add refusal + non-regression tests |
| `src/utils/taskHierarchy.ts` | The one frontend hierarchy resolution rule | `openSubtaskCounts` / `buildHierarchyInfoByVault` take a per-vault archived map |
| `src/composables/useTaskListHierarchy.ts` | Main-list per-row hierarchy | Accept + forward the per-vault archived map |
| `src/composables/useTaskDetailTaskSet.ts` | Task Detail's task set + picker candidates | `pickerCandidates` also excludes archived-list tasks |
| `src/composables/useTaskLists.ts` | Per-vault list config cache | Expose `archivedByVault` map |
| `src/components/Tasks.vue` | Tasks view container | Load configs in the aggregate fan-out; pass the map through |
| `src/components/TaskDetail.vue` | Task Detail container | Pass `archivedLists` into the task set; disclose archived-list subtask |
| `src/components/TaskSubtasks.vue` | Subtasks section | Disable Add-subtask input on an archived Task |
| `tests/task-hierarchy.test.ts` | Frontend hierarchy tests | Count + picker cases |
| `tests/task-detail.test.ts` | Task Detail tests | Disabled input + disclosure cases |
| `docs/Gaps.md` | Audited backlog | Remove GAP-90/91/92 |
| `AGENTS.md` | Agent operating guide | Record both rules |

---

### Task 1: Core refuses an archived Task as a new parent

**Files:**
- Modify: `src-tauri/core/src/services/tasks/parent/mod.rs` (add helper near `reject_ambiguous_parent` ~line 523; call from `validate_parent_assignment` ~line 199 and `add_subtask` ~line 417)
- Test: `src-tauri/core/src/services/tasks/parent/tests/mod.rs`

**Interfaces:**
- Consumes: `tasks::TaskItem` (fields `path: PathBuf`, `status: String`), `tasks::list_tasks_structural(root, Some(&prop)) -> Result<Vec<TaskItem>, String>`
- Produces: `fn reject_archived_parent(all: &[tasks::TaskItem], parent: &Path) -> Result<(), String>` — called by both phase-1 sites

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/core/src/services/tasks/parent/tests/mod.rs`:

```rust
/// An archived task must not become a NEW parent (GAP-90). The Parent picker
/// already excludes archived tasks from the other direction; before this,
/// Add Subtask and a direct set_task_parent bypassed that policy entirely,
/// because neither phase 1 ever inspected `status`.
#[test]
fn refuses_an_archived_parent_without_enabling_ids_or_stamping() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = write(
        &root,
        "p.md",
        "---\ntype: Task\nstatus: archived\ntitle: \"P\"\n---\n",
    );
    let before = std::fs::read_to_string(&parent).unwrap();
    let err = set_task_parent(&paths, &vault, &root.join("c.md"), Some(&parent));
    assert!(err.is_err(), "an archived parent must be refused");
    // Phase separation: the refusal precedes EVERY side effect.
    assert_eq!(std::fs::read_to_string(&parent).unwrap(), before);
    assert!(!config_for(&paths, &vault).task_id_enabled);
}

/// The SECOND entry point. `add_subtask` has its own phase 1 (it has no child
/// path to validate yet, so it cannot call validate_parent_assignment), so a
/// test of set_task_parent alone would not catch this site losing the check —
/// the exact "fixed one site, left its sibling" defect this branch keeps hitting.
#[test]
fn add_subtask_refuses_an_archived_parent() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_disabled(dir.path(), &[]);
    let root = tasks_root(&paths, &vault);
    let parent = write(
        &root,
        "p.md",
        "---\ntype: Task\nstatus: archived\ntitle: \"P\"\n---\n",
    );
    let cfg = config_for(&paths, &vault);
    let vault_path = tasks_root_for(&paths, &vault).unwrap().0;
    let err = add_subtask(
        &paths, &vault, &vault_path, &root, &cfg, &parent, &root,
        "Child", "2026-07-26", None, None, &[], None,
    );
    assert!(err.is_err(), "add_subtask must refuse an archived parent too");
    assert!(!config_for(&paths, &vault).task_id_enabled);
}

/// NON-REGRESSION for PR #77's Fix 1: the rule governs ASSIGNING a parent,
/// never HAVING one. A relationship set while the parent was active must keep
/// resolving after the parent is archived — hiding it is what invited a silent
/// overwrite through the picker in the first place.
#[test]
fn an_existing_relationship_survives_the_parent_being_archived() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["c.md"]);
    let root = tasks_root(&paths, &vault);
    let parent = write(
        &root,
        "p.md",
        "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n",
    );
    let child = root.join("c.md");
    set_task_parent(&paths, &vault, &child, Some(&parent)).unwrap();
    // Archive the parent AFTER the relationship exists.
    let archived = std::fs::read_to_string(&parent)
        .unwrap()
        .replace("status: new", "status: archived");
    std::fs::write(&parent, archived).unwrap();

    let prop = config_for(&paths, &vault).task_id_property_name().to_string();
    let all = tasks::list_tasks_structural(&root, Some(&prop)).unwrap();
    let index = tasks::parent_index(&all);
    let canon_child = std::fs::canonicalize(&child).unwrap();
    assert!(
        index.contains_key(&canon_child),
        "an archived parent must still resolve for an existing child"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test parent:: 2>&1 | tail -30`
Expected: the two refusal tests FAIL (assignment succeeds, `is_err()` is false). The non-regression test should already PASS — if it fails, stop: the baseline is different from what the spec assumed.

- [ ] **Step 3: Add the shared helper**

In `src-tauri/core/src/services/tasks/parent/mod.rs`, immediately after `reject_ambiguous_parent`:

```rust
/// Refuse an ARCHIVED task as a NEW parent (GAP-90).
///
/// A SHARED helper rather than an inline check, because the two phase-1 sites
/// (`validate_parent_assignment` for `set_task_parent`, and `add_subtask`'s own)
/// are separate functions that already share every other per-check helper
/// (`reject_ambiguous_parent`, `parent_id_unassignable`). Inlining it at one
/// site is exactly how the other silently keeps the old behavior.
///
/// This governs ASSIGNING a parent, never HAVING one: an existing on-disk pair
/// is never re-validated here, so a child whose parent was archived afterwards
/// keeps resolving and rendering it (PR #77's Fix 1 — hiding it is what invited
/// a silent overwrite through the picker).
///
/// Archived LISTS are deliberately NOT considered: that is a frontend display
/// rule (`archivedMatcher`), and blocking here would refuse a Task that is
/// itself active and plainly visible under Plan/Tags grouping (design spec,
/// Non-goals).
fn reject_archived_parent(all: &[tasks::TaskItem], parent: &Path) -> Result<(), String> {
    let archived = all
        .iter()
        .find(|t| t.path == parent)
        .is_some_and(|t| t.status == tasks::STATUS_ARCHIVED);
    if archived {
        return Err("That task is archived, so it can't be given subtasks. \
                    Unarchive it first."
            .to_string());
    }
    Ok(())
}
```

If `tasks::STATUS_ARCHIVED` does not already exist, use the literal `"archived"` and note it — do **not** invent a constant in a module this task doesn't own.

- [ ] **Step 4: Call it from both phase-1 sites**

In `validate_parent_assignment`, directly after the existing `reject_ambiguous_parent(&all, &parent)?;`:

```rust
    reject_archived_parent(&all, &parent)?;
```

In `add_subtask`'s phase 1, directly after its own `reject_ambiguous_parent(&all, &parent)?;`:

```rust
    reject_archived_parent(&all, &parent)?;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test parent:: 2>&1 | tail -20`
Expected: all three new tests PASS, no existing parent test regresses.

- [ ] **Step 6: Run the full core gates**

Run: `cd src-tauri && cargo fmt --check && cd core && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | tail -15`
Expected: clean; all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/core/src/services/tasks/parent/
git commit -m "fix(core): refuse an archived task as a new parent

The Parent picker excluded archived tasks from being newly assigned, but
nothing enforced that anywhere else: neither phase-1 site inspected status,
so Add Subtask on an archived task -- reachable through an active child's
parent chip -- assigned it exactly like any other (GAP-90).

reject_archived_parent is a SHARED helper called from both sites rather than
an inline check, mirroring reject_ambiguous_parent: add_subtask has its own
phase 1 and cannot reuse validate_parent_assignment, so an inline check would
have left the second site behind. Both calls sit in phase 1, so a refusal
leaves Task IDs off and nothing stamped.

Governs ASSIGNING a parent, never HAVING one -- an existing relationship
still resolves after its parent is archived, pinned by a non-regression test."
```

---

### Task 2: The open-subtask count ignores children in archived lists

**Files:**
- Modify: `src/utils/taskHierarchy.ts` (`openSubtaskCounts` ~line 128, `buildHierarchyInfoByVault` ~line 168)
- Modify: `src/composables/useTaskListHierarchy.ts`
- Test: `tests/task-hierarchy.test.ts`

**Interfaces:**
- Consumes: `archivedMatcher(archived: string[]) => (list: string) => boolean` from `src/utils/taskSections.ts`
- Produces:
  - `buildHierarchyInfoByVault(tasks: AggTask[], byVault: Map<string, Map<string,string>>, archivedByVault: Map<string, string[]>): Map<string, Map<string, HierarchyInfo>>`
  - `useTaskListHierarchy()` returns `{ hierarchyOf, setHierarchyTasks, setHierarchyStatus, setArchivedByVault }` where `setArchivedByVault(m: Map<string, string[]>): void`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `describe("buildHierarchyInfoByVault", ...)` block in `tests/task-hierarchy.test.ts` (or a new describe if none exists):

```ts
it("excludes an open child in an archived list from its parent's count", () => {
  // GAP-91 (count facet): archiving a list hides it from the Lists view and
  // from count_open_tasks, but the subtask badge kept counting its children —
  // the badge and the open counts beside it disagreed about the same task.
  const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const c = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", list: "Old" });
  const byVault = buildParentIndexByVault([p, c]);
  const info = buildHierarchyInfoByVault([p, c], byVault, new Map([["v1", ["Old"]]]));
  expect(info.get("v1")?.get("/v1/p.md")?.openSubtaskCount).toBe(0);
});

it("keys archived lists per vault so one vault cannot suppress another's count", () => {
  // Ids AND archived sets are vault-scoped. A flattened set would let "Old"
  // archived in v1 silently zero an identically-named LIVE list in v2.
  const p1 = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const c1 = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", list: "Old" });
  const p2 = task({ vaultId: "v2", id: "p", path: "/v2/p.md" });
  const c2 = task({ vaultId: "v2", id: "c", parentId: "p", path: "/v2/c.md", list: "Old" });
  const all = [p1, c1, p2, c2];
  const info = buildHierarchyInfoByVault(all, buildParentIndexByVault(all), new Map([["v1", ["Old"]]]));
  expect(info.get("v1")?.get("/v1/p.md")?.openSubtaskCount).toBe(0);
  expect(info.get("v2")?.get("/v2/p.md")?.openSubtaskCount).toBe(1);
});

it("matches archived list names case-insensitively, like every other surface", () => {
  // archivedMatcher is the ONE membership rule; a bespoke check here would drift.
  const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const c = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", list: "OLD" });
  const info = buildHierarchyInfoByVault([p, c], buildParentIndexByVault([p, c]), new Map([["v1", ["old"]]]));
  expect(info.get("v1")?.get("/v1/p.md")?.openSubtaskCount).toBe(0);
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/task-hierarchy.test.ts 2>&1 | tail -25`
Expected: FAIL — `buildHierarchyInfoByVault` takes 2 arguments, and counts are `1` where `0` is expected.

- [ ] **Step 3: Thread the archived map through `taskHierarchy.ts`**

Add the import at the top of `src/utils/taskHierarchy.ts`:

```ts
import { archivedMatcher } from "./taskSections";
```

Replace `openSubtaskCounts` with:

```ts
/** Pass 2 of {@link buildHierarchyInfoByVault}: `(vaultId, parentPath)` ->
 * open child count. A child is "open" when it is not done, not archived, AND
 * not filed in one of ITS OWN vault's archived lists — archiving a list hides
 * it from the Lists view and from `count_open_tasks`, so a badge that kept
 * counting its children disagreed with the open counts rendered beside it
 * (GAP-91, count facet). Keyed per vault: ids and archived sets are both
 * vault-scoped, so a flattened set would let one vault's archived list name
 * zero an identically-named LIVE list in another (the aggregate view renders
 * many vaults at once). */
function openSubtaskCounts(
  tasks: AggTask[],
  byVault: Map<string, Map<string, string>>,
  archivedByVault: Map<string, string[]>,
): Map<string, Map<string, number>> {
  const counts = new Map<string, Map<string, number>>();
  // One matcher per vault, built once — archivedMatcher allocates a Set.
  const matchers = new Map<string, (list: string) => boolean>();
  const isArchivedList = (vaultId: string, list: string) => {
    let m = matchers.get(vaultId);
    if (!m) {
      m = archivedMatcher(archivedByVault.get(vaultId) ?? []);
      matchers.set(vaultId, m);
    }
    return m(list);
  };
  for (const t of tasks) {
    if (t.done || t.status === "archived") continue;
    if (isArchivedList(t.vaultId, t.list)) continue;
    const parentPath = byVault.get(t.vaultId)?.get(t.path);
    if (!parentPath) continue;
    const bucket = vaultBucket(counts, t.vaultId);
    bucket.set(parentPath, (bucket.get(parentPath) ?? 0) + 1);
  }
  return counts;
}
```

Then change `buildHierarchyInfoByVault`'s signature and its call to pass 2:

```ts
export function buildHierarchyInfoByVault(
  tasks: AggTask[],
  byVault: Map<string, Map<string, string>>,
  archivedByVault: Map<string, string[]>,
): Map<string, Map<string, HierarchyInfo>> {
```

and inside it:

```ts
  const counts = openSubtaskCounts(tasks, byVault, archivedByVault);
```

Also extend that function's existing doc comment with:

```
 * A child in an ARCHIVED LIST is likewise excluded from the count (but still
 * resolves as somebody's parent), so the badge agrees with count_open_tasks.
```

- [ ] **Step 4: Thread it through `useTaskListHierarchy.ts`**

Replace the composable body's state + computed + return with:

```ts
export function useTaskListHierarchy() {
  const hierarchyTasks = ref<AggTask[]>([]);
  // Per-vault archived list names. A MAP, never a flat set: the aggregate view
  // renders many vaults at once and list names collide across them (GAP-91).
  const archivedByVault = ref(new Map<string, string[]>());
  const byVault = computed(() => buildParentIndexByVault(hierarchyTasks.value));
  const infoByVault = computed(() =>
    buildHierarchyInfoByVault(hierarchyTasks.value, byVault.value, archivedByVault.value),
  );

  function setArchivedByVault(m: Map<string, string[]>): void {
    archivedByVault.value = m;
  }
```

and add `setArchivedByVault` to the returned object:

```ts
  return { hierarchyOf, setHierarchyTasks, setHierarchyStatus, setArchivedByVault };
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/task-hierarchy.test.ts 2>&1 | tail -20`
Expected: PASS. Fix any other call site the typechecker flags in Step 6.

- [ ] **Step 6: Typecheck**

Run: `npm run build 2>&1 | tail -20`
Expected: clean. If `Tasks.vue` fails because `setArchivedByVault` is unused, that is expected — Task 3 wires it.

- [ ] **Step 7: Commit**

```bash
git add src/utils/taskHierarchy.ts src/composables/useTaskListHierarchy.ts tests/task-hierarchy.test.ts
git commit -m "fix(ui): drop archived-list children from the open-subtask count

The frontend hierarchy code had no notion of list archiving, so an open child
inside a since-archived list kept inflating its parent's subtask badge even
after a reload -- while the default Lists view and both the per-vault and
All-tasks open counts excluded that same child. The badge and the counts
beside it disagreed about one task (GAP-91, count facet).

The archived set travels as a per-VAULT map, never a flattened set: the
aggregate view renders many vaults at once and list names collide across
them, so one vault's archived 'Old' would otherwise zero another's live one.
Membership goes through the shared archivedMatcher so the case-insensitive
rule cannot drift from every other surface."
```

---

### Task 3: Aggregate mode loads per-vault configs and feeds the map

**Files:**
- Modify: `src/composables/useTaskLists.ts` (expose `archivedByVault`)
- Modify: `src/components/Tasks.vue` (aggregate fan-out ~line 262-276; wire the map)
- Test: `tests/task-hierarchy.test.ts` or the existing Tasks-view test file

**Interfaces:**
- Consumes: `loadVaultConfig(id: string): Promise<void>`, `vaultConfigs: Ref<Map<string, TasksConfig>>` (both already in `useTaskLists`)
- Produces: `archivedByVault: ComputedRef<Map<string, string[]>>` from `useTaskLists`

- [ ] **Step 1: Write the failing test**

Add to `tests/task-hierarchy.test.ts`:

```ts
it("loads every vault's tasks config in the aggregate fan-out", async () => {
  // Aggregate mode fanned out list_task_lists but never get_tasks_config, so
  // archivedLists was [] there BY CONSTRUCTION — the count fix would silently
  // do nothing in the All-tasks view (the 'fixed one site, left its sibling'
  // failure this branch keeps hitting).
  const configCalls: string[] = [];
  mockIPC((cmd, args) => {
    const a = args as Record<string, unknown>;
    if (cmd === "list_vaults") return [{ id: "v1", name: "V1", path: "/v1", open: false },
                                       { id: "v2", name: "V2", path: "/v2", open: false }];
    if (cmd === "get_tasks_config") { configCalls.push(a.id as string); return { listOrder: [], archivedLists: [] }; }
    if (cmd === "list_task_lists") return [];
    if (cmd === "list_tasks") return [];
    if (cmd === "count_open_tasks") return 0;
    return undefined;
  });
  // Mount Tasks.vue in aggregate mode (vaultId: null) and flush.
  // (Follow the existing mount+flushPromises helper used elsewhere in tests/.)
  await mountAggregateTasksView();
  expect(configCalls.sort()).toEqual(["v1", "v2"]);
});
```

If no `mountAggregateTasksView` helper exists, write the mount inline using the same `mount(Tasks, { props: { vaultId: null }, global: { plugins: [createPinia()] } })` + `flushPromises()` shape the sibling Tasks tests already use. **Read a neighbouring test first and copy its exact mount shape** rather than inventing one.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-hierarchy.test.ts -t "aggregate fan-out" 2>&1 | tail -20`
Expected: FAIL — `configCalls` is empty (or holds only a composer-picked vault).

- [ ] **Step 3: Expose the per-vault archived map**

In `src/composables/useTaskLists.ts`, beside the existing `archivedLists` computed:

```ts
  // Every LOADED vault's archived list names, keyed by vault id — what the
  // hierarchy count needs in the aggregate view, where the per-vault
  // `archivedLists` computed above is deliberately [] (a single archived set
  // is meaningless across vaults). Separate from that computed rather than
  // replacing it: the Lists GROUPING's aggregate behavior is a distinct,
  // deliberate simplification and is not being redefined here.
  const archivedByVault = computed(() => {
    const out = new Map<string, string[]>();
    for (const [id, cfg] of vaultConfigs.value) out.set(id, cfg.archivedLists ?? []);
    return out;
  });
```

Add `archivedByVault` to the composable's returned object.

- [ ] **Step 4: Load configs in the aggregate fan-out**

In `src/components/Tasks.vue`'s aggregate branch, beside the existing `void loadVaultLists(v.id);`:

```ts
          // The lists enumeration AND the config both ride the same fan-out,
          // each with its own catch — a failed config read must not mark the
          // vault's TASKS as failed. Without this the All-tasks view had no
          // archived data at all, so the hierarchy count's archived-list
          // exclusion silently did nothing here (GAP-91).
          void loadVaultLists(v.id);
          void loadVaultConfig(v.id);
```

- [ ] **Step 5: Wire the map into the hierarchy composable**

In `Tasks.vue`, destructure `archivedByVault` from `useTaskLists(...)` and `setArchivedByVault` from `useTaskListHierarchy()`, then keep them in sync:

```ts
// The count's archived-list exclusion reads a per-vault map; configs arrive
// asynchronously (and lazily per vault), so watch rather than set once.
watch(archivedByVault, (m) => setArchivedByVault(m), { immediate: true });
```

Place the `watch` after both destructurings. Ensure `watch` is imported from `vue`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx vitest run tests/ 2>&1 | tail -20`
Expected: the new test PASSES and no sibling Tasks test regresses.

- [ ] **Step 7: Typecheck**

Run: `npm run build 2>&1 | tail -20`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/composables/useTaskLists.ts src/components/Tasks.vue tests/
git commit -m "fix(ui): load per-vault tasks configs in the aggregate fan-out

The All-tasks view fanned out list_task_lists but never get_tasks_config, so
vaultConfigs stayed empty and archivedLists was [] there by construction --
the open-subtask count's archived-list exclusion would have silently done
nothing in exactly the view that renders every vault at once.

The config load joins the existing best-effort fan-out with its own catch, so
a failed config read cannot mark that vault's TASKS as failed, matching how
loadVaultLists already rides that loop. archivedByVault is exposed as a new
per-vault map rather than changing the existing per-vault-only archivedLists
computed, whose aggregate behavior backs the Lists grouping and is a separate
deliberate simplification."
```

---

### Task 4: The parent picker excludes archived-list tasks

**Files:**
- Modify: `src/composables/useTaskDetailTaskSet.ts` (`pickerCandidates` line 31; signature line 29)
- Modify: `src/components/TaskDetail.vue` (pass `archivedLists` into `useTaskDetailTaskSet`)
- Test: `tests/task-detail.test.ts`

**Interfaces:**
- Consumes: `archivedMatcher` from `src/utils/taskSections.ts`; `archivedLists: Ref<string[]>` already present in `TaskDetail.vue` (line 70)
- Produces: `useTaskDetailTaskSet(task: Ref<AggTask>, archivedLists: Ref<string[]>)` — **note the new second parameter**

- [ ] **Step 1: Write the failing test**

Add to `tests/task-detail.test.ts`:

```ts
it("excludes archived tasks AND archived-list tasks from parent candidates", () => {
  // GAP-91 (picker): pickerCandidates filtered on t.status only, so an ACTIVE
  // task filed in a since-archived list was still offered as a new parent —
  // the one exclusion every other archived-list consumer already enforces.
  const self = task({ path: "/v/a.md", vaultId: "v1" });
  const archivedLists = ref<string[]>(["Old"]);
  const set = useTaskDetailTaskSet(ref(self), archivedLists);
  set.allTasks.value = [
    self,
    task({ path: "/v/live.md", vaultId: "v1", list: "Live" }),
    task({ path: "/v/arch.md", vaultId: "v1", status: "archived" }),
    task({ path: "/v/inarchlist.md", vaultId: "v1", list: "Old" }),
    task({ path: "/v/casefold.md", vaultId: "v1", list: "OLD" }),
  ];
  expect(set.pickerCandidates.value.map((t) => t.path).sort())
    .toEqual(["/v/a.md", "/v/live.md"]);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-detail.test.ts -t "parent candidates" 2>&1 | tail -20`
Expected: FAIL — `/v/inarchlist.md` and `/v/casefold.md` are still present.

- [ ] **Step 3: Add the filter**

In `src/composables/useTaskDetailTaskSet.ts`, add the import:

```ts
import { archivedMatcher } from "../utils/taskSections";
```

Change the signature and `pickerCandidates`:

```ts
export function useTaskDetailTaskSet(task: Ref<AggTask>, archivedLists: Ref<string[]>) {
  const allTasks = ref<AggTask[]>([]);
  // A candidate must be assignable AS a new parent: not archived itself, and
  // not filed in an archived list — the same two-part test useTaskDisplay
  // applies, through the same shared archivedMatcher so the case-insensitive
  // rule cannot drift (GAP-91). `allTasks` itself stays archived-INCLUSIVE:
  // it decides whether a relationship EXISTS, and an archived task can still
  // be somebody's parent (PR #77's Fix 1). This narrows only the OPTIONS.
  const pickerCandidates = computed(() => {
    const isArchivedList = archivedMatcher(archivedLists.value);
    return allTasks.value.filter((t) => t.status !== "archived" && !isArchivedList(t.list));
  });
```

Extend the file's header doc comment so the archived-LIST half is recorded alongside the archived-status half already documented there.

- [ ] **Step 4: Pass `archivedLists` from `TaskDetail.vue`**

Find the `useTaskDetailTaskSet(...)` call in `src/components/TaskDetail.vue` and pass the existing ref as the second argument:

```ts
useTaskDetailTaskSet(toRef(props, "task"), archivedLists)
```

Keep the existing first argument exactly as it is — only add the second. `archivedLists` is already declared at line 70 and populated by `loadLists()`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/task-detail.test.ts 2>&1 | tail -20`
Expected: PASS, no sibling regressions.

- [ ] **Step 6: Typecheck**

Run: `npm run build 2>&1 | tail -20`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/composables/useTaskDetailTaskSet.ts src/components/TaskDetail.vue tests/task-detail.test.ts
git commit -m "fix(ui): exclude archived-list tasks from the parent picker

pickerCandidates filtered on the task's own status only, so an ACTIVE task
filed in a since-archived list was still offered as an assignable new parent
-- the one exclusion every other archived-list consumer (the Lists grouping,
the composer/editor pickers, count_open_tasks) already enforces (GAP-91).

The filter goes through the shared archivedMatcher, so the case-insensitive
membership rule cannot drift from those surfaces. allTasks itself stays
archived-INCLUSIVE: it decides whether a relationship EXISTS, and an archived
task can still be somebody's parent -- this narrows only the OPTIONS."
```

---

### Task 5: Add Subtask — disable on an archived Task, disclose an archived list

**Files:**
- Modify: `src/components/TaskSubtasks.vue` (Add-subtask input)
- Modify: `src/components/TaskDetail.vue` (`onAddSubtask` ~line 182-215; pass the new prop)
- Test: `tests/task-detail.test.ts`

**Interfaces:**
- Consumes: `notifications.notify(kind, message, opts)` (already imported in `TaskDetail.vue`); `archivedMatcher`; `archivedLists` ref
- Produces: `TaskSubtasks.vue` gains prop `disabledReason?: string | null` — when non-null the Add-subtask input and its submit are disabled and the reason renders

- [ ] **Step 1: Write the failing tests**

Add to `tests/task-detail.test.ts`:

```ts
it("disables Add subtask on an archived task", () => {
  // GAP-90's UI half: core refuses an archived parent, but an unguarded input
  // would meet the user with an error toast instead of an affordance. The
  // archived task's own detail IS reachable — via an active child's parent chip.
  const wrapper = mountDetail(task({ status: "archived" }));
  const input = wrapper.find('[data-testid="add-subtask-input"]');
  expect(input.attributes("disabled")).toBeDefined();
});

it("keeps Add subtask enabled on an active task", () => {
  const wrapper = mountDetail(task({ status: "new" }));
  expect(wrapper.find('[data-testid="add-subtask-input"]').attributes("disabled"))
    .toBeUndefined();
});

it("discloses when a new subtask lands in an archived list", async () => {
  // GAP-92: the child correctly INHERITS the parent's list, but when that list
  // is archived the task is hidden from the Lists view and from the open-task
  // badge the instant it is created — silently, before this.
  const notifications = useNotificationsStore();
  const spy = vi.spyOn(notifications, "notify");
  const wrapper = mountDetail(task({ status: "new", list: "Old" }), { archivedLists: ["Old"] });
  await addSubtask(wrapper, "Child");
  expect(spy.mock.calls.some(([, msg]) => String(msg).toLowerCase().includes("archived")))
    .toBe(true);
});

it("raises no archived disclosure for a live list", async () => {
  const notifications = useNotificationsStore();
  const spy = vi.spyOn(notifications, "notify");
  const wrapper = mountDetail(task({ status: "new", list: "Live" }), { archivedLists: ["Old"] });
  await addSubtask(wrapper, "Child");
  expect(spy.mock.calls.some(([, msg]) => String(msg).toLowerCase().includes("archived")))
    .toBe(false);
});
```

**Read the existing `tests/task-detail.test.ts` mount helper first** and reuse it — write `mountDetail`/`addSubtask` to match the file's established shape rather than inventing new helpers. If the Add-subtask input has no `data-testid` yet, add `data-testid="add-subtask-input"` in Step 3.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/task-detail.test.ts 2>&1 | tail -25`
Expected: FAIL — the input is never disabled; no archived disclosure fires.

- [ ] **Step 3: Add the disabled affordance to `TaskSubtasks.vue`**

Add to `defineProps`:

```ts
  /** Non-null when the Add-subtask input must be inert, with the reason shown.
   * A HINT ONLY — core's reject_archived_parent is the authority (it refuses
   * the write regardless of what this component renders). Without it the user
   * meets an error toast instead of an affordance. */
  disabledReason?: string | null;
```

Bind it on the input and its submit control:

```
:disabled="Boolean(disabledReason) || busy"
data-testid="add-subtask-input"
```

and render the reason beneath the input when present:

```html
<p v-if="disabledReason" class="text-micro text-fg-subtle">{{ disabledReason }}</p>
```

- [ ] **Step 4: Pass the reason and add the disclosure in `TaskDetail.vue`**

Add the computed near the other archived state:

```ts
// GAP-90's UI hint. Core refuses the write either way; this makes the refusal
// visible as an affordance instead of an error toast on submit.
const addSubtaskDisabledReason = computed(() =>
  props.task.status === "archived"
    ? "This task is archived. Unarchive it to add subtasks."
    : null,
);
```

Bind it: `<TaskSubtasks ... :disabled-reason="addSubtaskDisabledReason" />`

In `onAddSubtask`, after `subtasksRef.value?.reset();` inside the `try`:

```ts
    // GAP-92: the child correctly INHERITS the parent's list, keeping it
    // beside its parent — but an archived list is hidden from the Lists view
    // and from count_open_tasks the instant the task is created. The task is
    // not lost (it renders here and under Plan/Tags grouping); the defect was
    // the silence, so disclose rather than reroute it away from its parent.
    if (archivedMatcher(archivedLists.value)(props.task.list)) {
      notifications.notify(
        "info",
        `Added to "${props.task.list}", an archived list hidden from the Lists view.`,
        {},
      );
    }
```

Import `archivedMatcher` from `../utils/taskSections` if not already imported. If `"info"` is not a valid notification kind in this store, use the kind the store actually defines — check `src/stores/notifications.ts` first.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/task-detail.test.ts 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Full frontend gates**

Run: `npm test 2>&1 | tail -15 && npm run lint && npm run build 2>&1 | tail -10`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/components/TaskSubtasks.vue src/components/TaskDetail.vue tests/task-detail.test.ts
git commit -m "fix(ui): gate Add subtask on an archived task and disclose an archived list

Two halves of the same blind spot. An archived task's own detail is reachable
through an active child's parent chip, and its Add-subtask input was gated
only on the write lock -- so it offered an assignment core now refuses
(GAP-90). The input is disabled with a stated reason: a HINT only, since core
remains the authority.

Separately, a new subtask correctly inherits its parent's list, but when that
list is archived the child is hidden from the Lists view and from
count_open_tasks the instant it is created (GAP-92). It is not lost -- it
renders in this section and under Plan/Tags grouping -- so the fix is
disclosure, not rerouting the child away from its parent and breaking the
inheritance design to solve a visibility problem."
```

---

### Task 6: Retire the gaps and record the rules

**Files:**
- Modify: `docs/Gaps.md` (remove GAP-90 ~line 1063, GAP-91 ~line 1104, GAP-92 ~line 1148)
- Modify: `AGENTS.md` (tasks-domain Hierarchy section)

- [ ] **Step 1: Remove the three closed entries**

Delete the GAP-90, GAP-91 (including its "Third facet" paragraph and the "fix all three facets together" note) and GAP-92 sections from `docs/Gaps.md`. Leave every other entry byte-for-byte untouched.

- [ ] **Step 2: Record both rules in AGENTS.md**

In the tasks-domain **Hierarchy** section, add a bullet:

```markdown
  - **Archiving is respected on both sides, with one authority each.**
    *Write side (core):* `reject_archived_parent` — a shared phase-1 helper
    called from BOTH `validate_parent_assignment` and `add_subtask`'s own
    phase 1 (they are separate functions sharing per-check helpers, not one
    validator) — refuses an ARCHIVED Task as a NEW parent. It governs
    *assigning* a parent, never *having* one: an existing pair is never
    re-validated, so a child whose parent was archived later keeps resolving
    it. Core deliberately does NOT know about archived LISTS — that would
    refuse a Task that is itself active and plainly visible under Plan/Tags.
    *Display side (frontend):* a Task that is archived OR filed in an archived
    list is neither offered as a new parent (`pickerCandidates`) nor counted in
    a parent's open-subtask badge (`openSubtaskCounts`), both through the
    shared `archivedMatcher`. The archived set is keyed **per vault** — the
    aggregate view renders many vaults at once and list names collide across
    them — which is why `Tasks.vue`'s aggregate fan-out loads each vault's
    config alongside its lists. A new subtask still INHERITS its parent's
    list; when that list is archived the create path discloses it. The
    disabled Add-subtask input is a UI hint; core is the authority.
```

- [ ] **Step 3: Verify no dangling references**

Run: `grep -n "GAP-90\|GAP-91\|GAP-92" docs/ AGENTS.md -r`
Expected: no hits outside the design spec's own "Gaps closed" line.

- [ ] **Step 4: Commit**

```bash
git add docs/Gaps.md AGENTS.md
git commit -m "docs: retire GAP-90/91/92 and record the archiving rules

The archived-hierarchy family is closed, so the entries leave the backlog per
that file's own convention. AGENTS.md gains the two rules and, more
importantly, the two constraints a future change must respect: core governs
ASSIGNING a parent and never HAVING one (an inherited archived parent must
keep resolving), and core deliberately does not know about archived LISTS."
```

---

### Task 7: Full verification

- [ ] **Step 1: Frontend gates in CI order**

Run: `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage 2>&1 | tail -25`
Expected: all pass. `check:quality` must run with no `coverage/` dir present, which is why `test:coverage` is last. If the LOC or quality baseline improved, re-run the gate with `--update` and commit the baseline in the same PR.

- [ ] **Step 2: Rust gates**

Run: `cd src-tauri && cargo fmt --check && cd core && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | tail -15`
Expected: clean.

- [ ] **Step 3: Production build**

Run: `npm run build 2>&1 | tail -10`
Expected: clean.

- [ ] **Step 4: Commit any baseline updates, then push**

```bash
git add -A
git commit -m "chore: update LOC/quality baselines for the archived-hierarchy increment" || true
git push -u origin claude/task-management-ux-05tpp0
```

---

## Self-Review

**Spec coverage:**

| Spec requirement | Task |
| --- | --- |
| Rule A — core refuses archived parent, shared helper, both phase-1 sites | Task 1 |
| Rule A — phase-1 ordering (IDs off, nothing stamped) | Task 1, Steps 1/5 |
| Rule A — inheritance non-regression | Task 1, Step 1 (third test) |
| Rule A — frontend disabled input as hint | Task 5 |
| Rule B — `pickerCandidates` excludes archived + archived-list | Task 4 |
| Rule B — `openSubtaskCounts` excludes archived-list children | Task 2 |
| Rule B — per-vault keying | Task 2 (test 2), Task 3 |
| Rule B — aggregate loads configs | Task 3 |
| Rule C — archived-list disclosure | Task 5 |
| Gaps retired, AGENTS.md updated | Task 6 |

No spec requirement is unassigned.

**Placeholder scan:** No TBD/TODO. Two steps deliberately instruct reading a neighbouring test first (Task 3 Step 1, Task 5 Step 1) rather than inventing a mount helper — that is a concrete instruction with a named fallback shape, not a placeholder. Task 1 Step 3 and Task 5 Step 4 each name a specific fallback if a constant/kind does not exist.

**Type consistency:** `buildHierarchyInfoByVault(tasks, byVault, archivedByVault)` is used with three arguments in Task 2's tests, its implementation, and `useTaskListHierarchy`. `setArchivedByVault` is produced in Task 2 and consumed in Task 3 under the same name. `useTaskDetailTaskSet(task, archivedLists)` is defined and called with two arguments in Task 4. `archivedByVault` is `Map<string, string[]>` at every site. `disabledReason` matches between `TaskSubtasks.vue`'s prop and `TaskDetail.vue`'s binding (`:disabled-reason`).
