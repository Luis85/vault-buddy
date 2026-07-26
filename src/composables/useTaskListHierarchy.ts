import { computed, type Ref } from "vue";

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
 * `tasks` may span every vault at once (the aggregate view), so the per-vault
 * index AND the per-task info map are each built ONCE in a computed and
 * reused across every row lookup — O(1) per row instead of an O(n)
 * find+filter (Task 12's perf fix: a checkbox toggle or filter keystroke on a
 * thousand-task vault was redoing Θ(n²) comparisons per render).
 */
export function useTaskListHierarchy(tasks: Ref<AggTask[]>) {
  const byVault = computed(() => buildParentIndexByVault(tasks.value));
  const infoByVault = computed(() => buildHierarchyInfoByVault(tasks.value, byVault.value));

  function hierarchyOf(task: AggTask): HierarchyInfo {
    return infoByVault.value.get(task.vaultId)?.get(task.path) ?? NO_HIERARCHY;
  }

  return { hierarchyOf };
}
