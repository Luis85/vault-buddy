<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { DocumentsConfig } from "../types";
import VaultFolderSetting from "./VaultFolderSetting.vue";

// The Documents tab of Vault settings. Self-contained: loads its own config,
// auto-saves both fields (folder + date-folders toggle) through the one
// set_documents_config command. A failed read shows an inline error and NO
// editable fields, so a seeded default can never be auto-saved over a value we
// failed to read.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const documentsFolder = ref("");
const documentDateFolders = ref(true);

const autosave = useAutosave(
  async () => {
    await invoke("set_documents_config", {
      id: props.vaultId,
      documentsFolder: documentsFolder.value.trim() || null,
      documentDateFolders: documentDateFolders.value,
    });
  },
  { label: "documents settings" },
);

onMounted(async () => {
  try {
    const cfg = await invoke<DocumentsConfig>("get_documents_config", { id: props.vaultId });
    documentsFolder.value = cfg.documentsFolder ?? "";
    documentDateFolders.value = cfg.documentDateFolders;
  } catch (e) {
    loadError.value = String(e);
    logWarning(`get_documents_config failed (vault ${props.vaultId}): ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

// Typed folder edits debounce; the toggle saves immediately. onMounted assigns
// the refs directly (not via these handlers), so neither fires on load.
function onFolderInput(value: string) {
  documentsFolder.value = value;
  autosave.schedule();
}
function onToggle(event: Event) {
  documentDateFolders.value = (event.target as HTMLInputElement).checked;
  autosave.saveNow();
}
</script>

<template>
  <!-- focusout flushes a pending debounced folder save when focus leaves. -->
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
    <p
      v-else-if="loadError"
      data-testid="documents-load-error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <template v-else>
      <VaultFolderSetting
        :model-value="documentsFolder"
        heading="Documents folder"
        label="Documents folder"
        placeholder="Documents"
        input-id="documents-folder"
        input-testid="documents-folder-input"
        error-testid="documents-folder-error"
        :error="autosave.error.value"
        @update:model-value="onFolderInput"
      />
      <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          for="document-date-folders"
          class="text-sm text-slate-200"
        >
          Organize into year/month folders
          <span class="block text-xs text-slate-500">Off = one flat folder</span>
        </label>
        <input
          id="document-date-folders"
          data-testid="document-date-folders-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="documentDateFolders"
          @change="onToggle"
        >
      </div>
    </template>
  </div>
</template>
