import type { AggTask } from "../types";
import { archivedMatcher } from "./taskSections";

// Mirrors core::tasks::hierarchy exactly (src-tauri/core/src/tasks/hierarchy.rs)
// so the frontend and core can never disagree about the same vault's Parent
// row / subtasks (Codex P2, PR #77): this is the ONE place the resolution
// rule lives — Task 10's main-list badge/chip consumes it too, and a second
// implementation would drift.

/** Shown once per vault, the moment a write turns Task IDs on to support a
 * parent/child relationship — the flag can't be inferred from a returned id
 * (an already-enabled vault with an unstamped parent returns the identical
 * shape), so without this disclosure the user finds Task IDs enabled — and
 * locked (Task 6) — with no warning. Shared by useTaskHierarchy's setParent
 * and Task 9's Add subtask. */
export const TASK_IDS_ENABLED_MESSAGE =
  "Task IDs were turned on for this vault so subtasks can reference their parent.";

// Same rule as core::tasks::ambiguous_ids, per vault: an id carried by more
// than one task identifies nothing, so it resolves no relationship.
function ambiguousIds(tasks: AggTask[], vaultId: string): Set<string> {
  const seen = new Map<string, number>();
  for (const t of tasks) {
    if (t.vaultId !== vaultId || !t.id) continue;
    seen.set(t.id, (seen.get(t.id) ?? 0) + 1);
  }
  return new Set([...seen].filter(([, n]) => n > 1).map(([id]) => id));
}

// Mirrors core::tasks::hierarchy::drop_cyclic_edges. A pre-existing on-disk
// cycle must render both rows top-level, not as each other's parent.
//
// TWO PHASES — collect every cyclic key against the UNCHANGED map, then
// delete. Deleting inside the walk breaks the very paths still being
// inspected: for A->B->A, removing A's edge while processing A leaves B's
// later walk unable to reach A, so B->A survives and one side of the loop
// still renders (Codex P2, PR #77). Rust's borrow checker forces the
// two-phase shape there; ported deliberately, not by accident, here.
function dropCyclicEdges(edges: Map<string, string>): void {
  const cyclic: string[] = [];
  for (const start of edges.keys()) {
    const seen = new Set<string>();
    let cur: string | undefined = start;
    while (cur !== undefined) {
      const next: string | undefined = edges.get(cur);
      if (next === undefined) break;
      if (next === start) {
        cyclic.push(start);
        break;
      }
      if (seen.has(next)) break; // a different cycle upstream, not ours
      seen.add(next);
      cur = next;
    }
  }
  for (const key of cyclic) edges.delete(key);
}

/** Child PATH -> parent PATH, for ONE vault's tasks, edges resolved THROUGH
 * ids (never by path/title), with every edge touching a cycle dropped — the
 * DISPLAY index, mirroring core::tasks::hierarchy::parent_index exactly
 * (ambiguous ids first, then cycles). Every lookup is scoped to `vaultId`
 * first: ids are only unique within a vault, so an aggregate caller must
 * never link across vaults by accident. */
export function buildParentIndex(tasks: AggTask[], vaultId: string): Map<string, string> {
  const ambiguous = ambiguousIds(tasks, vaultId);
  // id -> path, for the UNambiguous ids only (scoped to this vault).
  const byId = new Map<string, string>();
  for (const t of tasks) {
    if (t.vaultId !== vaultId || !t.id || ambiguous.has(t.id)) continue;
    byId.set(t.id, t.path);
  }
  const edges = new Map<string, string>();
  for (const t of tasks) {
    if (t.vaultId !== vaultId || !t.parentId) continue;
    // An unresolvable or ambiguous parent-id yields no edge: the child is an
    // orphan, never a guess.
    const parentPath = byId.get(t.parentId);
    if (parentPath) edges.set(t.path, parentPath);
  }
  dropCyclicEdges(edges);
  return edges;
}

/** One buildParentIndex per DISTINCT vault represented in `tasks` — Task 10's
 * main-list badge/chip, unlike useTaskHierarchy, can show every vault's rows
 * at once (the aggregate view), and ids are unique only within a vault, so
 * each vault needs its OWN index computed under its own scope. */
export function buildParentIndexByVault(tasks: AggTask[]): Map<string, Map<string, string>> {
  const byVault = new Map<string, Map<string, string>>();
  for (const vaultId of new Set(tasks.map((t) => t.vaultId))) {
    byVault.set(vaultId, buildParentIndex(tasks, vaultId));
  }
  return byVault;
}

/** One task's resolved parent row (or null) and open (not-done, not-archived)
 * subtask count — the list's per-row derivation (Task 10). */
export interface HierarchyInfo {
  parent: AggTask | null;
  openSubtaskCount: number;
}

/** The `(vaultId, path)` bucket inside a nested vault map, creating it on
 * first use — the one repeated shape all three of buildHierarchyInfoByVault's
 * passes share, factored out so each pass reads as its own rule instead of
 * three copies of the same get-or-create dance (also what keeps each pass's
 * own cyclomatic complexity low). */
function vaultBucket<V>(outer: Map<string, Map<string, V>>, vaultId: string): Map<string, V> {
  let inner = outer.get(vaultId);
  if (!inner) {
    inner = new Map();
    outer.set(vaultId, inner);
  }
  return inner;
}

/** Pass 1 of {@link buildHierarchyInfoByVault}: `(vaultId, path)` -> the task
 * itself, for resolving a PARENT's own row — unfiltered by status, since an
 * archived task must still resolve (Fix 1). */
function taskByPath(tasks: AggTask[]): Map<string, Map<string, AggTask>> {
  const byPath = new Map<string, Map<string, AggTask>>();
  for (const t of tasks) vaultBucket(byPath, t.vaultId).set(t.path, t);
  return byPath;
}

/** Pass 2 of {@link buildHierarchyInfoByVault}: `(vaultId, parentPath)` ->
 * open child count. "Open" means not done, not archived, AND not filed in one
 * of its OWN vault's archived lists: archiving a list hides it from the Lists
 * view and from `count_open_tasks`, so a badge that kept counting its children
 * disagreed with the open counts rendered right beside it, even after a full
 * reload (GAP-91, count facet).
 *
 * `archivedByVault` is keyed PER VAULT, never flattened: ids and archived sets
 * are both vault-scoped, and the aggregate ("All tasks") view renders many
 * vaults at once — one flat set would let a list archived in vault A silently
 * zero an identically-named LIVE list in vault B.
 *
 * Only the COUNT is scoped this way. An archived-list task still resolves AS a
 * parent, exactly like an archived one: hiding a parent from resolution is the
 * bug PR #77's Fix 1 closed (an invisible parent invited a silent overwrite
 * through the picker). */
function openSubtaskCounts(
  tasks: AggTask[],
  byVault: Map<string, Map<string, string>>,
  archivedByVault: Map<string, string[]>,
): Map<string, Map<string, number>> {
  const counts = new Map<string, Map<string, number>>();
  // One matcher per vault, built lazily and memoized — archivedMatcher
  // allocates a Set, which must not happen once per task in a large vault.
  const matchers = new Map<string, (list: string) => boolean>();
  const inArchivedList = (vaultId: string, list: string) => {
    let match = matchers.get(vaultId);
    if (!match) {
      match = archivedMatcher(archivedByVault.get(vaultId) ?? []);
      matchers.set(vaultId, match);
    }
    return match(list);
  };
  for (const t of tasks) {
    if (t.done || t.status === "archived") continue;
    if (inArchivedList(t.vaultId, t.list)) continue;
    const parentPath = byVault.get(t.vaultId)?.get(t.path);
    if (!parentPath) continue;
    const bucket = vaultBucket(counts, t.vaultId);
    bucket.set(parentPath, (bucket.get(parentPath) ?? 0) + 1);
  }
  return counts;
}

/** Every task's {@link HierarchyInfo}, keyed by vault then path, built in
 * ONE pass over `tasks` instead of a per-task lookup — the list used to
 * re-derive each row from scratch (an `allTasks.find` + `allTasks.filter`,
 * each O(n)) on every render, so a thousand-task vault redid roughly
 * `n rows * O(n)` comparisons per keystroke or checkbox toggle (Task 12's
 * perf fix). Reads the SAME per-vault index useTaskHierarchy builds for Task
 * Detail so an ambiguous id, a cycle, or a cross-vault id collision renders
 * as unresolved in BOTH places (Codex P2, PR #77) — this is a memoization of
 * that one shared rule, never a second resolution rule. Direct children
 * only, matching useTaskHierarchy's children/progress (no grandchildren).
 *
 * A task missing from the returned map (Task IDs off is the common case —
 * every vault's index is then empty) resolves to no entry; callers supply
 * the trivial `{parent: null, openSubtaskCount: 0}` default themselves
 * (`useTaskListHierarchy.hierarchyOf`) rather than this function padding
 * every path with a value nothing asked for.
 *
 * An ARCHIVED task is excluded from every open-subtask COUNT (archiving
 * removes it from view everywhere else; it must not keep inflating its
 * former parent's badge) but — like `buildParentIndex`'s own edges — still
 * resolves as somebody's PARENT: hiding an archived parent from resolution
 * is exactly the bug Fix 1 closed (an active child's parent going invisible
 * invited a silent overwrite via the picker). `taskByPath`/`openSubtaskCounts`
 * apply that split once, rather than at every call site.
 *
 * A task in an ARCHIVED LIST follows the identical one-directional rule
 * (GAP-91): excluded from the count so the badge agrees with the Lists view
 * and `count_open_tasks`, but still resolvable as a parent. `archivedByVault`
 * is REQUIRED rather than defaulted — there is exactly one production caller,
 * and a silent default would let a future one forget the map and quietly
 * over-report every count, which is the defect this parameter exists to fix.
 */
export function buildHierarchyInfoByVault(
  tasks: AggTask[],
  byVault: Map<string, Map<string, string>>,
  archivedByVault: Map<string, string[]>,
): Map<string, Map<string, HierarchyInfo>> {
  const out = new Map<string, Map<string, HierarchyInfo>>();
  // Early-out: no vault has ANY edge (Task IDs off is the default), so every
  // answer is trivially null/0 — skip the passes below rather than walking
  // `tasks` for nothing on the vault's most common configuration.
  const hasAnyEdge = [...byVault.values()].some((index) => index.size > 0);
  if (!hasAnyEdge) return out;

  const byPath = taskByPath(tasks);
  const counts = openSubtaskCounts(tasks, byVault, archivedByVault);
  for (const t of tasks) {
    const parentPath = byVault.get(t.vaultId)?.get(t.path);
    const parent = parentPath ? (byPath.get(t.vaultId)?.get(parentPath) ?? null) : null;
    const openSubtaskCount = counts.get(t.vaultId)?.get(t.path) ?? 0;
    vaultBucket(out, t.vaultId).set(t.path, { parent, openSubtaskCount });
  }
  return out;
}

/** All paths that are `path` itself or a transitive child of it in `index` (a
 * buildParentIndex child->parent map) — the set that would create a cycle if
 * `path` picked one of them as ITS OWN parent. A UI hint only, computed from
 * the possibly-incomplete frontend index (e.g. with Task IDs off the index is
 * empty and nothing is pre-disabled — correctly, since no parent links can
 * exist yet): core re-validates on write and remains the authority. */
export function descendantPaths(index: Map<string, string>, path: string): Set<string> {
  const childrenOf = new Map<string, string[]>();
  for (const [child, parentPath] of index) {
    const siblings = childrenOf.get(parentPath);
    if (siblings) siblings.push(child);
    else childrenOf.set(parentPath, [child]);
  }
  const out = new Set<string>([path]);
  const stack = [path];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    for (const child of childrenOf.get(cur) ?? []) {
      if (!out.has(child)) {
        out.add(child);
        stack.push(child);
      }
    }
  }
  return out;
}
