<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { TaskItem, TaskPatch, Vault } from "../types";
import SelectMenu from "./SelectMenu.vue";

// Split a free-text tags field on commas/whitespace, strip leading `#`s,
// drop empties, dedupe case-insensitively keeping the first casing.
// Client-side parsing is lenient; the shell strictly validates the charset
// and errors on a bad token.
function parseTagsInput(s: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of s.split(/[\s,]+/)) {
    const t = raw.replace(/^#+/, "");
    if (!t || seen.has(t.toLowerCase())) continue;
    seen.add(t.toLowerCase());
    out.push(t);
  }
  return out;
}

// A due only counts when it's a plain YYYY-MM-DD — a hand-authored value like
// "tomorrow" degrades to no-date instead of erroring (defensive read).
const DUE_RE = /^\d{4}-\d{2}-\d{2}$/;
const dueOf = (t: TaskItem): string | null =>
  t.due && DUE_RE.test(t.due) ? t.due : null;

// LOCAL calendar date — never UTC/ISO slicing, matching add_task's local-date
// rule; near midnight UTC-derived "today" would mis-bucket by a day.
function localToday(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// Deterministic short label (no locale dependence): "Jul 15".
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
function dueLabel(d: string): string {
  const [, m, day] = d.split("-");
  const month = MONTHS[Number(m) - 1];
  return month ? `${month} ${Number(day)}` : d;
}
const isOverdue = (t: TaskItem): boolean => {
  const d = dueOf(t);
  return d !== null && !t.done && d < localToday();
};

const props = defineProps<{ vaultId: string | null }>();
// Aggregate mode: one merged view across every vault (vaultId === null).
const isAggregate = computed(() => props.vaultId === null);

// A task enriched with its owning vault — the ONE internal shape for both
// modes, so every action reads task.vaultId and needs no mode branches.
type AggTask = TaskItem & { vaultId: string; vaultName: string };

const notifications = useNotificationsStore();

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
// reactive Set so the template's :disabled tracks add/delete.
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

async function toggle(task: AggTask) {
  // Ignore a re-action while this row's write is still pending — otherwise two
  // concurrent set_task_status writes for the same task can land out of order.
  if (busy.value.has(task.path)) return;
  const done = !task.done;
  // Optimistic: flip locally, revert + notify on failure.
  task.done = done;
  task.status = done ? "done" : "new";
  sortInPlace();
  busy.value.add(task.path);
  try {
    await invoke("set_task_status", { id: task.vaultId, path: task.path, status: task.status });
  } catch (e) {
    task.done = !done;
    task.status = done ? "new" : "done";
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
  // Optimistic: remove from the list; re-insert at the same spot + notify on
  // failure (the list stays sorted, so re-inserting at `index` restores order).
  const index = tasks.value.findIndex((t) => t.path === task.path);
  const removed = tasks.value.splice(index, 1)[0];
  try {
    await invoke("set_task_status", { id: task.vaultId, path: task.path, status: "archived" });
  } catch (e) {
    tasks.value.splice(index, 0, removed);
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
// two tag sections (Task 7) opens its editor on only the clicked row.
const editingKey = ref<string | null>(null);
const rowKey = (bucketKey: string, task: AggTask) => `${bucketKey}:${task.path}`;
const editTitle = ref("");
const editDue = ref("");
const editPriority = ref("normal");
const editTags = ref("");

const normalizedPriority = (t: TaskItem) =>
  t.priority === "high" || t.priority === "low" ? t.priority : "normal";

function startEdit(task: AggTask, bucketKey: string) {
  editingKey.value = rowKey(bucketKey, task);
  editTitle.value = task.title;
  editDue.value = dueOf(task) ?? "";
  editPriority.value = normalizedPriority(task);
  editTags.value = task.tags.join(", ");
}

function cancelEdit() {
  editingKey.value = null;
}

async function saveEdit(task: AggTask) {
  if (busy.value.has(task.path)) return;
  const patch: TaskPatch = {};
  const title = editTitle.value.trim();
  if (title && title !== task.title) patch.title = title;
  if (editDue.value !== (dueOf(task) ?? "")) {
    if (editDue.value === "") patch.clearDue = true;
    else patch.due = editDue.value;
  }
  if (editPriority.value !== normalizedPriority(task)) patch.priority = editPriority.value;
  const parsedTags = parseTagsInput(editTags.value);
  if (parsedTags.join(" ") !== task.tags.join(" ")) patch.tags = parsedTags;
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
        ></div>
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
    />

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
        @keydown.enter="add"
      />
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

    <div v-if="showAddOptions" class="flex items-center gap-1">
      <input
        v-model="addDue"
        data-testid="task-add-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
      />
      <div class="flex gap-0.5" role="radiogroup" aria-label="Priority">
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
      />
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

    <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
    <p
      v-else-if="loadError"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <p v-else-if="tasks.length === 0" class="text-xs text-slate-400">
      No tasks yet.
    </p>
    <p v-else-if="filteredTasks.length === 0" class="text-xs text-slate-400">
      No tasks match{{ tagFilter ? ` #${tagFilter}` : "" }}{{ showFilter && filter ? ` "${filter}"` : "" }}.
    </p>
    <template v-else>
      <div v-for="bucket in buckets" :key="bucket.key" class="mt-1 first:mt-0">
        <h3
          v-if="bucket.label"
          data-testid="task-bucket-header"
          class="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wider"
          :class="bucket.key === 'overdue' ? 'text-red-300' : 'text-slate-500'"
        >
          {{ bucket.label }}
        </h3>
        <ul class="flex flex-col gap-1">
          <li
            v-for="task in bucket.tasks"
            :key="rowKey(bucket.key, task)"
            data-testid="task-row"
            class="flex items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1"
          >
            <div v-if="editingKey === rowKey(bucket.key, task)" class="flex min-w-0 flex-1 flex-col gap-1 py-0.5">
              <input
                v-model="editTitle"
                data-testid="task-edit-title"
                type="text"
                aria-label="Task title"
                class="min-w-0 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none"
                @keydown.enter.prevent="saveEdit(task)"
                @keydown.esc="cancelEdit"
              />
              <div class="flex items-center gap-1">
                <input
                  v-model="editDue"
                  data-testid="task-edit-due"
                  type="date"
                  aria-label="Due date"
                  class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
                />
                <div class="flex gap-0.5" role="radiogroup" aria-label="Priority">
                  <button
                    v-for="p in ['high', 'normal', 'low']"
                    :key="p"
                    type="button"
                    role="radio"
                    :data-testid="`task-edit-priority-${p}`"
                    :aria-checked="editPriority === p"
                    class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
                    :class="
                      editPriority === p
                        ? 'border-violet-400 bg-violet-500/20 text-slate-100'
                        : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
                    "
                    @click="editPriority = p"
                  >
                    {{ p }}
                  </button>
                </div>
              </div>
              <input
                v-model="editTags"
                data-testid="task-edit-tags"
                type="text"
                placeholder="#tags"
                aria-label="Tags"
                class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
              />
              <div class="flex items-center justify-end gap-1">
                <button
                  type="button"
                  data-testid="task-edit-cancel"
                  class="cursor-pointer rounded-lg px-2 py-0.5 text-xs text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
                  @click="cancelEdit"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  data-testid="task-edit-save"
                  :disabled="isBusy(task.path)"
                  class="cursor-pointer rounded-lg bg-violet-600/80 px-2 py-0.5 text-xs font-semibold text-white hover:bg-violet-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
                  @click="saveEdit(task)"
                >
                  Save
                </button>
              </div>
            </div>
            <template v-else>
              <input
                type="checkbox"
                data-testid="task-checkbox"
                :checked="task.done"
                :disabled="isBusy(task.path)"
                :aria-label="`Mark ${task.title} ${task.done ? 'not done' : 'done'}`"
                class="shrink-0 cursor-pointer accent-violet-500 disabled:cursor-default disabled:opacity-50"
                @change="toggle(task)"
              />
              <button
                type="button"
                data-testid="task-open"
                class="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
                :aria-label="`Open ${task.title} in Obsidian`"
                :title="`Open ${task.title} in Obsidian`"
                @click="openInObsidian(task)"
              >
                <span
                  v-if="isAggregate"
                  data-testid="task-vault"
                  class="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-violet-600/80 text-[9px] font-bold text-white"
                  :title="task.vaultName"
                >{{ task.vaultName.charAt(0).toUpperCase() }}</span>
                <span
                  v-if="task.priority === 'high' || task.priority === 'low'"
                  data-testid="task-priority"
                  class="h-1.5 w-1.5 shrink-0 rounded-full"
                  :class="task.priority === 'high' ? 'bg-red-400' : 'bg-slate-500'"
                  :title="task.priority === 'high' ? 'High priority' : 'Low priority'"
                  aria-hidden="true"
                ></span>
                <span
                  class="min-w-0 flex-1 truncate text-sm"
                  :class="task.done ? 'text-slate-500 line-through' : 'text-slate-100'"
                >
                  {{ task.title }}
                </span>
                <span
                  v-for="tag in task.tags"
                  :key="tag"
                  data-testid="task-tag"
                  role="button"
                  tabindex="0"
                  :aria-label="`Filter by tag ${tag}`"
                  class="shrink-0 cursor-pointer rounded-full bg-white/10 px-1.5 text-[10px] text-violet-200 transition-colors hover:bg-violet-500/30"
                  @click.stop="tagFilter = tag"
                  @keydown.enter.stop.prevent="tagFilter = tag"
                  @keydown.space.stop.prevent="tagFilter = tag"
                >#{{ tag }}</span>
                <span
                  v-if="dueOf(task)"
                  data-testid="task-due"
                  class="shrink-0 text-[10px] tabular-nums"
                  :class="isOverdue(task) ? 'font-semibold text-red-300' : 'text-slate-400'"
                >{{ dueLabel(dueOf(task)!) }}</span>
              </button>
              <button
                type="button"
                data-testid="task-edit"
                :disabled="isBusy(task.path)"
                :aria-label="`Edit ${task.title}`"
                title="Edit"
                class="shrink-0 cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
                @click="startEdit(task, bucket.key)"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
                </svg>
              </button>
              <button
                type="button"
                data-testid="task-archive"
                :disabled="isBusy(task.path)"
                :aria-label="`Archive ${task.title}`"
                title="Archive"
                class="shrink-0 cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
                @click="archive(task)"
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <rect x="3" y="4" width="18" height="4" rx="1" />
                  <path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4" />
                </svg>
              </button>
            </template>
          </li>
        </ul>
      </div>
    </template>
  </div>
</template>
