<script setup lang="ts">
import { computed, ref } from "vue";
import { useVaultsStore } from "../stores/vaults";
import VaultList from "./VaultList.vue";

const store = useVaultsStore();

const filter = ref("");
// A short list is scannable at a glance; only offer filtering when the
// list is long enough that scanning stops working.
const FILTER_THRESHOLD = 5;
const showFilter = computed(() => store.vaults.length > FILTER_THRESHOLD);
const filtered = computed(() => {
  const query = filter.value.trim().toLowerCase();
  if (!query) return store.vaults;
  return store.vaults.filter(
    (v) =>
      v.name.toLowerCase().includes(query) ||
      v.path.toLowerCase().includes(query),
  );
});

function onFilterEscape(event: KeyboardEvent) {
  if (filter.value) {
    // first Escape clears the filter; a second one bubbles up and closes
    filter.value = "";
    event.stopPropagation();
  }
}
</script>

<template>
  <div
    class="flex h-full w-full flex-col rounded-2xl border border-white/10 bg-slate-900/90 p-3 shadow-[0_2px_6px_rgba(0,0,0,0.35)] backdrop-blur"
  >
    <div class="mb-2 flex items-center justify-between">
      <h1 class="text-sm font-bold text-slate-100">Vaults</h1>
      <span
        v-if="store.vaults.length > 0"
        class="rounded-full bg-white/10 px-2 py-0.5 text-xs text-slate-300"
      >
        {{ store.vaults.length }}
      </span>
    </div>
    <input
      v-if="showFilter"
      v-model="filter"
      type="search"
      placeholder="Filter vaults…"
      aria-label="Filter vaults"
      class="mb-2 w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      @keydown.escape="onFilterEscape"
    />
    <p
      v-if="store.error"
      class="mb-2 rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ store.error }}
    </p>
    <div class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1">
      <VaultList
        v-if="filtered.length > 0"
        :vaults="filtered"
        :busy-vault-id="store.busyVaultId"
        :busy-command="store.busyCommand"
        @open-vault="store.runAction('open_vault', $event)"
        @open-daily-note="store.runAction('open_daily_note', $event)"
      />
      <p v-else-if="store.vaults.length > 0" class="text-xs text-slate-400">
        No vaults match "{{ filter }}".
      </p>
      <p v-else-if="store.loaded" class="text-xs text-slate-400">
        Obsidian not found — no vaults discovered. Is Obsidian installed and
        has it been opened at least once?
      </p>
    </div>
  </div>
</template>
