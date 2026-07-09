<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { TaskItem, TasksConfig } from "../types";

const props = defineProps<{ vaultId: string }>();
const notifications = useNotificationsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasks = ref<TaskItem[]>([]);
const newTitle = ref("");
const folder = ref(""); // empty shows the "Tasks" placeholder
const adding = ref(false);
// Task paths whose set_task_status write is in flight. A second toggle of the
// same row while its write is pending would race the first (on a slow disk the
// two writes can land out of order, leaving the file disagreeing with the UI),
// so the checkbox is disabled and re-entrant toggles are ignored until it
// resolves. A reactive Set so the template's :disabled tracks add/delete.
const toggling = ref(new Set<string>());
const isToggling = (path: string) => toggling.value.has(path);

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

async function reload() {
  // Post-mount refresh (after a folder change): keep the existing list on a
  // transient failure — never blank a working list (mirrors the vaults store)
  // — and surface the error as a toast instead of the blocking banner.
  try {
    tasks.value = await invoke<TaskItem[]>("list_tasks", { id: props.vaultId });
    loadError.value = null;
  } catch (e) {
    notifications.error(String(e));
    logWarning(`list_tasks refresh failed: ${String(e)}`);
  }
}

onMounted(async () => {
  try {
    const cfg = await invoke<TasksConfig>("get_tasks_config", { id: props.vaultId });
    folder.value = cfg.tasksFolder ?? "";
    // Initial load: a failure here has no list to preserve, so it shows the
    // blocking loadError banner rather than a toast.
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
  // Ignore a re-toggle while this row's write is still pending — otherwise two
  // concurrent set_task_status writes for the same task can land out of order.
  if (toggling.value.has(task.path)) return;
  const done = !task.done;
  // Optimistic: flip locally, revert + notify on failure.
  task.done = done;
  task.status = done ? "done" : "new";
  sortInPlace();
  toggling.value.add(task.path);
  try {
    await invoke("set_task_status", { id: props.vaultId, path: task.path, done });
  } catch (e) {
    task.done = !done;
    task.status = done ? "new" : "done";
    sortInPlace();
    notifications.error(String(e));
    logWarning(`set_task_status failed: ${String(e)}`);
  } finally {
    toggling.value.delete(task.path);
  }
}

async function saveFolder() {
  const value = folder.value.trim();
  try {
    await invoke("set_tasks_config", {
      id: props.vaultId,
      tasksFolder: value === "" ? null : value,
    });
    await reload();
  } catch (e) {
    notifications.error(String(e));
    logWarning(`set_tasks_config failed: ${String(e)}`);
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center gap-1">
      <input
        v-model="folder"
        data-testid="tasks-folder-input"
        type="text"
        placeholder="Tasks"
        aria-label="Tasks folder"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="saveFolder"
      />
      <button
        type="button"
        data-testid="tasks-folder-save"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="saveFolder"
      >
        Save
      </button>
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
          :disabled="isToggling(task.path)"
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
      </li>
    </ul>
  </div>
</template>
