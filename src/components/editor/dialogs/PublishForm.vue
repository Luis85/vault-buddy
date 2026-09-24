<script setup lang="ts">
/**
 * The Publish dialog's fields (Task 48; F-43): the vault, the folder
 * inside it, the dated-folder toggle and the companion-note choice.
 * Presentational — `PublishDialog` owns the defaults and the request; this
 * only binds four models. A disabled form (a publish in flight) is
 * disabled field by field, so nothing can change under a running copy.
 */
import type { VaultChoice } from "../../../editorTypes";

defineProps<{ vaults: VaultChoice[]; disabled: boolean }>();

const vaultId = defineModel<string>("vaultId", { required: true });
const folder = defineModel<string>("folder", { required: true });
const dated = defineModel<boolean>("dated", { required: true });
const createNote = defineModel<boolean>("createNote", { required: true });
</script>

<template>
  <div class="flex flex-col gap-2 text-xs">
    <label class="flex flex-col gap-1">
      <span class="text-fg-muted">Vault</span>
      <select
        v-model="vaultId"
        data-testid="publish-vault"
        :disabled="disabled"
        class="rounded-control border border-line bg-raised px-2 py-1 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      >
        <option
          value=""
          disabled
        >
          Choose a vault…
        </option>
        <option
          v-for="vault in vaults"
          :key="vault.id"
          :value="vault.id"
        >
          {{ vault.name }}
        </option>
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class="text-fg-muted">Folder in the vault</span>
      <input
        v-model="folder"
        data-testid="publish-folder"
        type="text"
        placeholder="The vault's screen-capture folder"
        :disabled="disabled"
        class="rounded-control border border-line bg-raised px-2 py-1 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      >
    </label>
    <label class="flex items-center gap-2">
      <input
        v-model="dated"
        data-testid="publish-dated"
        type="checkbox"
        :disabled="disabled"
        class="accent-violet-500"
      >
      Put it in a year/month folder
    </label>
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
