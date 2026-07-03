<script setup lang="ts">
import { useVaultsStore } from "../stores/vaults";
import VaultList from "./VaultList.vue";

const store = useVaultsStore();
</script>

<template>
  <div class="h-full w-64 overflow-y-auto rounded-xl bg-slate-100/95 p-3 shadow-xl">
    <h1 class="mb-2 text-sm font-bold text-slate-700">Your vaults</h1>
    <p
      v-if="store.error"
      class="mb-2 rounded bg-red-100 px-2 py-1 text-xs text-red-700"
    >
      {{ store.error }}
    </p>
    <VaultList
      v-if="store.vaults.length > 0"
      :vaults="store.vaults"
      :busy-vault-id="store.busyVaultId"
      @open-vault="store.runAction('open_vault', $event)"
      @open-daily-note="store.runAction('open_daily_note', $event)"
    />
    <p v-else-if="store.loaded" class="text-xs text-slate-600">
      Obsidian not found — no vaults discovered. Is Obsidian installed and has
      it been opened at least once?
    </p>
  </div>
</template>
