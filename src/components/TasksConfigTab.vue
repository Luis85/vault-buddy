<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { useSettingsLoad } from "../composables/useSettingsLoad";
import type { TasksConfig } from "../types";
import TaskListSettings from "./TaskListSettings.vue";
import VaultFolderSetting from "./VaultFolderSetting.vue";

// The Tasks tab of Vault settings: the per-vault tasks folder (auto-saved via
// set_tasks_config) plus the self-contained TaskListSettings card. A failed
// folder read shows an inline error and no folder input (so a seed can't be
// saved over an unread value), but the lists card — which loads independently
// — still renders.
const props = defineProps<{ vaultId: string }>();

const { loading, loadError, load } = useSettingsLoad();
const tasksFolder = ref("");
// Last value known persisted (null = tasks root / none). A save that changes
// it remounts the lists card so its lists reload against the new root — else a
// default/order save from the stale card would target the old root.
const savedFolder = ref<string | null>(null);
const listsNonce = ref(0);
// True while the lists card has a save in flight. The folder input is disabled
// then, so a folder change can't overlap an in-flight list save and land
// old-root list preferences on the new root (Codex PR #55). Together with
// pendingFolderChange (which hides the lists card once the folder diverges),
// a folder change and a list save are mutually exclusive.
const listSaving = ref(false);

const autosave = useAutosave(
  async () => {
    const value = tasksFolder.value.trim() || null;
    await invoke("set_tasks_config", { id: props.vaultId, tasksFolder: value });
    if (value !== savedFolder.value) {
      savedFolder.value = value;
      listsNonce.value += 1;
    }
  },
  { label: "tasks folder" },
);

const taskIdEnabled = ref(false);
// Empty means "use the default"; the default name is shown as a placeholder.
const taskIdProperty = ref("");

const idAutosave = useAutosave(
  async () => {
    await invoke("set_task_id_config", {
      id: props.vaultId,
      enabled: taskIdEnabled.value,
      property: taskIdProperty.value.trim() || null,
    });
  },
  { label: "task ids" },
);

onMounted(() =>
  load<TasksConfig>("get_tasks_config", props.vaultId, (cfg) => {
    tasksFolder.value = cfg.tasksFolder ?? "";
    savedFolder.value = cfg.tasksFolder ?? null;
    taskIdEnabled.value = cfg.taskIdEnabled ?? false;
    // Show the resolved name only when the user set a non-default one, so the
    // placeholder communicates the default without pre-filling it.
    taskIdProperty.value = cfg.taskIdProperty && cfg.taskIdProperty !== "task-id" ? cfg.taskIdProperty : "";
  }),
);

function onFolderInput(value: string) {
  tasksFolder.value = value;
  autosave.schedule();
}

function onIdEnabledChange(value: boolean) {
  taskIdEnabled.value = value;
  idAutosave.saveNow();
}
function onIdPropertyInput(value: string) {
  taskIdProperty.value = value;
  idAutosave.schedule();
}

// True while the typed folder differs from what's persisted (a folder save is
// debounced/in-flight). The lists card below reads lists for the CURRENT
// persisted root, so while the root is about to change it must not be editable
// — a default/order pick then would persist old-root list names, which the
// pending set_tasks_config preserves onto the new root (Codex PR #55). When the
// folder save lands, savedFolder matches and listsNonce remounts the card
// against the new root; a failed save keeps it pending (the folder error shows).
const pendingFolderChange = computed(() => (tasksFolder.value.trim() || null) !== savedFolder.value);
</script>

<template>
  <div
    class="flex flex-col gap-3"
    @focusout="autosave.flush()"
  >
    <p
      v-if="loading"
      class="text-xs text-slate-400"
    >
      Loading…
    </p>
    <template v-else>
      <p
        v-if="loadError"
        data-testid="tasks-load-error"
        class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
      >
        {{ loadError }}
      </p>
      <VaultFolderSetting
        v-else
        :model-value="tasksFolder"
        heading="Tasks folder"
        label="Tasks folder"
        placeholder="Tasks"
        input-id="tasks-folder"
        input-testid="tasks-folder-input"
        error-testid="tasks-folder-error"
        :error="autosave.error.value"
        :disabled="listSaving"
        @update:model-value="onFolderInput"
      />
      <!-- Self-contained (own load/save); remounts on a persisted folder
           change. Hidden while a folder change is pending so it can't save
           old-root list preferences onto the about-to-change root. -->
      <TaskListSettings
        v-if="!pendingFolderChange"
        :key="listsNonce"
        :vault-id="vaultId"
        @saving-change="listSaving = $event"
      />
      <p
        v-else
        data-testid="tasks-lists-pending"
        class="rounded-xl border border-white/10 bg-white/5 p-2 text-xs text-slate-500"
      >
        List settings reload once the tasks folder is saved…
      </p>
      <section v-if="!loadError">
        <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
          Task IDs
        </h2>
        <div class="rounded-xl border border-white/10 bg-white/5 p-2">
          <div class="flex items-center justify-between gap-2">
            <label
              for="task-id-enabled"
              class="text-sm text-slate-200"
            >
              Generate an ID for each task
              <span class="block text-xs text-slate-500">Written to new tasks and stamped on the next edit</span>
            </label>
            <input
              id="task-id-enabled"
              data-testid="task-id-enabled"
              type="checkbox"
              class="h-4 w-4 accent-violet-500"
              :checked="taskIdEnabled"
              @change="onIdEnabledChange(($event.target as HTMLInputElement).checked)"
            >
          </div>
          <div
            v-if="taskIdEnabled"
            class="mt-2"
          >
            <label
              for="task-id-property"
              class="mb-1 block text-sm text-slate-200"
            >
              Property name
              <span class="block text-xs text-slate-500">Frontmatter key the ID is saved under</span>
            </label>
            <input
              id="task-id-property"
              data-testid="task-id-property"
              type="text"
              placeholder="task-id"
              class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
              :value="taskIdProperty"
              @input="onIdPropertyInput(($event.target as HTMLInputElement).value)"
              @blur="idAutosave.flush()"
            >
            <p
              v-if="idAutosave.error.value"
              data-testid="task-id-error"
              class="mt-1 text-xs text-red-300"
            >
              {{ idAutosave.error.value }}
            </p>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>
