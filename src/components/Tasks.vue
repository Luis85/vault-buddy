<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskItem, TaskPatch, Vault } from "../types";
import { dueOf, localToday, parseTagsInput } from "../utils/taskFields";
import SelectMenu from "./SelectMenu.vue";
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
const newTitle = ref("");
const adding = ref(false);
const showAddOptions = ref(false);
const addDue = ref("");
const addPriority = ref("normal");
const addTags = ref("");
// Aggregate add: which vault receives the new task. Defaults to the first
// vault; component-local, no persistence across opens (YAGNI per spec).
const addVaultId = ref("");
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

const PRIORITY_RANK: Record<string, number> = { high: 0, low: 2 };
const rank = (t: TaskItem) => PRIORITY_RANK[t.priority ?? ""] ?? 1;
// "0<date>" < "1" makes valid dues sort ascending ahead of undated.
const dueKey = (t: TaskItem) => {
  const d = dueOf(t);
  return d ? `0${d}` : "1";
};

function sortInPlace() {
  // Mirrors core::tasks::list_tasks so an optimistic insert/edit lands where
  // a refetch would put it: open first (due asc → priority → newest created
  // → title); done by newest created → title.
  tasks.value.sort(
    (a, b) =>
      Number(a.done) - Number(b.done) ||
      (a.done
        ? b.created.localeCompare(a.created) ||
          a.title.localeCompare(b.title) ||
          a.vaultName.localeCompare(b.vaultName) ||
          a.path.localeCompare(b.path)
        : dueKey(a).localeCompare(dueKey(b)) ||
          rank(a) - rank(b) ||
          b.created.localeCompare(a.created) ||
          a.title.localeCompare(b.title) ||
          a.vaultName.localeCompare(b.vaultName) ||
          a.path.localeCompare(b.path)),
  );
}

type Bucket = { key: string; label: string | null; tasks: AggTask[] };

// Component-local; every panel visit starts back on dates (YAGNI: no
// persistence this slice).
const grouping = ref<"dates" | "tags">("dates");

const buckets = computed<Bucket[]>(() => {
  if (grouping.value === "tags") {
    // One section per tag (alphabetical, case-insensitive), a task under
    // EACH of its tags — Obsidian tag-pane semantics; then No tags, then
    // Done. Headers always render in tag mode. Order within sections is
    // the global sort, untouched.
    const byTag = new Map<string, { label: string; tasks: AggTask[] }>();
    const notags: AggTask[] = [];
    const done: AggTask[] = [];
    for (const t of filteredTasks.value) {
      if (t.done) {
        done.push(t);
        continue;
      }
      if (t.tags.length === 0) {
        notags.push(t);
        continue;
      }
      for (const tag of t.tags) {
        const key = tag.toLowerCase();
        const entry = byTag.get(key) ?? { label: tag, tasks: [] };
        entry.tasks.push(t);
        byTag.set(key, entry);
      }
    }
    const sections: Bucket[] = [...byTag.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, { label, tasks }]) => ({ key: `tag:${key}`, label: `#${label}`, tasks }));
    if (notags.length > 0) sections.push({ key: "notags", label: "No tags", tasks: notags });
    if (done.length > 0) sections.push({ key: "done", label: "Done", tasks: done });
    return sections;
  }
  const today = localToday();
  const groups: Record<string, AggTask[]> = { overdue: [], today: [], upcoming: [], nodate: [], done: [] };
  for (const t of filteredTasks.value) {
    if (t.done) groups.done.push(t);
    else {
      const d = dueOf(t);
      if (!d) groups.nodate.push(t);
      else if (d < today) groups.overdue.push(t);
      else if (d === today) groups.today.push(t);
      else groups.upcoming.push(t);
    }
  }
  // Headers only once a dated open task exists — a vault that never uses due
  // dates keeps the flat list it had before this feature.
  const showHeaders =
    groups.overdue.length + groups.today.length + groups.upcoming.length > 0;
  return [
    { key: "overdue", label: "Overdue" },
    { key: "today", label: "Today" },
    { key: "upcoming", label: "Upcoming" },
    { key: "nodate", label: "No date" },
    { key: "done", label: "Done" },
  ]
    .map(({ key, label }) => ({ key, label: showHeaders ? label : null, tasks: groups[key] }))
    .filter((b) => b.tasks.length > 0);
});

onMounted(async () => {
  try {
    if (props.vaultId !== null) {
      const items = await invoke<TaskItem[]>("list_tasks", { id: props.vaultId });
      const id = props.vaultId;
      tasks.value = items.map((t) => ({ ...t, vaultId: id, vaultName: "" }));
    } else {
      // Aggregate: fan out over every vault, best-effort per vault — the
      // same posture as the store's taskCounts load. A failed vault
      // contributes nothing and is named in ONE toast; the blocking banner
      // is reserved for list_vaults failing or EVERY vault failing.
      const vaults = await invoke<Vault[]>("list_vaults");
      allVaults.value = vaults;
      addVaultId.value = vaults[0]?.id ?? "";
      const failed: string[] = [];
      const results = await Promise.all(
        vaults.map(async (v) => {
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

async function add() {
  const title = newTitle.value.trim();
  const targetVault = isAggregate.value ? addVaultId.value : props.vaultId;
  if (!title || adding.value || !targetVault) return;
  adding.value = true;
  try {
    const args: Record<string, unknown> = { id: targetVault, title };
    if (addDue.value) args.due = addDue.value;
    if (addPriority.value !== "normal") args.priority = addPriority.value;
    const tags = parseTagsInput(addTags.value);
    if (tags.length > 0) args.tags = tags;
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
    newTitle.value = "";
    addDue.value = "";
    addPriority.value = "normal";
    addTags.value = "";
    showAddOptions.value = false;
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}

function onTitleEnter(e: KeyboardEvent) {
  // GAP-31: committing an IME candidate fires Enter with isComposing=true —
  // that must select the candidate, never create a task document (a vault
  // write) from the half-composed title. The Search view's handlers are the
  // precedent.
  if (e.isComposing) return;
  void add();
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

    <div class="flex items-center gap-1">
      <SelectMenu
        v-if="isAggregate"
        v-model="addVaultId"
        :options="vaultOptions"
        aria-label="Vault for the new task"
        data-testid="task-add-vault"
      />
      <input
        v-model="newTitle"
        data-testid="task-input"
        type="text"
        placeholder="Add a task…"
        aria-label="New task title"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="onTitleEnter"
      >
      <button
        type="button"
        data-testid="task-add-options"
        :aria-label="showAddOptions ? 'Hide task options' : 'Set due date or priority'"
        :aria-expanded="showAddOptions"
        title="Due date / priority"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="showAddOptions ? 'border-violet-400 text-slate-100' : ''"
        @click="showAddOptions = !showAddOptions"
      >
        ⋯
      </button>
      <button
        type="button"
        data-testid="task-add"
        :disabled="adding || newTitle.trim() === ''"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="add"
      >
        Add
      </button>
    </div>

    <div
      v-if="showAddOptions"
      class="flex items-center gap-1"
    >
      <input
        v-model="addDue"
        data-testid="task-add-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
      >
      <div
        class="flex gap-0.5"
        role="radiogroup"
        aria-label="Priority"
      >
        <button
          v-for="p in ['high', 'normal', 'low']"
          :key="p"
          type="button"
          role="radio"
          :data-testid="`task-add-priority-${p}`"
          :aria-checked="addPriority === p"
          class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            addPriority === p
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          @click="addPriority = p"
        >
          {{ p }}
        </button>
      </div>
      <input
        v-model="addTags"
        data-testid="task-add-tags"
        type="text"
        placeholder="#tags"
        aria-label="Tags"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      >
    </div>

    <div
      v-if="!loading && !loadError && tasks.length > 0"
      class="flex gap-0.5 self-start"
      role="radiogroup"
      aria-label="Group tasks by"
    >
      <button
        v-for="g in [
          { key: 'dates', label: 'Dates' },
          { key: 'tags', label: 'Tags' },
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
