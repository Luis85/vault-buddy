<script setup lang="ts">
/**
 * A vault destination's three fields — the vault, the folder inside it and
 * the dated-folder toggle — shared by the Publish dialog (`PublishForm`,
 * Task 48) and the Checks dialog's destination picker
 * (`ChecksDestination`, Task 54), so the two can never offer the same
 * choice differently. Presentational: three models; `testid` prefixes each
 * field's test id (`<testid>-vault`, `-folder`, `-dated`).
 *
 * Visual-parity Task 21: the concept's `.fieldpair` of `.field`s
 * ("Destination vault" / "Folder inside vault", screen 09), the vaults by
 * NAME (design D6), and the editor's base input look (`style.css`).
 */
import type { VaultChoice } from "../../../editorTypes";

defineProps<{ vaults: VaultChoice[]; disabled: boolean; testid: string }>();

const vaultId = defineModel<string>("vaultId", { required: true });
const folder = defineModel<string>("folder", { required: true });
const dated = defineModel<boolean>("dated", { required: true });
</script>

<template>
  <div class="grid grid-cols-2 gap-[9px] max-[520px]:grid-cols-1">
    <label class="flex min-w-0 flex-col gap-[5px] text-[10px] text-fg-secondary">
      Destination vault
      <select
        v-model="vaultId"
        :data-testid="`${testid}-vault`"
        :disabled="disabled"
        class="text-xs"
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
    <label class="flex min-w-0 flex-col gap-[5px] text-[10px] text-fg-secondary">
      Folder inside vault
      <input
        v-model="folder"
        :data-testid="`${testid}-folder`"
        type="text"
        placeholder="The vault's screen-capture folder"
        :disabled="disabled"
        class="text-xs"
      >
    </label>
  </div>
  <label class="flex items-center gap-2 text-[11px] text-fg">
    <input
      v-model="dated"
      :data-testid="`${testid}-dated`"
      type="checkbox"
      :disabled="disabled"
    >
    Put it in a year/month folder
  </label>
</template>
