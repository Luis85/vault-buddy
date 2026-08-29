import { invoke } from "@tauri-apps/api/core";
import { computed, type Ref, ref } from "vue";

import { logWarning } from "../logging";
import type { AggTask, TaskItem } from "../types";
import { buildParentIndex, descendantPaths } from "../utils/taskHierarchy";
import { archivedMatcher } from "../utils/taskSections";

/**
 * Loads the vault's task set that useTaskHierarchy resolves the Parent row
 * against, and the frontend cycle-prevention hint the picker disables
 * options with. Split out of TaskDetail.vue (which otherwise owns none of
 * this — Tasks.vue's own loaded list is unrelated: that view is UNMOUNTED
 * while this one shows, so there is nothing to share) purely to keep
 * TaskDetail's own script small and focused; this file has no reason to be
 * reused elsewhere yet.
 *
 * Loaded archived-INCLUSIVE (Fix 1, subtasks vault-UX-polish increment): an
 * active child's parent can be archived later, and `list_tasks`'s plain view
 * drops `status: archived` rows — a resolver built from that view could
 * never see the parent, and a user shown "No parent" could pick a new one,
 * silently REPLACING the real relationship. `allTasks` decides whether a
 * relationship exists, so it belongs on the archived-inclusive read, exactly
 * like core's own hierarchy guard (`list_tasks_structural`). `pickerCandidates`
 * is the deliberately narrower, archived-EXCLUDED view of the SAME set for
 * `TaskParentPicker`'s options — you should not be able to newly ASSIGN an
 * archived task as a parent, only inherit one that was already set before it
 * was archived.
 *
 * "Archived" there means the task's own `status` OR its LIST being archived
 * (GAP-91): list archiving is meant to hide a whole list and everything filed
 * under it from every picker, and this was the one archived-list consumer that
 * never applied the rule. `archivedLists` is a REF, not a snapshot — TaskDetail
 * loads it asynchronously, so a value captured at call time would leave the
 * picker offering a list archived moments later.
 */
export function useTaskDetailTaskSet(task: Ref<AggTask>, archivedLists: Ref<string[]>) {
  const allTasks = ref<AggTask[]>([]);
  // Narrows only the OPTIONS. `allTasks` stays archived-INCLUSIVE above: it
  // decides whether a relationship EXISTS, and an archived (or archived-list)
  // task can still be somebody's parent.
  const pickerCandidates = computed(() => {
    const inArchivedList = archivedMatcher(archivedLists.value);
    return allTasks.value.filter((t) => t.status !== "archived" && !inArchivedList(t.list));
  });

  async function reload(): Promise<void> {
    try {
      const items =
        (await invoke<TaskItem[]>("list_tasks", {
          id: task.value.vaultId,
          includeArchived: true,
        })) ?? [];
      // The row for THIS task keeps the exact `task.value` object identity
      // rather than a freshly mapped clone: useTaskHierarchy mutates
      // `task.value` (parentId/parentLink/id) directly, and buildParentIndex
      // reads those fields back off THIS array — a clone with the same path
      // would silently desync, leaving the just-written relationship
      // invisible until the next reload even though the write succeeded.
      // Identity alone isn't the whole fix, though: THIS reload only ever
      // runs (mount aside) right after a write that bootstrapped Task IDs for
      // the whole vault, precisely because `task.value` is stale — it still
      // carries its pre-write id/parentId/parentLink. Keeping the old object
      // AS-IS would preserve identity while re-freezing the very staleness
      // this reload exists to clear. So the fresh DTO's fields are adopted
      // onto `task.value` IN PLACE (mutate, never replace — Object.assign
      // leaves any property absent from its source untouched, so vaultId/
      // vaultName — AggTask-only, absent on the TaskItem DTO — survive).
      allTasks.value = items.map((t) => {
        if (t.path !== task.value.path) return { ...t, vaultId: task.value.vaultId, vaultName: "" };
        Object.assign(task.value, t);
        return task.value;
      });
    } catch (e) {
      logWarning(`task detail: could not load the task set: ${String(e)}`);
    }
  }

  // A UI HINT ONLY: the picker pre-disables self + its own descendants
  // (picking either would create a cycle). With Task IDs off — the default —
  // the index is empty and nothing is pre-disabled: correctly, since no
  // parent-id link can exist yet for anything to walk. Core re-validates on
  // write regardless (validate_parent_assignment) and remains the authority.
  const invalidParentPaths = computed(() =>
    Array.from(descendantPaths(buildParentIndex(allTasks.value, task.value.vaultId), task.value.path)),
  );

  return { allTasks, pickerCandidates, reload, invalidParentPaths };
}
