<script setup lang="ts">
import type { Vault } from "../types";

// The vault-row list, extracted out of ImportVaultPicker.vue's template.
// That template carried no internal branching in the row itself — the `<li
// v-for>` was the only local complexity to shed — so moving the loop here
// (rather than just the row markup, the TaskRow.vue precedent) is what
// actually removes a branch from the parent's own fallow template-complexity
// count; Task 3's added filter block had pushed it just over the CRAP
// threshold. Presentational only: the parent still owns viewState,
// filtering, and favorites ordering, and reacts to the emitted pick.
defineProps<{ vaults: Vault[] }>();
defineEmits<{ (e: "pick", id: string): void }>();
</script>

<template>
  <ul class="space-y-1">
    <li
      v-for="vault in vaults"
      :key="vault.id"
    >
      <button
        type="button"
        data-testid="import-picker-vault"
        class="flex w-full cursor-pointer items-center gap-2 rounded-control border border-white/10 bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="$emit('pick', vault.id)"
      >
        <span class="min-w-0 flex-1 truncate text-sm font-medium text-fg">
          {{ vault.name }}
        </span>
      </button>
    </li>
  </ul>
</template>
