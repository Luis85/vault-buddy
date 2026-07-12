<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { TasksConfig } from "../types";
import TaskListSettings from "./TaskListSettings.vue";
import VaultFolderSetting from "./VaultFolderSetting.vue";

// The Tasks tab of Vault settings: the per-vault tasks folder (auto-saved via
// set_tasks_config) plus the self-contained TaskListSettings card. A failed
// folder read shows an inline error and no folder input (so a seed can't be
// saved over an unread value), but the lists card — which loads independently
// — still renders.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasksFolder = ref("");
// Last value known persisted (null = tasks root / none). A save that changes
// it remounts the lists card so its lists reload against the new root — else a
// default/order save from the stale card would target the old root.
const savedFolder = ref<string | null>(null);
const listsNonce = ref(0);

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

onMounted(async () => {
  try {
    const cfg = await invoke<TasksConfig>("get_tasks_config", { id: props.vaultId });
    tasksFolder.value = cfg.tasksFolder ?? "";
    savedFolder.value = cfg.tasksFolder ?? null;
  } catch (e) {
    loadError.value = String(e);
    logWarning(`get_tasks_config failed (vault ${props.vaultId}): ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

function onFolderInput(value: string) {
  tasksFolder.value = value;
  autosave.schedule();
}
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
        @update:model-value="onFolderInput"
      />
      <!-- Self-contained (own load/save); remounts on a persisted folder change. -->
      <TaskListSettings
        :key="listsNonce"
        :vault-id="vaultId"
      />
    </template>
  </div>
</template>
