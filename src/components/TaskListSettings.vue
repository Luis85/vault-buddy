<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref, watch } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { TasksConfig } from "../types";
import { orderLists } from "../utils/taskSections";
import TaskListPicker from "./TaskListPicker.vue";

// The per-vault lists settings object (defaultList + listOrder), rendered
// inside the Vault settings Tasks tab. Self-contained (own load) and
// auto-saved: a default-list pick or a reorder saves immediately through
// set_task_lists_config. Folders on disk stay the source of truth for which
// lists exist; this card only edits preferences about them.
const props = defineProps<{ vaultId: string }>();
// Surfaced so the parent (TasksConfigTab) can fence the tasks-folder input
// while a list save is in flight — a folder change must not overlap a list
// save, or the late list write lands old-root preferences on the new root.
const emit = defineEmits<{ "saving-change": [value: boolean] }>();

const loading = ref(true);
const defaultList = ref("");
// The vault's lists in effective display order (listOrder first, the rest
// alphabetical — exactly what the sections and pickers render). Reordering
// edits this array; a save persists it as the new listOrder.
const order = ref<string[]>([]);

const autosave = useAutosave(
  async () => {
    await invoke("set_task_lists_config", {
      id: props.vaultId,
      defaultList: defaultList.value || null,
      listOrder: order.value,
    });
  },
  { label: "task lists" },
);
watch(autosave.saving, (value) => emit("saving-change", value));

onMounted(async () => {
  try {
    const [cfg, lists] = await Promise.all([
      invoke<TasksConfig>("get_tasks_config", { id: props.vaultId }),
      invoke<string[]>("list_task_lists", { id: props.vaultId }),
    ]);
    defaultList.value = cfg?.defaultList ?? "";
    order.value = orderLists(
      Array.isArray(lists) ? lists : [],
      Array.isArray(cfg?.listOrder) ? cfg.listOrder : [],
    );
  } catch (e) {
    // Read failures degrade to an empty card (log-only) — a later save still
    // field-errors if attempted, so nothing is silently lost.
    logWarning(`task list settings load failed: ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

// The picker and the reorder buttons fire only on user action (onMounted
// assigns the refs directly), so saveNow() here never fires on load.
function onDefaultChange(value: string) {
  defaultList.value = value;
  autosave.saveNow();
}
function move(index: number, delta: -1 | 1) {
  const target = index + delta;
  if (target < 0 || target >= order.value.length) return;
  const next = [...order.value];
  [next[index], next[target]] = [next[target], next[index]];
  order.value = next;
  autosave.saveNow();
}
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Task lists
    </h2>
    <div class="rounded-xl border border-white/10 bg-white/5 p-2">
      <p
        v-if="loading"
        class="text-xs text-slate-400"
      >
        Loading…
      </p>
      <template v-else>
        <label class="mb-1 block text-sm text-slate-200">
          Default list for new tasks
          <span class="block text-xs text-slate-500">Where a task lands when you don't pick a list</span>
        </label>
        <TaskListPicker
          :model-value="defaultList"
          :lists="order"
          :allow-create="false"
          aria-label="Default list for new tasks"
          data-testid="default-list"
          @update:model-value="onDefaultChange"
        />
        <template v-if="order.length > 1">
          <p class="mb-1 mt-2 text-sm text-slate-200">
            List order
            <span class="block text-xs text-slate-500">How sections and pickers arrange the lists</span>
          </p>
          <ul class="flex flex-col gap-1">
            <li
              v-for="(list, i) in order"
              :key="list"
              data-testid="list-order-row"
              class="flex items-center gap-1 rounded-lg border border-white/10 bg-white/5 px-2 py-0.5"
            >
              <span class="min-w-0 flex-1 truncate text-sm text-slate-100">{{ list }}</span>
              <button
                type="button"
                :data-testid="`list-order-up-${i}`"
                :disabled="i === 0"
                :aria-label="`Move ${list} up`"
                class="cursor-pointer rounded px-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-30"
                @click="move(i, -1)"
              >
                ↑
              </button>
              <button
                type="button"
                :data-testid="`list-order-down-${i}`"
                :disabled="i === order.length - 1"
                :aria-label="`Move ${list} down`"
                class="cursor-pointer rounded px-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-30"
                @click="move(i, 1)"
              >
                ↓
              </button>
            </li>
          </ul>
        </template>
        <p
          v-if="order.length === 0"
          class="mt-1 text-xs text-slate-500"
        >
          No lists yet — create one from the tasks view's Add options.
        </p>
        <p
          v-if="autosave.error.value"
          data-testid="task-lists-error"
          class="mt-1 text-xs text-red-300"
        >
          {{ autosave.error.value }}
        </p>
      </template>
    </div>
  </section>
</template>
