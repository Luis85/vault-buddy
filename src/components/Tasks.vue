<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskItem, TaskPatch, TasksConfig, Vault } from "../types";
import { localToday } from "../utils/taskFields";
import { type Bucket, dateBuckets, listSections, orderLists, tagSections } from "../utils/taskSections";
import {
  directionApplies,
  loadSortPref,
  NATURAL_DIR,
  saveSortPref,
  SORT_OPTIONS,
  type SortKey,
  taskComparator,
  type TaskSortPref,
} from "../utils/taskSort";
import SelectMenu from "./SelectMenu.vue";
import TaskComposer from "./TaskComposer.vue";
import TaskEditor from "./TaskEditor.vue";
import TaskRow from "./TaskRow.vue";

const props = defineProps<{ vaultId: string | null }>();
// Aggregate mode: one merged view across every vault (vaultId === null).
const isAggregate = computed(() => props.vaultId === null);

const notifications = useNotificationsStore();
const vaultsStore = useVaultsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasks = ref<AggTask[]>([]);
const allVaults = ref<Vault[]>([]);
const filter = ref("");
const adding = ref(false);
// The add composer owns its own draft field state and reports a parsed payload
// up via `submit`; the container keeps validation + the write + the reset call.
const composer = ref<InstanceType<typeof TaskComposer> | null>(null);
const vaultOptions = computed(() =>
  allVaults.value.map((v) => ({ value: v.id, label: v.name })),
);
// Task paths whose set_task_status write is in flight. A second action on the
// same row while its write is pending would race the first (on a slow disk the
// two writes can land out of order, leaving the file disagreeing with the UI),
// so the row's controls are disabled and re-entrant actions are ignored until
// it resolves — a toggle and an archive for the same task can't race. A
// reactive Set so the template's :disabled tracks add/delete. Keyed by path alone:
// task paths are unique across vaults (two vaults would have to contain the same
// absolute file), and the aggregation spec documents that assumption — this comment
// is its code-side anchor.
const busy = ref(new Set<string>());
const isBusy = (path: string) => busy.value.has(path);

// done / total of the visible (non-archived) list; drives the progress bar.
const progress = computed(() => {
  const total = tasks.value.length;
  const done = tasks.value.filter((t) => t.done).length;
  return { total, done, pct: total === 0 ? 0 : Math.round((done / total) * 100) };
});

// Same threshold as the vault list: a filter only earns its row above 5.
const showFilter = computed(() => tasks.value.length > 5);

// One active tag filter at a time, set by clicking a row chip. Matching is
// case-insensitive and exact per tag (nested tags are distinct strings).
// Independent of the >5 title-filter threshold: it can only be activated by
// clicking an existing chip, and its dismiss chip is always visible while
// active, so it can never strand the user.
const tagFilter = ref<string | null>(null);

const filteredTasks = computed(() => {
  const q = filter.value.trim().toLowerCase();
  const tag = tagFilter.value?.toLowerCase() ?? null;
  return tasks.value.filter((t) => {
    if (tag && !t.tags.some((x) => x.toLowerCase() === tag)) return false;
    // Gate the title query on showFilter too: archiving below the threshold
    // hides the INPUT, and a stale query with no visible control would
    // strand the user on a narrowed/empty list until remount.
    if (q && showFilter.value && !t.title.toLowerCase().includes(q)) return false;
    return true;
  });
});

// The user's sort choice for this view, persisted per view key ("all" for
// the aggregate). The comparator lives in utils/taskSort (mirroring
// core::tasks::list_tasks for Default) so an optimistic insert/edit lands
// where a refetch would put it.
const sortViewKey = props.vaultId ?? "all";
const sortPref = ref<TaskSortPref>(loadSortPref(sortViewKey));

function sortInPlace() {
  tasks.value.sort(taskComparator(sortPref.value));
}

// Picking a key resets direction to that key's natural one (due: soonest
// first, created: newest first) instead of inheriting the previous key's
// toggle state, which reads as arbitrary.
function setSortKey(key: SortKey) {
  sortPref.value = { key, dir: NATURAL_DIR[key] };
  saveSortPref(sortViewKey, sortPref.value);
  sortInPlace();
}

function flipSortDir() {
  sortPref.value = { ...sortPref.value, dir: sortPref.value.dir === "asc" ? "desc" : "asc" };
  saveSortPref(sortViewKey, sortPref.value);
  sortInPlace();
}

// Component-local; every panel visit starts back on dates (YAGNI: no
// persistence this slice).
const grouping = ref<"dates" | "tags" | "lists">("dates");

// A List is a folder under the vault's tasks root; enumeration is fetched
// per vault (fan-out in aggregate mode, best-effort like the tasks load)
// so the Lists grouping can show empty lists and the pickers can offer
// every list. listOrder comes from the vault's lists settings object; the
// aggregate keeps [] (a cross-vault order union is YAGNI — alphabetical).
const vaultLists = ref(new Map<string, string[]>());
const vaultConfigs = ref(new Map<string, TasksConfig>());
// Sections honor the vault's configured order in per-vault mode; the
// aggregate stays alphabetical (a cross-vault order union is YAGNI).
const listOrder = computed(() =>
  props.vaultId !== null ? (vaultConfigs.value.get(props.vaultId)?.listOrder ?? []) : [],
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
const composerVaultId = ref<string | null>(props.vaultId);
const creatingList = ref(false);
const composerLists = computed(() => {
  const id = composerVaultId.value;
  if (id === null) return [];
  const cfg = vaultConfigs.value.get(id);
  return orderLists(vaultLists.value.get(id) ?? [], cfg?.listOrder ?? []);
});
const composerDefaultList = computed(() => {
  const id = composerVaultId.value;
  return (id !== null && vaultConfigs.value.get(id)?.defaultList) || "";
});

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

// The composer's New list flow: create in the composer's target vault, fold
// the landed name into the vault's lists, and re-select it in the picker.
async function onCreateList(name: string) {
  const id = composerVaultId.value ?? props.vaultId;
  if (id === null || creatingList.value) return;
  creatingList.value = true;
  try {
    const created = await invoke<string>("create_task_list", { id, name });
    const lists = vaultLists.value.get(id) ?? [];
    if (!lists.some((l) => l.toLowerCase() === created.toLowerCase())) {
      vaultLists.value.set(id, [...lists, created]);
      vaultLists.value = new Map(vaultLists.value);
    }
    composer.value?.setList(created);
  } catch (e) {
    notifications.error(String(e));
    logWarning(`create_task_list failed: ${String(e)}`);
  } finally {
    creatingList.value = false;
  }
}

const buckets = computed<Bucket[]>(() => {
  if (grouping.value === "tags") return tagSections(filteredTasks.value);
  if (grouping.value === "lists")
    // Per-vault mode surfaces empty (fresh) lists; the aggregate skips them
    // to avoid cross-vault noise.
    return listSections(filteredTasks.value, knownLists.value, listOrder.value, {
      includeEmpty: !isAggregate.value,
    });
  return dateBuckets(filteredTasks.value, localToday());
});

// Best-effort per vault, like the tasks load: a vault whose enumeration
// fails contributes no lists (log-only — the tasks toast already names a
// broken vault) and the view still renders.
// In-flight dedupe: the composer's initial vault-change fires while the
// aggregate fan-out for the same vault is still pending — without the guard
// every aggregate open would fetch the first vault's lists twice.
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

onMounted(async () => {
  try {
    if (props.vaultId !== null) {
      const id = props.vaultId;
      const [items] = await Promise.all([
        invoke<TaskItem[]>("list_tasks", { id }),
        // Lists + config feed the Lists grouping and the composer's picker;
        // a failed read degrades (log-only, same posture as the tasks load).
        loadVaultLists(id),
        loadVaultConfig(id),
      ]);
      tasks.value = items.map((t) => ({ ...t, vaultId: id, vaultName: "" }));
      // Core hands back Default order; a persisted non-default sort must
      // apply to the initial load too, not only after edits.
      sortInPlace();
    } else {
      // Aggregate: fan out over every vault, best-effort per vault — the
      // same posture as the store's taskCounts load. A failed vault
      // contributes nothing and is named in ONE toast; the blocking banner
      // is reserved for list_vaults failing or EVERY vault failing.
      const vaults = await invoke<Vault[]>("list_vaults");
      allVaults.value = vaults;
      const failed: string[] = [];
      const results = await Promise.all(
        vaults.map(async (v) => {
          // The list enumeration rides the same fan-out (its own catch —
          // a lists failure must not mark the vault's TASKS as failed).
          void loadVaultLists(v.id);
          try {
            const items = await invoke<TaskItem[]>("list_tasks", { id: v.id });
            return items.map((t) => ({ ...t, vaultId: v.id, vaultName: v.name }));
          } catch (e) {
            failed.push(v.name);
            logWarning(`list_tasks failed for vault ${v.id}: ${String(e)}`);
            return [];
          }
        }),
      );
      if (vaults.length > 0 && failed.length === vaults.length) {
        loadError.value = "Couldn't load tasks from any vault.";
      } else {
        tasks.value = results.flat();
        sortInPlace();
        if (failed.length > 0) {
          notifications.error(`Couldn't load tasks from ${failed.join(", ")}.`);
        }
      }
    }
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

type AddPayload = {
  title: string;
  due: string;
  priority: string;
  tags: string[];
  list: string;
  vaultId: string | null;
};

async function add(payload: AddPayload) {
  const title = payload.title.trim();
  // Aggregate mode resolves the target from the payload's picked vault; a
  // single-vault view always writes to its own vault.
  const targetVault = isAggregate.value ? payload.vaultId : props.vaultId;
  if (!title || adding.value || !targetVault) return;
  adding.value = true;
  try {
    const args: Record<string, unknown> = { id: targetVault, title };
    if (payload.due) args.due = payload.due;
    if (payload.priority !== "normal") args.priority = payload.priority;
    if (payload.tags.length > 0) args.tags = payload.tags;
    // Always explicit ("" = the tasks root): the composer displayed the
    // effective target, so a picked No list overrides a configured default
    // instead of falling back to it.
    args.list = payload.list;
    const created = await invoke<TaskItem>("add_task", args);
    tasks.value.unshift({
      ...created,
      vaultId: targetVault,
      vaultName: allVaults.value.find((v) => v.id === targetVault)?.name ?? "",
    });
    sortInPlace();
    // GAP-32 / Codex PR #46: the vault-row badge only reloaded on
    // panel-shown, going stale after an add until the panel reopened.
    void vaultsStore.refreshTaskCount(targetVault);
    // Fields clear only on success — a failed add keeps the user's input.
    composer.value?.reset();
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}

async function toggle(task: AggTask) {
  // Ignore a re-action while this row's write is still pending — otherwise two
  // concurrent set_task_status writes for the same task can land out of order.
  if (busy.value.has(task.path)) return;
  // GAP-32: captured BEFORE the optimistic flip so a failed write can
  // restore the task's actual original status (e.g. "in-progress") instead
  // of forging "new" — the old revert derived the restored value from the
  // just-flipped `done` boolean, which only ever knows "done"/"new".
  const prevStatus = task.status;
  const done = !task.done;
  // Optimistic: flip locally, revert + notify on failure.
  task.done = done;
  task.status = done ? "done" : "new";
  sortInPlace();
  busy.value.add(task.path);
  try {
    await invoke("set_task_status", { id: task.vaultId, path: task.path, status: task.status });
    // Badge refresh (GAP-32 / Codex PR #46): kicked off right after the
    // write resolves, before the row's busy flag clears in `finally` — it's
    // fire-and-forget (`void`) against the vaults store's own state, so it
    // never blocks this row's controls either way, but starting it here
    // keeps it colocated with the success branch rather than a `finally`
    // that also runs on failure.
    void vaultsStore.refreshTaskCount(task.vaultId);
  } catch (e) {
    task.status = prevStatus;
    task.done = prevStatus === "done";
    sortInPlace();
    notifications.error(String(e));
    logWarning(`set_task_status failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
  }
}

async function archive(task: AggTask) {
  if (busy.value.has(task.path)) return;
  busy.value.add(task.path);
  // Optimistic: remove from the list; on failure push back + re-sort rather
  // than re-inserting at a captured index (GAP-32: the index goes stale —
  // one slot off — if a concurrent add landed while this write was in
  // flight; recomputing placement via sortInPlace is always correct).
  const index = tasks.value.findIndex((t) => t.path === task.path);
  const removed = tasks.value.splice(index, 1)[0];
  try {
    await invoke("set_task_status", { id: task.vaultId, path: task.path, status: "archived" });
    void vaultsStore.refreshTaskCount(task.vaultId);
  } catch (e) {
    tasks.value.push(removed);
    sortInPlace();
    notifications.error(String(e));
    logWarning(`archive failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
  }
}

async function openInObsidian(task: AggTask) {
  try {
    await invoke("open_task", { id: task.vaultId, path: task.path });
    // Obsidian takes over — get the panel out of the way. Panel visibility is
    // owned by Rust (close_panel), best-effort, mirroring the vault-open and
    // recording-open flows. A failed launch falls through to the catch and
    // keeps the panel up so the error toast is visible.
    void invoke("close_panel").catch(() => {});
  } catch (e) {
    notifications.error(String(e));
    logWarning(`open_task failed: ${String(e)}`);
  }
}

// Inline editor: one row at a time; opening another row discards unsaved
// edits in the first (the file is the source of truth, edits are cheap).
// Keyed on `${bucketKey}:${path}` (not a bare path) so a task rendered in
// two tag sections (Task 7) opens its editor on only the clicked row. The
// draft field state and its IME-guarded key handlers live in TaskEditor; the
// container keeps editingKey (which row is open) and the optimistic write.
const editingKey = ref<string | null>(null);
const rowKey = (bucketKey: string, task: AggTask) => `${bucketKey}:${task.path}`;

function startEdit(task: AggTask, bucketKey: string) {
  editingKey.value = rowKey(bucketKey, task);
}

function cancelEdit() {
  editingKey.value = null;
}

async function onEditorSave(task: AggTask, patch: TaskPatch) {
  editingKey.value = null;
  if (Object.keys(patch).length === 0) return;
  // Optimistic: apply locally (re-sort/re-bucket live), revert on failure.
  const before = { title: task.title, due: task.due, priority: task.priority, tags: task.tags };
  if (patch.title) task.title = patch.title;
  if (patch.clearDue) task.due = null;
  else if (patch.due) task.due = patch.due;
  if (patch.priority) task.priority = patch.priority === "normal" ? null : patch.priority;
  if (patch.tags !== undefined) task.tags = patch.tags;
  sortInPlace();
  busy.value.add(task.path);
  try {
    await invoke("update_task", { id: task.vaultId, path: task.path, patch });
  } catch (e) {
    Object.assign(task, before);
    sortInPlace();
    notifications.error(String(e));
    logWarning(`update_task failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div
      v-if="!loading && !loadError && progress.total > 0"
      data-testid="task-progress"
      class="flex items-center gap-2"
    >
      <div class="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-white/10">
        <div
          class="h-full rounded-full bg-violet-500 transition-all"
          :style="{ width: `${progress.pct}%` }"
        />
      </div>
      <span class="shrink-0 text-xs tabular-nums text-slate-400">
        {{ progress.done }} / {{ progress.total }}
      </span>
    </div>

    <input
      v-if="showFilter"
      v-model="filter"
      data-testid="task-filter"
      type="search"
      placeholder="Filter tasks…"
      aria-label="Filter tasks"
      class="rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
    >

    <div
      v-if="tagFilter"
      data-testid="task-tag-filter"
      class="flex items-center gap-1 self-start rounded-full bg-violet-500/20 py-0.5 pl-2 pr-1 text-xs text-violet-200"
    >
      <span>#{{ tagFilter }}</span>
      <button
        type="button"
        data-testid="task-tag-filter-clear"
        aria-label="Clear tag filter"
        class="cursor-pointer rounded-full px-1 text-violet-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="tagFilter = null"
      >
        ✕
      </button>
    </div>

    <TaskComposer
      ref="composer"
      :is-aggregate="isAggregate"
      :vault-options="vaultOptions"
      :adding="adding"
      :lists="composerLists"
      :default-list="composerDefaultList"
      :creating-list="creatingList"
      @submit="add"
      @create-list="onCreateList"
      @vault-change="onComposerVaultChange"
    />

    <div
      v-if="!loading && !loadError && tasks.length > 0"
      class="flex items-center gap-0.5"
    >
      <div
        class="flex gap-0.5"
        role="radiogroup"
        aria-label="Group tasks by"
      >
        <button
          v-for="g in [
            { key: 'dates', label: 'Dates' },
            { key: 'tags', label: 'Tags' },
            { key: 'lists', label: 'Lists' },
          ] as const"
          :key="g.key"
          type="button"
          role="radio"
          :data-testid="`task-grouping-${g.key}`"
          :aria-checked="grouping === g.key"
          class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            grouping === g.key
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          @click="grouping = g.key"
        >
          {{ g.label }}
        </button>
      </div>
      <div class="ml-auto flex items-center gap-1">
        <SelectMenu
          :model-value="sortPref.key"
          :options="SORT_OPTIONS"
          aria-label="Sort tasks"
          data-testid="task-sort"
          @update:model-value="setSortKey($event as SortKey)"
        />
        <button
          type="button"
          data-testid="task-sort-dir"
          :disabled="!directionApplies(sortPref.key)"
          :aria-label="`Sort direction: ${sortPref.dir === 'asc' ? 'ascending' : 'descending'}`"
          :title="sortPref.dir === 'asc' ? 'Ascending' : 'Descending'"
          class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
          @click="flipSortDir"
        >
          {{ sortPref.dir === "asc" ? "↑" : "↓" }}
        </button>
      </div>
    </div>

    <p
      v-if="loading"
      class="text-xs text-slate-400"
    >
      Loading…
    </p>
    <p
      v-else-if="loadError"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <p
      v-else-if="tasks.length === 0"
      class="text-xs text-slate-400"
    >
      No tasks yet.
    </p>
    <p
      v-else-if="filteredTasks.length === 0"
      class="text-xs text-slate-400"
    >
      No tasks match{{ tagFilter ? ` #${tagFilter}` : "" }}{{ showFilter && filter ? ` "${filter}"` : "" }}.
    </p>
    <template v-else>
      <div
        v-for="bucket in buckets"
        :key="bucket.key"
        class="mt-1 first:mt-0"
      >
        <h3
          v-if="bucket.label"
          data-testid="task-bucket-header"
          class="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wider"
          :class="bucket.key === 'overdue' ? 'text-red-300' : 'text-slate-500'"
        >
          {{ bucket.label }}
        </h3>
        <ul class="flex flex-col gap-1">
          <TaskRow
            v-for="task in bucket.tasks"
            :key="rowKey(bucket.key, task)"
            :task="task"
            :busy="isBusy(task.path)"
            :is-aggregate="isAggregate"
            :editing="editingKey === rowKey(bucket.key, task)"
            @toggle="toggle(task)"
            @archive="archive(task)"
            @edit="startEdit(task, bucket.key)"
            @open="openInObsidian(task)"
            @tag-click="tagFilter = $event"
          >
            <TaskEditor
              :task="task"
              :busy="isBusy(task.path)"
              @save="onEditorSave(task, $event)"
              @cancel="cancelEdit"
            />
          </TaskRow>
        </ul>
      </div>
    </template>
  </div>
</template>
