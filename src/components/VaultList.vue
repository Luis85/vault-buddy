<script setup lang="ts">
import { computed } from "vue";
import type { Vault } from "../types";

const props = defineProps<{
  vaults: Vault[];
  busyVaultId: string | null;
  busyCommand: "open_vault" | "open_daily_note" | null;
  captureDisabled: boolean;
  recordingVaultId: string | null;
}>();
defineEmits<{
  (e: "open-vault", id: string): void;
  (e: "open-daily-note", id: string): void;
  (e: "capture", id: string): void;
  (e: "capture-settings", id: string): void;
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

// Vaults currently open in Obsidian surface first, under their own header.
// With nothing open the list stays flat (no headers). Alphabetical order
// (from discovery) is preserved within each group.
const groups = computed(() => {
  const open = props.vaults.filter((v) => v.open);
  const rest = props.vaults.filter((v) => !v.open);
  if (open.length === 0) {
    return [{ key: "all", label: null as string | null, vaults: rest }];
  }
  return [
    { key: "open", label: "Open now" as string | null, vaults: open },
    { key: "rest", label: "Other vaults" as string | null, vaults: rest },
  ].filter((group) => group.vaults.length > 0);
});
</script>

<template>
  <div
    v-for="group in groups"
    :key="group.key"
    class="mt-2 first:mt-0"
  >
    <h2
      v-if="group.label"
      class="mb-1 px-2 text-[10px] font-semibold uppercase tracking-wider text-slate-500"
    >
      {{ group.label }}
    </h2>
    <ul class="space-y-1">
      <li v-for="vault in group.vaults" :key="vault.id" :title="vault.path">
      <div
        class="flex items-center gap-1 rounded-lg transition-colors hover:bg-white/10"
      >
        <button
          type="button"
          class="flex min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
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
            <span class="flex items-center gap-1.5">
              <span class="truncate text-sm font-medium text-slate-100">
                {{ vault.name }}
              </span>
              <span
                v-if="vault.open"
                class="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-400"
                title="Open in Obsidian"
                aria-hidden="true"
              ></span>
              <span
                v-if="vault.id === recordingVaultId"
                class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-red-500"
                title="Recording…"
                aria-hidden="true"
              ></span>
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
          class="mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
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
        <button
          type="button"
          class="mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null || captureDisabled"
          :aria-label="`Capture knowledge in ${accessibleName(vault)}`"
          title="Capture knowledge (record audio)"
          @click="$emit('capture', vault.id)"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            aria-hidden="true"
          >
            <rect x="9" y="2" width="6" height="12" rx="3" />
            <path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v4" />
          </svg>
        </button>
        <button
          type="button"
          class="mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null"
          :aria-label="`Capture settings for ${accessibleName(vault)}`"
          title="Capture settings"
          @click="$emit('capture-settings', vault.id)"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <circle cx="12" cy="12" r="3" />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.09a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.09a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.09a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
        </button>
      </div>
      </li>
    </ul>
  </div>
</template>
