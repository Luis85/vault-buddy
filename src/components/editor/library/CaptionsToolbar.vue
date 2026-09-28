<script setup lang="ts">
/**
 * `CaptionsLibrary`'s two actions (Task 36; visual-parity Task 10, concept
 * spec §3.4): **Add caption** and **Import SRT / VTT**, full-width, plus
 * the import's "replace" choice. Presentational: each button is disabled
 * by, and titled with, the reason its parent computed (`captionRules`
 * drafts, R20: a disabled control says why), and a click is only an emit.
 * Splitting lives on each caption card ("Split cue").
 */
import { computed } from "vue";

import LibraryButton from "./LibraryButton.vue";

const props = defineProps<{
  importReason: string | null;
  importing: boolean;
  addReason: string | null;
}>();

const replace = defineModel<boolean>("replace", { required: true });

const emit = defineEmits<{
  (e: "import"): void;
  (e: "add"): void;
}>();

const importReasonShown = computed(() => (props.importing ? "An import is already running" : props.importReason));
</script>

<template>
  <div class="flex flex-col gap-2">
    <LibraryButton
      icon="plus"
      data-testid="caption-add"
      :disabled="addReason !== null"
      :title="addReason ?? 'Add a caption at the playhead'"
      @click="emit('add')"
    >
      Add caption
    </LibraryButton>
    <LibraryButton
      icon="upload"
      data-testid="caption-import"
      :disabled="importReasonShown !== null"
      :title="importReasonShown ?? 'Import SRT / WebVTT onto the selected clip'"
      @click="emit('import')"
    >
      {{ importing ? "Importing…" : "Import SRT / VTT" }}
    </LibraryButton>
    <label class="flex items-center gap-2 text-[10px] text-fg-secondary">
      <input
        v-model="replace"
        data-testid="caption-import-replace"
        type="checkbox"
      >
      Replace this clip's captions on import
    </label>
  </div>
</template>
