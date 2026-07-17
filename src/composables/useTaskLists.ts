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

// Rewrite the list preferences after a rename (`to` = the landed name) or a
// delete (`to` = null) so the default / order / archived set never point at a
// stale name — otherwise an untouched add would reapply the old default and
// recreate the old folder (Codex, PR #59). A List IS a folder, and the two
// lifecycle ops treat descendants DIFFERENTLY on disk:
//   - RENAME moves the whole subtree (Work → Projects turns Work/Q3 into
//     Projects/Q3), so descendants are prefix-rewritten under `to`.
//   - DELETE never removes descendants — `tasks::delete_task_list` keeps a
//     folder that still holds sub-lists or foreign files — so descendant prefs
//     are LEFT UNTOUCHED; dropping them would strand a still-existing child
//     list (unarchiving it, losing its order) (Codex, PR #59 re-review).
// Pure so syncListPrefs stays thin.
function remapListPrefs(cfg: TasksConfig | undefined, from: string, to: string | null) {
  const { defaultList = null, listOrder = [], archivedLists = [] } = (cfg ?? {}) as Partial<TasksConfig>;
  const low = from.toLowerCase();
  const prefix = `${low}/`;
  // Map one entry: the exact list → `to` (renamed) or dropped (deleted); a
  // descendant → rewritten under `to` on a RENAME only, else unchanged.
  const one = (l: string): string | null => {
    const ll = l.toLowerCase();
    if (ll === low) return to;
    if (to !== null && ll.startsWith(prefix)) return to + l.slice(from.length);
    return l;
  };
  const remap = (arr: string[]) => arr.map(one).filter((l): l is string => l !== null);
  return {
    defaultList: defaultList === null ? null : one(defaultList),
    listOrder: remap(listOrder),
    archivedLists: remap(archivedLists),
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
  // Persist + cache the remapped list prefs after a rename/delete so a stale
  // default/order/archived entry can't survive (Codex, PR #59). Only when the
  // config is cached — with none loaded there's nothing to remap and writing
  // computed-empty prefs would clobber the real (unread) settings.
  async function syncListPrefs(id: string, from: string, to: string | null) {
    const cfg = vaultConfigs.value.get(id);
    if (!cfg) return;
    const next = remapListPrefs(cfg, from, to);
    try {
      await invoke("set_task_lists_config", { id, ...next });
      vaultConfigs.value.set(id, { ...cfg, ...next });
      vaultConfigs.value = new Map(vaultConfigs.value);
    } catch (e) {
      logWarning(`sync list prefs after lifecycle change failed: ${String(e)}`);
    }
  }

  async function renameList(from: string, to: string): Promise<string | null> {
    if (vaultId === null) return null;
    const id = vaultId;
    try {
      const landed = await invoke<string>("rename_task_list", { id, from, to });
      // Paths changed: refresh this vault's list enumeration so the old name
      // drops and the landed one (which may carry a ` (N)` collision suffix)
      // appears.
      await loadVaultLists(id);
      await syncListPrefs(id, from, landed);
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
      const outcome = await invoke<{ moved: number; folderRemoved: boolean }>("delete_task_list", { id, list });
      await loadVaultLists(id);
      // Reconcile prefs ONLY when the folder was actually removed. A folder
      // kept because it still holds sub-lists or foreign files is a list that
      // STILL EXISTS, so clearing its default/order/archived entry would
      // strand it — and its descendants were never removed either (Codex, PR
      // #59 re-review). A removed folder was empty, so there are no descendant
      // prefs to worry about; dropping the exact entry is complete.
      if (outcome?.folderRemoved) await syncListPrefs(id, list, null);
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
