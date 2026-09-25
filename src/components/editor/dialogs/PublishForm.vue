<script setup lang="ts">
/**
 * The Publish dialog's fields (Task 48; F-43): the vault, the folder
 * inside it, the dated-folder toggle (`DestinationFields`, shared with the
 * Checks dialog's destination picker since Task 54) and the companion-note
 * choice. Presentational — `PublishDialog` owns the defaults and the
 * request; this only binds four models. A disabled form (a publish in
 * flight) is disabled field by field, so nothing can change under a
 * running copy.
 */
import type { VaultChoice } from "../../../editorTypes";
import DestinationFields from "./DestinationFields.vue";

defineProps<{ vaults: VaultChoice[]; disabled: boolean }>();

const vaultId = defineModel<string>("vaultId", { required: true });
const folder = defineModel<string>("folder", { required: true });
const dated = defineModel<boolean>("dated", { required: true });
const createNote = defineModel<boolean>("createNote", { required: true });
</script>

<template>
  <div class="flex flex-col gap-2 text-xs">
    <DestinationFields
      v-model:vault-id="vaultId"
      v-model:folder="folder"
      v-model:dated="dated"
      :vaults="vaults"
      :disabled="disabled"
      testid="publish"
    />
    <label class="flex items-center gap-2">
      <input
        v-model="createNote"
        data-testid="publish-create-note"
        type="checkbox"
        :disabled="disabled"
        class="accent-violet-500"
      >
      Write a companion note with its chapters
    </label>
  </div>
</template>
