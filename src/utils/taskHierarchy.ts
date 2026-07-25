import type { AggTask } from "../types";

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
