<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useTaskActions } from "../composables/useTaskActions";
import { useTaskLists } from "../composables/useTaskLists";
import { useTaskReorder } from "../composables/useTaskReorder";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskItem, Vault } from "../types";
import { localToday } from "../utils/taskFields";
import { planReorder } from "../utils/taskOrder";
import { type Bucket, dateBuckets, listSections, tagSections } from "../utils/taskSections";
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
// Row writes (toggle/archive/open/editor save), the shared per-path busy
// guard, and the lists state each live in their composable — state + IPC
// split out for the LOC cap, rendering stays here.
const {
  busy,
  isBusy,
  toggle,
  archive,
  openInObsidian,
  editingKey,
  rowKey,
  startEdit,
  cancelEdit,
  onEditorSave,
} = useTaskActions({ tasks, sortInPlace });
const {
  listOrder,
  knownLists,
  creatingList,
  composerLists,
  composerDefaultList,
  listsForVault,
  loadVaultLists,
  loadVaultConfig,
  onComposerVaultChange,
  createList,
} = useTaskLists(props.vaultId);

// The composer's New list flow: the composable creates + caches; the picker
// shows the created list once it is re-selected here.
async function onCreateList(name: string) {
  const created = await createList(name);
  if (created !== null) composer.value?.setList(created);
}

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

// Manual ordering: drag (or arrow-key) a row within its section; the landed
// slot becomes an `order` rank via planReorder — one midpoint write in the
// common case, a section-wide materialization when ranks need seeding.
// Handles show only in Manual sort with NO filter active: reordering a
// filtered subset would write ranks against invisible neighbors.
const rootRef = ref<HTMLElement | null>(null);
const reordering = ref(false);
const reorderEnabled = computed(
  () =>
    sortPref.value.key === "manual" &&
    filter.value.trim() === "" &&
    tagFilter.value === null &&
    !reordering.value,
);
const { dragState, onHandlePointerDown, onHandleKeydown } = useTaskReorder({
  enabled: () => reorderEnabled.value,
  rowsFor: (sectionKey) =>
    rootRef.value
      ? ([...rootRef.value.querySelectorAll(`[data-reorder-section="${sectionKey}"]`)] as HTMLElement[])
      : [],
  commit: commitReorder,
});

async function commitReorder(sectionKey: string, fromIndex: number, toIndex: number) {
  const section = buckets.value.find((b) => b.key === sectionKey)?.tasks ?? [];
  const plan = planReorder(section, fromIndex, toIndex);
  if (!plan) return;
  if (plan.kind === "single") {
    const task = section[fromIndex];
    if (busy.value.has(task.path)) return;
    const prev = task.order;
    task.order = plan.order;
    sortInPlace();
    busy.value.add(task.path);
    try {
      await invoke("update_task", { id: task.vaultId, path: task.path, patch: { order: plan.order } });
    } catch (e) {
      task.order = prev;
      sortInPlace();
      notifications.error(String(e));
      logWarning(`reorder failed: ${String(e)}`);
    } finally {
      busy.value.delete(task.path);
    }
    return;
  }
  // Materialization: seed spaced ranks across the section — optimistic for
  // the whole batch, serialized writes (each its own file, possibly across
  // vaults in the aggregate), one revert + toast if ANY write fails. The
  // view-level guard keeps a second reorder from interleaving.
  reordering.value = true;
  const affected = section.filter((t) => plan.orders.has(t.path));
  const prevOrders = new Map(affected.map((t) => [t.path, t.order] as const));
  for (const t of affected) t.order = plan.orders.get(t.path) ?? t.order;
  sortInPlace();
  try {
    for (const t of affected) {
      await invoke("update_task", { id: t.vaultId, path: t.path, patch: { order: t.order } });
    }
  } catch (e) {
    for (const t of affected) t.order = prevOrders.get(t.path) ?? t.order;
    sortInPlace();
    notifications.error(`Couldn't save the new order: ${String(e)}`);
    logWarning(`reorder materialization failed: ${String(e)}`);
  } finally {
    reordering.value = false;
  }
}

// Component-local; every panel visit starts back on dates (YAGNI: no
// persistence this slice).
const grouping = ref<"dates" | "tags" | "lists">("dates");

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

</script>

<template>
  <div
    ref="rootRef"
    class="flex flex-col gap-2"
  >
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
            v-for="(task, i) in bucket.tasks"
            :key="rowKey(bucket.key, task)"
            :task="task"
            :busy="isBusy(task.path)"
            :is-aggregate="isAggregate"
            :editing="editingKey === rowKey(bucket.key, task)"
            :reorderable="reorderEnabled"
            :dragging="dragState?.sectionKey === bucket.key && dragState.fromIndex === i"
            :drop-target="
              dragState?.sectionKey === bucket.key &&
                dragState.toIndex === i &&
                dragState.fromIndex !== i
            "
            :data-reorder-section="bucket.key"
            @toggle="toggle(task)"
            @archive="archive(task)"
            @edit="startEdit(task, bucket.key)"
            @open="openInObsidian(task)"
            @tag-click="tagFilter = $event"
            @reorder-pointer-down="onHandlePointerDown($event, bucket.key, i)"
            @reorder-keydown="onHandleKeydown($event, bucket.key, i)"
          >
            <TaskEditor
              :task="task"
              :busy="isBusy(task.path)"
              :lists="listsForVault(task.vaultId)"
              @save="onEditorSave(task, $event)"
              @cancel="cancelEdit"
            />
          </TaskRow>
        </ul>
      </div>
    </template>
  </div>
</template>
