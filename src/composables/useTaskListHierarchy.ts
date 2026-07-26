import { computed, type Ref, ref } from "vue";

import type { AggTask } from "../types";
import { buildHierarchyInfoByVault, buildParentIndexByVault, type HierarchyInfo } from "../utils/taskHierarchy";

const NO_HIERARCHY: HierarchyInfo = { parent: null, openSubtaskCount: 0 };

/**
 * The main task list's per-row hierarchy derivation (Task 10): an open-
 * subtask count badge and a parent chip, reading the identical rule
 * useTaskHierarchy applies for Task Detail (src/utils/taskHierarchy.ts) so
 * the two surfaces can never disagree about the same relationship. Split into
 * its own composable — the pattern every sibling Tasks.vue concern already
 * follows — so the grandfathered LOC hotspot gains only a one-line call site.
 *
 * SELF-CONTAINED task set (Fix 1, subtasks vault-UX-polish increment): this
 * owns its OWN `hierarchyTasks`, populated via `setHierarchyTasks` at every
 * point Tasks.vue (re)loads its displayed `tasks` — an archived parent must
 * still resolve (the identical blind spot Task Detail's own fix closed: a
 * resolver built only from the archived-EXCLUDED displayed set can never see
 * that edge), while `tasks.value` itself keeps its exact historical
 * archived-EXCLUDED meaning everywhere else in Tasks.vue. `setHierarchyTasks`
 * both stores the superset here AND hands the caller back the visible
 * (non-archived) subset, so a load site stays a single assignment instead of
 * growing a second statement in the LOC-capped container.
 *
 * `hierarchyTasks` may span every vault at once (the aggregate view), so the
 * per-vault index and info map are each built ONCE in a computed and reused
 * across every row lookup — O(1) per row instead of an O(n) find+filter
 * (Task 12's perf fix: a checkbox toggle or filter keystroke on a thousand-
 * task vault was redoing Θ(n²) comparisons per render).
 *
 * `archivedByVault` (a REF the caller owns, not local state) supplies the
 * open-subtask count's archived-list exclusion (GAP-91). A per-vault MAP, never
 * one flat set: the aggregate view renders many vaults at once and list names
 * collide across them. Taken reactively rather than through a setter because
 * configs load lazily per vault — the count must re-derive as each lands, and
 * an imperative sync in the container would be one more thing to forget.
 *
 * Incremental status updates (`setHierarchyStatus`, below): a toggle mutates
 * `task.done`/`task.status` IN PLACE on the object `tasks.value` and
 * `hierarchyTasks` both hold, so it is visible here for free. An archive is
 * different — `useTaskActions.archive` optimistically SPLICES the row out of
 * the DISPLAYED array rather than editing the object, which touches neither
 * this array's contents nor the removed task's fields. Left uncalled, this
 * superset would keep reporting the pre-archive status forever, and
 * `buildHierarchyInfoByVault`'s open-subtask count would keep counting an
 * archived child as open until the next full reload.
 */
export function useTaskListHierarchy(archivedByVault: Ref<Map<string, string[]>>) {
  const hierarchyTasks = ref<AggTask[]>([]);
  const byVault = computed(() => buildParentIndexByVault(hierarchyTasks.value));
  const infoByVault = computed(() =>
    buildHierarchyInfoByVault(hierarchyTasks.value, byVault.value, archivedByVault.value),
  );

  function hierarchyOf(task: AggTask): HierarchyInfo {
    return infoByVault.value.get(task.vaultId)?.get(task.path) ?? NO_HIERARCHY;
  }

  function setHierarchyTasks(items: AggTask[]): AggTask[] {
    hierarchyTasks.value = items;
    return items.filter((t) => t.status !== "archived");
  }

  // Keyed by (vaultId, path) rather than an object reference the caller
  // happens to hold — the superset must stay correct even if a future load
  // path stops sharing task objects with the displayed `tasks` array. A
  // missing match is a silent no-op (e.g. a stale path after an unrelated
  // reload already replaced the superset) rather than an error, matching
  // this file's other defensive lookups.
  function setHierarchyStatus(vaultId: string, path: string, status: string): void {
    const found = hierarchyTasks.value.find((t) => t.vaultId === vaultId && t.path === path);
    if (found) found.status = status;
  }

  return { hierarchyOf, setHierarchyTasks, setHierarchyStatus };
}
