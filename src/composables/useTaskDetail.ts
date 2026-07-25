import { invoke } from "@tauri-apps/api/core";
import { type Ref, ref, watch } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskEditorPatch } from "../types";
import { applyDetailFields, applyMovedTask, type MovedTask, reflectStampedId } from "../utils/taskMutations";

// A save failure AFTER the fields already persisted is a move failure: name the
// list clearly (the fields ARE saved), matching useTaskActions.onEditorSave
// (final review, PR #76). Kept out of `save` so it doesn't push that function
// past the complexity ratchet.
function saveErrorMessage(e: unknown, fieldsSaved: boolean, targetList: string | undefined): string {
  return fieldsSaved
    ? `Saved fields, but couldn't move to "${targetList || "No list"}": ${String(e)}`
    : String(e);
}

/** The single-task write layer for the detail view. Unlike useTaskActions it
 * owns ONE task (no shared list, no re-sort): edits apply to the passed ref,
 * and the tasks list re-fetches when the user goes back. */
export function useTaskDetail(task: Ref<AggTask>) {
  const notifications = useNotificationsStore();
  const vaults = useVaultsStore();
  // One shared in-flight guard serializes every detail WRITE (save / delete /
  // duplicate): a slow save must not leave delete or duplicate clickable, or a
  // delete could race the save's atomic rename and the save would recreate the
  // deleted file (Codex P2, PR #76). Matches the row-write busy invariant.
  const busy = ref(false);
  // Mirror the in-flight write to the store so ActionPanel disables the header
  // Back button while a detail write commits: leaving mid-write would let the
  // write finish off-screen against a stale, remounted Tasks list (the delete
  // double-nav of round 24, the delete/save stale-row of rounds 25/26). Reset on
  // the next openTaskDetail; a stale `true` after an unmount is harmless because
  // ActionPanel gates on `view === "taskDetail"` too (Codex P2, PR #76).
  watch(busy, (b) => {
    vaults.taskDetailBusy = b;
  });

  async function save(patch: TaskEditorPatch): Promise<boolean> {
    const { list: targetList, ...fieldPatch } = patch;
    const hasFields = Object.keys(fieldPatch).length > 0;
    if (!hasFields && targetList === undefined) return true;
    if (busy.value) return false;
    busy.value = true;
    let fieldsSaved = false;
    try {
      if (hasFields) {
        const id = await invoke<string | null>("update_task", {
          id: task.value.vaultId,
          path: task.value.path,
          patch: fieldPatch,
        });
        applyDetailFields(task.value, fieldPatch);
        reflectStampedId(task.value, id);
        fieldsSaved = true;
      }
      if (targetList !== undefined && targetList !== task.value.list) {
        const moved = await invoke<MovedTask>("move_task_to_list", {
          id: task.value.vaultId,
          path: task.value.path,
          list: targetList,
        });
        applyMovedTask(task.value, moved);
        task.value.list = targetList;
      }
      void vaults.refreshTaskCount(task.value.vaultId);
      return true;
    } catch (e) {
      notifications.error(saveErrorMessage(e, fieldsSaved, targetList));
      logWarning(`task detail save failed: ${String(e)}`);
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function remove(): Promise<void> {
    if (busy.value) return;
    busy.value = true;
    try {
      await invoke("delete_task", { id: task.value.vaultId, path: task.value.path });
      void vaults.refreshTaskCount(task.value.vaultId);
      notifications.success(`Deleted "${task.value.title}".`);
      // Only navigate if we're still ON the detail view. If the user clicked the
      // header Back during a slow delete, the view already moved to tasks, and a
      // second contextual back() would over-advance to the vault list (Codex P2,
      // PR #76).
      if (vaults.view === "taskDetail") vaults.back(); // to the tasks list (remounts + re-fetches)
    } catch (e) {
      busy.value = false;
      notifications.error(String(e));
      logWarning(`delete_task failed: ${String(e)}`);
    }
  }

  // Shared by openInObsidian and duplicate's "Open" toast action — both are
  // the exact same launch-then-close-panel operation, just aimed at a
  // different path (the task's own vs. the freshly duplicated copy). Await
  // the launch, close the panel only on success, and surface a failure —
  // never fire-and-forget the close or swallow the launch error (Codex P2,
  // PR #76).
  async function launchInObsidian(path: string, failLabel: string): Promise<void> {
    try {
      await invoke("open_task", { id: task.value.vaultId, path });
      void invoke("close_panel").catch(() => {});
    } catch (e) {
      notifications.error(String(e));
      logWarning(`${failLabel} failed: ${String(e)}`);
    }
  }

  async function duplicate(): Promise<void> {
    if (busy.value) return;
    busy.value = true;
    try {
      const newPath = await invoke<string>("duplicate_task", {
        id: task.value.vaultId,
        path: task.value.path,
      });
      void vaults.refreshTaskCount(task.value.vaultId);
      notifications.notify("success", `Duplicated "${task.value.title}".`, {
        action: { label: "Open", run: () => launchInObsidian(newPath, "open_task (duplicate)") },
      });
    } catch (e) {
      notifications.error(String(e));
      logWarning(`duplicate_task failed: ${String(e)}`);
    } finally {
      busy.value = false;
    }
  }

  function openInObsidian(): Promise<void> {
    return launchInObsidian(task.value.path, "open_task");
  }

  return { busy, save, remove, duplicate, openInObsidian };
}
