<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { TaskItem } from "../types";

const props = defineProps<{ vaultId: string }>();
const notifications = useNotificationsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasks = ref<TaskItem[]>([]);
const newTitle = ref("");
const adding = ref(false);
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

function sortInPlace() {
  // Open first, newest created first, then title — mirrors the backend order
  // so an optimistic insert lands where a refetch would put it.
  tasks.value.sort(
    (a, b) =>
      Number(a.done) - Number(b.done) ||
      b.created.localeCompare(a.created) ||
      a.title.localeCompare(b.title),
  );
}

onMounted(async () => {
  try {
    tasks.value = await invoke<TaskItem[]>("list_tasks", { id: props.vaultId });
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

async function add() {
  const title = newTitle.value.trim();
  if (!title || adding.value) return;
  adding.value = true;
  try {
    const created = await invoke<TaskItem>("add_task", { id: props.vaultId, title });
    tasks.value.unshift(created);
    sortInPlace();
    newTitle.value = "";
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}

async function toggle(task: TaskItem) {
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
    await invoke("set_task_status", { id: props.vaultId, path: task.path, status: task.status });
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

async function archive(task: TaskItem) {
  if (busy.value.has(task.path)) return;
  busy.value.add(task.path);
  // Optimistic: remove from the list; re-insert at the same spot + notify on
  // failure (the list stays sorted, so re-inserting at `index` restores order).
  const index = tasks.value.findIndex((t) => t.path === task.path);
  const removed = tasks.value.splice(index, 1)[0];
  try {
    await invoke("set_task_status", { id: props.vaultId, path: task.path, status: "archived" });
  } catch (e) {
    tasks.value.splice(index, 0, removed);
    notifications.error(String(e));
    logWarning(`archive failed: ${String(e)}`);
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

    <div class="flex items-center gap-1">
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
        data-testid="task-add"
        :disabled="adding || newTitle.trim() === ''"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="add"
      >
        Add
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
    <ul v-else class="flex flex-col gap-1">
      <li
        v-for="task in tasks"
        :key="task.path"
        data-testid="task-row"
        class="flex items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1"
      >
        <input
          type="checkbox"
          data-testid="task-checkbox"
          :checked="task.done"
          :disabled="isBusy(task.path)"
          :aria-label="`Mark ${task.title} ${task.done ? 'not done' : 'done'}`"
          class="shrink-0 cursor-pointer accent-violet-500 disabled:cursor-default disabled:opacity-50"
          @change="toggle(task)"
        />
        <span
          class="min-w-0 flex-1 truncate text-sm"
          :class="task.done ? 'text-slate-500 line-through' : 'text-slate-100'"
          :title="task.title"
        >
          {{ task.title }}
        </span>
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
      </li>
    </ul>
  </div>
</template>
