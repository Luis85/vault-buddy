<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref, watch } from "vue";

import { logWarning } from "../logging";
import type { TasksConfig } from "../types";
import { orderLists } from "../utils/taskSections";
import TaskListPicker from "./TaskListPicker.vue";

// The per-vault lists settings object (defaultList + listOrder), rendered
// inside the Vault settings view below the Tasks folder. Self-contained
// (own load/save/error — the McpSettings/DocumentImportSettings precedent)
// so CaptureSettings only mounts it. Folders on disk stay the source of
// truth for which lists EXIST; this card only edits preferences about them.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const defaultList = ref("");
// The vault's lists in effective display order (listOrder first, the rest
// alphabetical — exactly what the sections and pickers render). Reordering
// edits this array; Save persists it as the new listOrder.
const order = ref<string[]>([]);
const saveState = ref<"idle" | "saving" | "saved">("idle");
const error = ref<string | null>(null);

// Any edit after a save clears the "Saved" acknowledgement — otherwise the UI
// keeps showing "Saved" over an unpersisted change (default-list pick OR a
// reorder), so a user could navigate away thinking it was saved. Covers both
// fields uniformly; the onMounted assignment fires this while saveState is
// already "idle", a harmless no-op.
watch([defaultList, order], () => {
  if (saveState.value === "saved") saveState.value = "idle";
});

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
    // Read failures degrade to an empty card (log-only) — the save still
    // field-errors if attempted, so nothing is silently lost.
    logWarning(`task list settings load failed: ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

function move(index: number, delta: -1 | 1) {
  const target = index + delta;
  if (target < 0 || target >= order.value.length) return;
  const next = [...order.value];
  [next[index], next[target]] = [next[target], next[index]];
  order.value = next; // the [defaultList, order] watcher clears a stale "Saved"
}

async function save() {
  if (saveState.value === "saving") return;
  saveState.value = "saving";
  error.value = null;
  try {
    await invoke("set_task_lists_config", {
      id: props.vaultId,
      defaultList: defaultList.value || null,
      listOrder: order.value,
    });
    saveState.value = "saved";
  } catch (e) {
    saveState.value = "idle";
    error.value = String(e);
    logWarning(`set_task_lists_config failed: ${String(e)}`);
  }
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
          v-model="defaultList"
          :lists="order"
          :allow-create="false"
          aria-label="Default list for new tasks"
          data-testid="default-list"
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
        <div class="mt-2 flex items-center gap-2">
          <button
            type="button"
            data-testid="task-lists-save"
            :disabled="saveState === 'saving'"
            class="cursor-pointer rounded-lg bg-violet-600/80 px-2 py-0.5 text-xs font-semibold text-white hover:bg-violet-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
            @click="save"
          >
            {{ saveState === "saving" ? "Saving…" : "Save lists settings" }}
          </button>
          <span
            v-if="saveState === 'saved'"
            class="text-xs text-emerald-300"
          >Saved</span>
        </div>
        <p
          v-if="error"
          data-testid="task-lists-error"
          class="mt-1 text-xs text-red-300"
        >
          {{ error }}
        </p>
      </template>
    </div>
  </section>
</template>
