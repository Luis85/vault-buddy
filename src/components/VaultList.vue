<script setup lang="ts">
import { computed } from "vue";
import type { Vault } from "../types";

const props = defineProps<{
  vaults: Vault[];
  busyVaultId: string | null;
  busyCommand: "open_vault" | "open_daily_note" | null;
}>();
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

const isBusy = (vault: Vault, command: "open_vault" | "open_daily_note") =>
  props.busyVaultId === vault.id && props.busyCommand === command;

// Duplicate-name vaults must also differ in their accessible names, not
// just visually — screen-reader users would otherwise hear two identical
// controls that target different vaults.
const accessibleName = (vault: Vault) =>
  isAmbiguous(vault) ? `${vault.name} (${vault.path})` : vault.name;
</script>

<template>
  <ul class="space-y-1">
    <li v-for="vault in vaults" :key="vault.id" :title="vault.path">
      <div
        class="flex items-center gap-1 rounded-lg transition-colors hover:bg-white/10"
      >
        <button
          type="button"
          class="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1.5 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:opacity-50"
          :disabled="busyVaultId !== null"
          :aria-label="`Open vault ${accessibleName(vault)}`"
          @click="$emit('open-vault', vault.id)"
        >
          <span
            class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-violet-600/80 text-xs font-bold text-white"
            aria-hidden="true"
          >
            {{ vault.name.charAt(0).toUpperCase() }}
          </span>
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm font-medium text-slate-100">
              {{ vault.name }}
            </span>
            <span
              v-if="isAmbiguous(vault)"
              class="block truncate text-xs text-slate-400"
            >
              {{ vault.path }}
            </span>
          </span>
          <span
            v-if="isBusy(vault, 'open_vault')"
            class="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-white/30 border-t-white"
            role="status"
            aria-label="Opening vault…"
          ></span>
        </button>
        <button
          type="button"
          class="mr-1 shrink-0 rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:opacity-50"
          :disabled="busyVaultId !== null"
          :aria-label="`Open today's daily note in ${accessibleName(vault)}`"
          title="Open today's daily note"
          @click="$emit('open-daily-note', vault.id)"
        >
          <span
            v-if="isBusy(vault, 'open_daily_note')"
            class="block h-4 w-4 animate-spin rounded-full border-2 border-white/30 border-t-white"
            role="status"
            aria-label="Opening daily note…"
          ></span>
          <svg
            v-else
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            aria-hidden="true"
          >
            <rect x="3" y="5" width="18" height="16" rx="2" />
            <path d="M8 3v4M16 3v4M3 11h18" />
          </svg>
        </button>
      </div>
    </li>
  </ul>
</template>
