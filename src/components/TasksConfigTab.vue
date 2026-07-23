<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { useSettingsLoad } from "../composables/useSettingsLoad";
import type { TasksConfig } from "../types";
import TaskIdSettings from "./TaskIdSettings.vue";
import TaskListSettings from "./TaskListSettings.vue";
import TaskTemplateSettings from "./TaskTemplateSettings.vue";
import Banner from "./ui/Banner.vue";
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

// The default task-id property name the backend falls back to when none is
// configured — single-sourced here so the load ternary below and the
// placeholder passed to TaskIdSettings can never drift apart.
const DEFAULT_TASK_ID_PROPERTY = "task-id";

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

// The additive task-document template (extra frontmatter + body), applied to
// every NEW task add_task creates. Its own independent field-save (the
// set_task_id_config pattern above) — a template save can't block the
// folder/lists/id saves and vice versa.
const taskExtraFrontmatter = ref("");
const taskBodyTemplate = ref("");

const templateAutosave = useAutosave(
  async () => {
    await invoke("set_task_template_config", {
      id: props.vaultId,
      extraFrontmatter: taskExtraFrontmatter.value.trim() || null,
      bodyTemplate: taskBodyTemplate.value.trim() || null,
    });
  },
  { label: "task template" },
);

onMounted(() =>
  load<TasksConfig>("get_tasks_config", props.vaultId, (cfg) => {
    tasksFolder.value = cfg.tasksFolder ?? "";
    savedFolder.value = cfg.tasksFolder ?? null;
    taskIdEnabled.value = cfg.taskIdEnabled ?? false;
    // Show the resolved name only when the user set a non-default one, so the
    // placeholder communicates the default without pre-filling it.
    taskIdProperty.value =
      cfg.taskIdProperty && cfg.taskIdProperty !== DEFAULT_TASK_ID_PROPERTY ? cfg.taskIdProperty : "";
    taskExtraFrontmatter.value = cfg.taskExtraFrontmatter ?? "";
    taskBodyTemplate.value = cfg.taskBodyTemplate ?? "";
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

function onExtraFrontmatterInput(value: string) {
  taskExtraFrontmatter.value = value;
  templateAutosave.schedule();
}
function onBodyTemplateInput(value: string) {
  taskBodyTemplate.value = value;
  templateAutosave.schedule();
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
      class="text-xs text-fg-muted"
    >
      Loading…
    </p>
    <template v-else>
      <Banner
        v-if="loadError"
        tone="danger"
        data-testid="tasks-load-error"
      >
        {{ loadError }}
      </Banner>
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
      <!-- Presentational (Task 9 review extraction) — state/autosave/load
           stay up here; the card only renders and emits raw input back. -->
      <TaskIdSettings
        v-if="!loadError"
        :enabled="taskIdEnabled"
        :property="taskIdProperty"
        :error="idAutosave.error.value"
        :placeholder="DEFAULT_TASK_ID_PROPERTY"
        @update:enabled="onIdEnabledChange"
        @update:property="onIdPropertyInput"
        @blur="idAutosave.flush()"
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
        class="rounded-xl border border-white/10 bg-white/5 p-2 text-xs text-fg-subtle"
      >
        List settings reload once the tasks folder is saved…
      </p>
      <!-- Presentational (mirrors TaskIdSettings.vue above) — state/autosave/
           load stay up here; the card only renders and emits raw input back. -->
      <TaskTemplateSettings
        v-if="!loadError"
        :extra-frontmatter="taskExtraFrontmatter"
        :body-template="taskBodyTemplate"
        @update:extra-frontmatter="onExtraFrontmatterInput"
        @update:body-template="onBodyTemplateInput"
        @blur="templateAutosave.flush()"
      />
    </template>
  </div>
</template>
