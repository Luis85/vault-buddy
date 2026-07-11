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
  loadSortPref,
  NATURAL_DIR,
  saveSortPref,
  type SortKey,
  taskComparator,
  type TaskSortPref,
} from "../utils/taskSort";
import TaskComposer from "./TaskComposer.vue";
import TaskEditor from "./TaskEditor.vue";
import TaskRow from "./TaskRow.vue";
import TaskViewControls from "./TaskViewControls.vue";

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
  // Filter by dataset rather than interpolating sectionKey into an attribute
  // selector: a Lists-grouping key is `list:<name>`, and a list folder name
  // may contain a double quote (is_valid_list_name only bars /, \, leading
  // dot), which would make the selector invalid and throw. querySelectorAll
  // returns document order, and a section's rows are contiguous, so the
  // filtered list stays in visual order.
  rowsFor: (sectionKey) =>
    rootRef.value
      ? ([...rootRef.value.querySelectorAll<HTMLElement>("[data-reorder-section]")].filter(
          (el) => el.dataset.reorderSection === sectionKey,
        ))
      : [],
  commit: commitReorder,
});

async function commitReorder(sectionKey: string, fromIndex: number, toIndex: number) {
  const section = buckets.value.find((b) => b.key === sectionKey)?.tasks ?? [];
  const plan = planReorder(section, fromIndex, toIndex);
  if (!plan) return;
  if (plan.kind === "single") await writeSingleRank(section[fromIndex], plan.order);
  else await materializeRanks(section, plan.orders);
}

// One midpoint write, optimistic with revert — the common drop.
async function writeSingleRank(task: AggTask, order: number) {
  if (busy.value.has(task.path)) return;
  const prev = task.order;
  task.order = order;
  sortInPlace();
  busy.value.add(task.path);
  // The view-level guard (same one materialization uses) disables ALL handles
  // until this write resolves — not just the per-path busy flag. A second
  // reorder of a DIFFERENT row would compute its rank against this optimistic,
  // not-yet-persisted position; if this write then fails and reverts, that
  // second rank would be relative to a slot that never landed, diverging from
  // what the user asked for. Serialize reorders view-wide instead.
  reordering.value = true;
  try {
    await invoke("update_task", { id: task.vaultId, path: task.path, patch: { order } });
  } catch (e) {
    task.order = prev;
    sortInPlace();
    notifications.error(String(e));
    logWarning(`reorder failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
    reordering.value = false;
  }
}

// Materialization: seed spaced ranks across the section — optimistic for
// the whole batch, serialized writes (each its own file, possibly across
// vaults in the aggregate). The view-level guard keeps a second reorder from
// interleaving.
async function materializeRanks(section: AggTask[], orders: Map<string, number>) {
  const affected = section.filter((t) => orders.has(t.path));
  // Abort if ANY affected row already has an in-flight write (e.g. a slow
  // status toggle on a neighbor in this section). Materialize must write EVERY
  // affected row to establish the section's total order, so it can't just skip
  // the busy one — and writing order to that file mid-save would race the
  // in-flight write (both are read-modify-write frontmatter edits, so whichever
  // lands last drops the other's change). Bail and let the user retry once the
  // save lands — the same silent no-op writeSingleRank does for its one busy
  // row (Codex, PR #53 re-review).
  if (affected.some((t) => busy.value.has(t.path))) return;
  reordering.value = true;
  // No affected row is busy (asserted above), so guard them all and — because
  // this batch owns every one of their guards — clear them all in `finally`.
  // Its update_task(order) and a toggle/edit/archive on the same row are both
  // read-modify-write frontmatter saves, so leaving the row controls live would
  // let a concurrent write clobber the order (or vice versa).
  affected.forEach((t) => busy.value.add(t.path));
  const prevOrders = new Map(affected.map((t) => [t.path, t.order] as const));
  for (const t of affected) t.order = orders.get(t.path) ?? t.order;
  sortInPlace();
  // The writes are serialized and non-atomic across files, so a mid-batch
  // failure leaves earlier files already written. Track what landed and
  // revert ONLY the tasks that never reached disk — reverting the whole batch
  // would desync the UI from a partially-written section (the mismatch would
  // surface on the next reload as a phantom partial reorder). Same "keep what
  // succeeded, name what failed" posture as the editor's field-then-move save.
  const written = new Set<string>();
  try {
    for (const t of affected) {
      await invoke("update_task", { id: t.vaultId, path: t.path, patch: { order: t.order } });
      written.add(t.path);
    }
  } catch (e) {
    // `?? null` (not `?? t.order`): a previous order of null means the task
    // was UNRANKED and must revert to unranked — `null ?? t.order` would
    // wrongly keep the new optimistic rank. Every affected path is a key in
    // prevOrders, so a genuinely missing entry can't occur here.
    for (const t of affected) {
      if (!written.has(t.path)) t.order = prevOrders.get(t.path) ?? null;
    }
    sortInPlace();
    notifications.error(`Couldn't save the new order: ${String(e)}`);
    logWarning(`reorder materialization failed: ${String(e)}`);
  } finally {
    affected.forEach((t) => busy.value.delete(t.path));
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

// Per-vault list folders exist even before any task lands in them. The view
// must stay reachable so a fresh empty list is visible (via the Lists
// grouping) instead of hidden behind "No tasks yet" until the first task is
// added — the aggregate deliberately omits empty lists (Codex, PR #53
// re-review).
const hasDisplayableLists = computed(() => !isAggregate.value && knownLists.value.length > 0);

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
  // undefined = the composer had no explicit pick and its default hadn't
  // loaded — let the backend apply the configured default (see the composer).
  list: string | undefined;
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
    // A defined list (incl. "" = an explicit No list override) is sent as-is;
    // undefined is omitted so the backend applies the vault's configured
    // default — the composer only omits it before its default has loaded, so
    // a quick add during that window still lands in the default list.
    if (payload.list !== undefined) args.list = payload.list;
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

    <TaskViewControls
      v-if="!loading && !loadError && (tasks.length > 0 || hasDisplayableLists)"
      :grouping="grouping"
      :sort-pref="sortPref"
      @update:grouping="grouping = $event"
      @set-sort-key="setSortKey"
      @flip-sort-dir="flipSortDir"
    />

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
      v-else-if="tasks.length === 0 && buckets.length === 0"
      class="text-xs text-slate-400"
    >
      No tasks yet.
    </p>
    <p
      v-else-if="buckets.length === 0"
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
