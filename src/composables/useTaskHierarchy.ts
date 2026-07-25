import { invoke } from "@tauri-apps/api/core";
import { computed, type Ref, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { AggTask, TaskWriteResult } from "../types";
import { buildParentIndex, TASK_IDS_ENABLED_MESSAGE } from "../utils/taskHierarchy";
import { reflectStampedId } from "../utils/taskMutations";

/** Open/total over a task's direct children (subtasks only — this does not
 * recurse into grandchildren). Not exported: nothing outside this module
 * needs the shape by name yet (a future consumer can destructure
 * `progress.value.done`/`.total` structurally, same as the composable tests
 * do), and an unused export is a dead-code hit (fallow). */
interface TaskProgress {
  done: number;
  total: number;
}

// Reflect update_task's parent-relationship result onto the CACHED rows: the
// child's own parentId/parentLink (and, riding the same write, its own
// possibly-just-stamped id), plus — when a parent was named — the SELECTED
// PARENT's id. With Task IDs off (the default) the parent's cached `id` stays
// `null` until this very write stamps it, and resolution compares ids, so
// skipping the parent-row patch would leave the just-made relationship
// invisible until a reload (Codex P1, PR #77). Not run at all on the
// idsEnabled path — see setParent — because then EVERY task's cached id is
// stale, not just these two rows.
function applyParentPatch(
  child: AggTask,
  allTasks: AggTask[],
  targetPath: string | null,
  res: TaskWriteResult,
): void {
  child.parentId = res.parentId;
  child.parentLink = res.parentLink;
  reflectStampedId(child, res.id);
  if (targetPath) {
    const parentRow = allTasks.find((t) => t.vaultId === child.vaultId && t.path === targetPath);
    if (parentRow && res.parentId) parentRow.id = res.parentId;
  }
}

/**
 * Resolves a task's parent/children/progress from the vault's already-loaded
 * task set (no IPC — see src/utils/taskHierarchy.ts for the shared
 * resolution rule), and writes parent changes via `setParent`.
 *
 * **The `busy` ref is PASSED IN, not created here.** TaskDetail.vue already
 * owns one from useTaskDetail; passing that SAME ref here makes it the one
 * guard serializing every write on the task. A second, independent guard
 * would let a field Save and a Change/Clear Parent overlap on the same
 * document: both atomic writers read the old content and the later
 * replacement discards the other's edit (Codex P2, PR #77). Defaults to a
 * fresh local ref so the pure-resolution tests (and any other read-only
 * caller) don't have to wire one up.
 *
 * `allTasks` is scoped to ONE vault by the caller (list_tasks is always
 * per-vault); every lookup here additionally filters by `task.value.vaultId`
 * so a stray cross-vault row can never resolve a relationship.
 *
 * `reload` is the container's existing task-set loader. It is called INSTEAD
 * OF the cheap two-row patch exactly when the write's `idsEnabled` comes back
 * true: that response means the whole cached set was loaded id-suppressed
 * (every task's `id` is `null`, not just the two rows this call touched), so
 * patching only those two would reveal the relationship just created while
 * leaving any PRE-EXISTING dormant hierarchy (hand-authored ids + parent
 * links that were invisible only because ids were off) still orphaned on
 * screen (Codex P2, PR #77). Optional: a caller with no reloader (or a test
 * exercising only the pure resolution) simply skips the reload.
 */
export function useTaskHierarchy(
  task: Ref<AggTask>,
  allTasks: Ref<AggTask[]>,
  busy: Ref<boolean> = ref(false),
  reload?: () => Promise<void>,
) {
  const notifications = useNotificationsStore();

  const index = computed(() => buildParentIndex(allTasks.value, task.value.vaultId));

  const parent = computed<AggTask | null>(() => {
    const parentPath = index.value.get(task.value.path);
    if (parentPath === undefined) return null;
    return (
      allTasks.value.find((t) => t.vaultId === task.value.vaultId && t.path === parentPath) ?? null
    );
  });

  const children = computed<AggTask[]>(() =>
    allTasks.value.filter(
      (t) => t.vaultId === task.value.vaultId && index.value.get(t.path) === task.value.path,
    ),
  );

  const progress = computed<TaskProgress>(() => ({
    done: children.value.filter((t) => t.done).length,
    total: children.value.length,
  }));

  async function setParent(path: string | null): Promise<void> {
    if (busy.value) return;
    busy.value = true;
    try {
      const patch = path !== null ? { parentPath: path } : { clearParent: true };
      const res = await invoke<TaskWriteResult>("update_task", {
        id: task.value.vaultId,
        path: task.value.path,
        patch,
      });
      if (res.idsEnabled) {
        await reload?.();
        notifications.notify("success", TASK_IDS_ENABLED_MESSAGE, {});
      } else {
        applyParentPatch(task.value, allTasks.value, path, res);
      }
    } catch (e) {
      notifications.error(String(e));
      logWarning(`set task parent failed: ${String(e)}`);
    } finally {
      busy.value = false;
    }
  }

  return { parent, children, progress, setParent };
}
