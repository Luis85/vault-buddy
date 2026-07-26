import { computed, ref } from "vue";

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
 */
export function useTaskListHierarchy() {
  const hierarchyTasks = ref<AggTask[]>([]);
  const byVault = computed(() => buildParentIndexByVault(hierarchyTasks.value));
  const infoByVault = computed(() => buildHierarchyInfoByVault(hierarchyTasks.value, byVault.value));

  function hierarchyOf(task: AggTask): HierarchyInfo {
    return infoByVault.value.get(task.vaultId)?.get(task.path) ?? NO_HIERARCHY;
  }

  function setHierarchyTasks(items: AggTask[]): AggTask[] {
    hierarchyTasks.value = items;
    return items.filter((t) => t.status !== "archived");
  }

  return { hierarchyOf, setHierarchyTasks };
}
