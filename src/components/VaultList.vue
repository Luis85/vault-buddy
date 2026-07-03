<script setup lang="ts">
import type { Vault } from "../types";

defineProps<{ vaults: Vault[]; busyVaultId: string | null }>();
defineEmits<{
  (e: "open-vault", id: string): void;
  (e: "open-daily-note", id: string): void;
}>();
</script>

<template>
  <ul class="space-y-2">
    <li
      v-for="vault in vaults"
      :key="vault.id"
      class="rounded-lg bg-white/90 px-3 py-2 shadow"
    >
      <div class="text-sm font-semibold text-slate-800">{{ vault.name }}</div>
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
