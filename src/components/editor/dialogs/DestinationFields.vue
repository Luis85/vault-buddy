<script setup lang="ts">
/**
 * A vault destination's three fields — the vault, the folder inside it and
 * the dated-folder toggle — shared by the Publish dialog (`PublishForm`,
 * Task 48) and the Checks dialog's destination picker
 * (`ChecksDestination`, Task 54), so the two can never offer the same
 * choice differently. Presentational: three models; `testid` prefixes each
 * field's test id (`<testid>-vault`, `-folder`, `-dated`).
 */
import type { VaultChoice } from "../../../editorTypes";

defineProps<{ vaults: VaultChoice[]; disabled: boolean; testid: string }>();

const vaultId = defineModel<string>("vaultId", { required: true });
const folder = defineModel<string>("folder", { required: true });
const dated = defineModel<boolean>("dated", { required: true });
</script>

<template>
  <label class="flex flex-col gap-1">
    <span class="text-fg-muted">Vault</span>
    <select
      v-model="vaultId"
      :data-testid="`${testid}-vault`"
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
      :data-testid="`${testid}-folder`"
      type="text"
      placeholder="The vault's screen-capture folder"
      :disabled="disabled"
      class="rounded-control border border-line bg-raised px-2 py-1 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
    >
  </label>
  <label class="flex items-center gap-2">
    <input
      v-model="dated"
      :data-testid="`${testid}-dated`"
      type="checkbox"
      :disabled="disabled"
      class="accent-violet-500"
    >
    Put it in a year/month folder
  </label>
</template>
