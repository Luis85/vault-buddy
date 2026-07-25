import { computed, type Ref } from "vue";

import type { AggTask } from "../types";
import { buildParentIndexByVault, taskHierarchyInfo } from "../utils/taskHierarchy";

/**
 * The main task list's per-row hierarchy derivation (Task 10): an open-
 * subtask count badge and a parent chip, reading the identical rule
 * useTaskHierarchy applies for Task Detail (src/utils/taskHierarchy.ts) so
 * the two surfaces can never disagree about the same relationship. Split into
 * its own composable — the pattern every sibling Tasks.vue concern already
 * follows — so the grandfathered LOC hotspot gains only a one-line call site.
 *
 * `tasks` may span every vault at once (the aggregate view), so the per-vault
 * index is built once in a computed and reused across every row lookup.
 */
export function useTaskListHierarchy(tasks: Ref<AggTask[]>) {
  const byVault = computed(() => buildParentIndexByVault(tasks.value));
  function hierarchyOf(task: AggTask) {
    return taskHierarchyInfo(task, tasks.value, byVault.value);
  }
  return { hierarchyOf };
}
