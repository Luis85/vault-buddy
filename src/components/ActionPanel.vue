<script setup lang="ts">
import { computed, ref } from "vue";
import { useVaultsStore } from "../stores/vaults";
import { useCaptureStore } from "../stores/capture";
import VaultList from "./VaultList.vue";
import BuddySettings from "./BuddySettings.vue";
import RecordingBar from "./RecordingBar.vue";

const store = useVaultsStore();
const capture = useCaptureStore();

const showSettings = ref(false);

const filter = ref("");
// A short list is scannable at a glance; only offer filtering when the
// list is long enough that scanning stops working.
const FILTER_THRESHOLD = 5;
const showFilter = computed(
  () => !showSettings.value && store.vaults.length > FILTER_THRESHOLD,
);
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
      <h1 class="text-sm font-bold text-slate-100">
        {{ showSettings ? "Buddy settings" : "Vaults" }}
      </h1>
      <div class="flex items-center gap-2">
        <span
          v-if="!showSettings && store.vaults.length > 0"
          class="rounded-full bg-white/10 px-2 py-0.5 text-xs text-slate-300"
        >
          {{ store.vaults.length }}
        </span>
        <button
          type="button"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="{ 'text-violet-300': showSettings }"
          :aria-label="showSettings ? 'Back to vaults' : 'Buddy settings'"
          :aria-pressed="showSettings"
          :title="showSettings ? 'Back to vaults' : 'Buddy settings'"
          data-testid="settings-toggle"
          @click="showSettings = !showSettings"
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
      v-if="!showSettings && store.error"
      class="mb-2 rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ store.error }}
    </p>
    <RecordingBar
      v-if="!showSettings && capture.status !== 'idle'"
      class="mb-2"
      :started-at-ms="capture.startedAtMs"
      :saving="capture.status === 'saving'"
      :warning="capture.warning"
      @stop="capture.stop()"
    />
    <p
      v-if="!showSettings && capture.error"
      class="mb-2 rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ capture.error }}
    </p>
    <div
      v-if="showSettings"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <BuddySettings />
    </div>
    <div v-else class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1">
      <VaultList
        v-if="filtered.length > 0"
        :vaults="filtered"
        :busy-vault-id="store.busyVaultId"
        :busy-command="store.busyCommand"
        :capture-disabled="capture.status !== 'idle'"
        @open-vault="store.runAction('open_vault', $event)"
        @open-daily-note="store.runAction('open_daily_note', $event)"
        @capture="capture.start($event)"
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
