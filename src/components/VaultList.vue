<script setup lang="ts">
import { computed } from "vue";
import type { Vault } from "../types";

const props = defineProps<{ vaults: Vault[]; busyVaultId: string | null }>();
defineEmits<{
  (e: "open-vault", id: string): void;
  (e: "open-daily-note", id: string): void;
}>();

// Obsidian allows two registered vaults whose folders share a name; without a
// disambiguator the rows would be identical while opening different vaults.
const duplicatedNames = computed(() => {
  const seen = new Set<string>();
  const dupes = new Set<string>();
  for (const vault of props.vaults) {
    const key = vault.name.toLowerCase();
    if (seen.has(key)) dupes.add(key);
    seen.add(key);
  }
  return dupes;
});

const isAmbiguous = (vault: Vault) =>
  duplicatedNames.value.has(vault.name.toLowerCase());
</script>

<template>
  <ul class="space-y-2">
    <li
      v-for="vault in vaults"
      :key="vault.id"
      class="rounded-lg bg-white/90 px-3 py-2 shadow"
      :title="vault.path"
    >
      <div class="text-sm font-semibold text-slate-800">{{ vault.name }}</div>
      <div
        v-if="isAmbiguous(vault)"
        class="truncate text-xs text-slate-500"
      >
        {{ vault.path }}
      </div>
      <div class="mt-1 flex gap-2">
        <button
          type="button"
          class="rounded bg-violet-600 px-2 py-1 text-xs text-white hover:bg-violet-500 disabled:opacity-50"
          :disabled="busyVaultId !== null"
          @click="$emit('open-vault', vault.id)"
        >
          Open vault
        </button>
        <button
          type="button"
          class="rounded bg-violet-600 px-2 py-1 text-xs text-white hover:bg-violet-500 disabled:opacity-50"
          :disabled="busyVaultId !== null"
          @click="$emit('open-daily-note', vault.id)"
        >
          Open today's daily note
        </button>
      </div>
    </li>
  </ul>
</template>
