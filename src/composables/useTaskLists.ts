import { invoke } from "@tauri-apps/api/core";
import { computed, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { TasksConfig } from "../types";
import { orderLists } from "../utils/taskSections";

// The Tasks view's lists state: a List is a folder under the vault's tasks
// root; enumeration is fetched per vault (fan-out in aggregate mode,
// best-effort like the tasks load) so the Lists grouping can show empty
// lists and the pickers can offer every list. listOrder comes from the
// vault's lists settings object. Split out of Tasks.vue for the LOC cap —
// this is state + IPC, no rendering.
export function useTaskLists(vaultId: string | null) {
  const notifications = useNotificationsStore();
  const vaultLists = ref(new Map<string, string[]>());
  const vaultConfigs = ref(new Map<string, TasksConfig>());
  // Sections honor the vault's configured order in per-vault mode; the
  // aggregate stays alphabetical (a cross-vault order union is YAGNI).
  const listOrder = computed(() =>
    vaultId !== null ? (vaultConfigs.value.get(vaultId)?.listOrder ?? []) : [],
  );
  const knownLists = computed(() => {
    const seen = new Map<string, string>();
    for (const lists of vaultLists.value.values())
      for (const l of lists) {
        const k = l.toLowerCase();
        if (!seen.has(k)) seen.set(k, l);
      }
    return [...seen.values()];
  });

  // The composer's target vault (its own pick in aggregate mode); its lists
  // and configured default feed the composer's list picker, fetched lazily
  // per vault and cached in the maps above.
  const composerVaultId = ref<string | null>(vaultId);
  const creatingList = ref(false);
  function listsForVault(id: string): string[] {
    const cfg = vaultConfigs.value.get(id);
    return orderLists(vaultLists.value.get(id) ?? [], cfg?.listOrder ?? []);
  }
  const composerLists = computed(() =>
    composerVaultId.value === null ? [] : listsForVault(composerVaultId.value),
  );
  const composerDefaultList = computed(() => {
    const id = composerVaultId.value;
    return (id !== null && vaultConfigs.value.get(id)?.defaultList) || "";
  });

  // Best-effort per vault, like the tasks load: a failed read degrades
  // (log-only — the tasks toast already names a broken vault). In-flight
  // dedupe: the composer's initial vault-change fires while the aggregate
  // fan-out for the same vault is still pending — without the guard every
  // aggregate open would fetch the first vault's lists twice.
  const listsInFlight = new Set<string>();
  async function loadVaultLists(id: string) {
    if (listsInFlight.has(id)) return;
    listsInFlight.add(id);
    try {
      const lists = await invoke<string[]>("list_task_lists", { id });
      vaultLists.value.set(id, Array.isArray(lists) ? lists : []);
      vaultLists.value = new Map(vaultLists.value); // Map mutation isn't tracked
    } catch (e) {
      logWarning(`list_task_lists failed for vault ${id}: ${String(e)}`);
    } finally {
      listsInFlight.delete(id);
    }
  }

  const configsInFlight = new Set<string>();
  async function loadVaultConfig(id: string) {
    if (configsInFlight.has(id)) return;
    configsInFlight.add(id);
    try {
      const cfg = await invoke<TasksConfig>("get_tasks_config", { id });
      if (cfg && Array.isArray(cfg.listOrder)) {
        vaultConfigs.value.set(id, cfg);
        vaultConfigs.value = new Map(vaultConfigs.value); // Map mutation isn't tracked
      }
    } catch (e) {
      logWarning(`get_tasks_config failed for vault ${id}: ${String(e)}`);
    } finally {
      configsInFlight.delete(id);
    }
  }

  function onComposerVaultChange(id: string) {
    composerVaultId.value = id;
    if (!vaultConfigs.value.has(id)) void loadVaultConfig(id);
    if (!vaultLists.value.has(id)) void loadVaultLists(id);
  }

  // The composer's New list flow: create in the composer's target vault and
  // fold the landed name into the vault's lists. Returns the created name so
  // the caller can re-select it in the picker (null on failure — toasted).
  async function createList(name: string): Promise<string | null> {
    const id = composerVaultId.value ?? vaultId;
    if (id === null || creatingList.value) return null;
    creatingList.value = true;
    try {
      const created = await invoke<string>("create_task_list", { id, name });
      const lists = vaultLists.value.get(id) ?? [];
      if (!lists.some((l) => l.toLowerCase() === created.toLowerCase())) {
        vaultLists.value.set(id, [...lists, created]);
        vaultLists.value = new Map(vaultLists.value);
      }
      return created;
    } catch (e) {
      notifications.error(String(e));
      logWarning(`create_task_list failed: ${String(e)}`);
      return null;
    } finally {
      creatingList.value = false;
    }
  }

  return {
    listOrder,
    knownLists,
    creatingList,
    composerVaultId,
    composerLists,
    composerDefaultList,
    listsForVault,
    loadVaultLists,
    loadVaultConfig,
    onComposerVaultChange,
    createList,
  };
}
