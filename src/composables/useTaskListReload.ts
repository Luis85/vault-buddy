import { invoke } from "@tauri-apps/api/core";
import type { Ref } from "vue";

import { logWarning } from "../logging";
import type { AggTask, TaskItem } from "../types";

/**
 * Re-fetch one vault's task set after a structural change (a list rename,
 * archive, or delete) relocates files on disk. Split out of Tasks.vue — a
 * grandfathered LOC hotspot at its recorded ceiling (docs/Gaps.md GAP-65) —
 * to make room for Task 10's hierarchy lookup without growing it.
 *
 * Per-vault only: the section menu that triggers this reload is hidden in
 * the aggregate view, so `vaultId === null` is a no-op.
 */
export function useTaskListReload(
  vaultId: string | null,
  tasks: Ref<AggTask[]>,
  sortInPlace: () => void,
) {
  async function reloadTasks(): Promise<void> {
    if (vaultId === null) return;
    try {
      const items = await invoke<TaskItem[]>("list_tasks", { id: vaultId });
      tasks.value = items.map((t) => ({ ...t, vaultId, vaultName: "" }));
      sortInPlace();
    } catch (e) {
      logWarning(`list_tasks reload failed for vault ${vaultId}: ${String(e)}`);
    }
  }
  return { reloadTasks };
}
