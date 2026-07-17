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
// The lists-config that "toggle archive on" produces: add `list` to the
// archived set (idempotent) and clear the default when it IS the list being
// archived — otherwise the composer would render a blank picker and unpicked
// adds would silently land in the now-hidden list (Task 8 review, Minor #3).
// Pure so archiveList stays a thin invoke + cache update.
function withListArchived(cfg: TasksConfig | undefined, list: string) {
  const { archivedLists = [], defaultList = null, listOrder = [] } = (cfg ?? {}) as Partial<TasksConfig>;
  const low = list.toLowerCase();
  return {
    defaultList: defaultList && defaultList.toLowerCase() === low ? null : defaultList,
    listOrder,
    // Set-dedup rather than a branch: re-archiving isn't reachable from the UI
    // (an archived list's section is hidden), so the idempotency is only a
    // guard, not a tested path — keep it branchless.
    archivedLists: [...new Set([...archivedLists, list])],
  };
}

export function useTaskLists(vaultId: string | null) {
  const notifications = useNotificationsStore();
  const vaultLists = ref(new Map<string, string[]>());
  const vaultConfigs = ref(new Map<string, TasksConfig>());
  // Sections honor the vault's configured order in per-vault mode; the
  // aggregate stays alphabetical (a cross-vault order union is YAGNI).
  const listOrder = computed(() =>
    vaultId !== null ? (vaultConfigs.value.get(vaultId)?.listOrder ?? []) : [],
  );
  // Archived lists hide from the Lists grouping and pickers (Task 8) —
  // per-vault only, same simplification as listOrder above: the aggregate
  // spans every vault, so there's no single archived set to apply and a
  // cross-vault union is YAGNI (the aggregate's Lists grouping just doesn't
  // filter for now).
  const archivedLists = computed(() =>
    vaultId !== null ? (vaultConfigs.value.get(vaultId)?.archivedLists ?? []) : [],
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
  // Archived names are dropped (Task 8) — the composer offers only visible
  // lists to pick for a NEW task, matching what the Lists grouping shows.
  function listsForVault(id: string): string[] {
    const cfg = vaultConfigs.value.get(id);
    const archived = new Set((cfg?.archivedLists ?? []).map((a) => a.toLowerCase()));
    return orderLists(vaultLists.value.get(id) ?? [], cfg?.listOrder ?? []).filter(
      (l) => !archived.has(l.toLowerCase()),
    );
  }
  const composerLists = computed(() =>
    composerVaultId.value === null ? [] : listsForVault(composerVaultId.value),
  );
  // The inline editor's picker is the one exception: it must still show (and
  // allow moving OUT of) a task's OWN current list even when that list is
  // archived — listsForVault above already dropped it from the general
  // options, so union it back in rather than rendering a blank selection.
  function listsForEditor(id: string, currentList: string): string[] {
    const base = listsForVault(id);
    if (currentList === "" || base.some((l) => l.toLowerCase() === currentList.toLowerCase()))
      return base;
    return [...base, currentList];
  }
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

  // Section-menu actions (per-vault only — the menu is hidden in aggregate, so
  // these all key off the view's own `vaultId`). Each toasts + logs on failure
  // and returns a success signal the container uses to close the popover and
  // decide whether to reload tasks.
  async function renameList(from: string, to: string): Promise<string | null> {
    if (vaultId === null) return null;
    const id = vaultId;
    try {
      const landed = await invoke<string>("rename_task_list", { id, from, to });
      // Paths changed: refresh this vault's list enumeration so the old name
      // drops and the landed one (which may carry a ` (N)` collision suffix)
      // appears.
      await loadVaultLists(id);
      return landed;
    } catch (e) {
      notifications.error(String(e));
      logWarning(`rename_task_list failed: ${String(e)}`);
      return null;
    }
  }

  async function deleteList(list: string): Promise<boolean> {
    if (vaultId === null) return false;
    const id = vaultId;
    try {
      await invoke("delete_task_list", { id, list });
      await loadVaultLists(id);
      return true;
    } catch (e) {
      notifications.error(String(e));
      logWarning(`delete_task_list failed: ${String(e)}`);
      return false;
    }
  }

  async function archiveList(list: string): Promise<boolean> {
    if (vaultId === null) return false;
    const id = vaultId;
    const cfg = vaultConfigs.value.get(id);
    const next = withListArchived(cfg, list);
    try {
      await invoke("set_task_lists_config", { id, ...next });
      // Update the cached config so the section hides immediately (the
      // archivedLists computed re-filters) without a round-trip.
      if (cfg) {
        vaultConfigs.value.set(id, { ...cfg, ...next });
        vaultConfigs.value = new Map(vaultConfigs.value);
      }
      return true;
    } catch (e) {
      notifications.error(String(e));
      logWarning(`archive list failed: ${String(e)}`);
      return false;
    }
  }

  return {
    listOrder,
    knownLists,
    archivedLists,
    creatingList,
    composerVaultId,
    composerLists,
    composerDefaultList,
    listsForEditor,
    loadVaultLists,
    loadVaultConfig,
    onComposerVaultChange,
    createList,
    renameList,
    deleteList,
    archiveList,
  };
}
