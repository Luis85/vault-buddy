<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { useVaultsStore } from "../stores/vaults";
import { useCaptureStore } from "../stores/capture";
import VaultList from "./VaultList.vue";
import BuddySettings from "./BuddySettings.vue";
import CaptureSettings from "./CaptureSettings.vue";
import RecordingBar from "./RecordingBar.vue";
import TranscriptionStatus from "./TranscriptionStatus.vue";
import RenamePrompt from "./RenamePrompt.vue";
import RecordMode from "./RecordMode.vue";
import Recordings from "./Recordings.vue";

const store = useVaultsStore();
const capture = useCaptureStore();

// store-backed so a failed update install can reopen the (destroyed)
// panel directly on the settings view
const { view } = storeToRefs(store);

const filter = ref("");
// A short list is scannable at a glance; only offer filtering when the
// list is long enough that scanning stops working.
const FILTER_THRESHOLD = 5;
const showFilter = computed(
  () => view.value === "list" && store.vaults.length > FILTER_THRESHOLD,
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

// The panel window is only hidden/shown, not unmounted, so onUnmounted no
// longer fires on close and transient UI used to survive a close-and-reopen.
// `shownNonce` bumps each time Rust re-shows the panel (see PanelRoot /
// toggle_panel's panel-shown event): treat it as the reopen signal and clear
// what a close used to reset — the filter text and a lingering post-save
// rename prompt. (The record chooser is now a store-owned view, reset by
// `refresh`/`showList`, so it needs no local teardown here.)
watch(
  () => store.shownNonce,
  () => {
    filter.value = "";
    capture.dismissRename();
  },
);
</script>

<template>
  <div
    class="relative flex h-full w-full flex-col rounded-2xl border border-white/10 bg-slate-900/90 p-3 shadow-[0_2px_6px_rgba(0,0,0,0.35)] backdrop-blur"
  >
    <div class="mb-2 flex items-center justify-between">
      <h1 class="text-sm font-bold text-slate-100">
        {{
          view === "settings"
            ? "Buddy settings"
            : view === "captureSettings"
              ? "Capture settings"
              : view === "recordings"
                ? "Recordings"
                : view === "recordMode"
                  ? "Record"
                  : "Vaults"
        }}
      </h1>
      <div class="flex items-center gap-2">
        <span
          v-if="view === 'list' && store.vaults.length > 0"
          class="rounded-full bg-white/10 px-2 py-0.5 text-xs text-slate-300"
        >
          {{ store.vaults.length }}
        </span>
        <button
          v-if="view === 'list'"
          type="button"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          aria-label="Buddy settings"
          title="Buddy settings"
          data-testid="settings-toggle"
          @click="store.openSettings()"
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
        <button
          v-else
          type="button"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          aria-label="Back"
          title="Back"
          data-testid="back-button"
          @click="store.back()"
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
            <path d="M19 12H5M12 19l-7-7 7-7" />
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
      v-if="view === 'list' && store.error"
      class="mb-2 rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ store.error }}
    </p>
    <RecordingBar
      v-if="view === 'list' && capture.status !== 'idle'"
      class="mb-2"
      :started-at-ms="capture.startedAtMs"
      :saving="capture.status === 'saving'"
      :starting="capture.status === 'starting'"
      :warning="capture.warning"
      :paused="capture.paused"
      :paused-total-ms="capture.pausedTotalMs"
      :paused-since-ms="capture.pausedSinceMs"
      :level="capture.level"
      @stop="capture.stop()"
      @pause="capture.pause()"
      @resume="capture.resume()"
    />
    <p
      v-if="view === 'list' && capture.error"
      class="mb-2 rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ capture.error }}
    </p>
    <TranscriptionStatus v-if="view === 'list'" class="mb-2" />
    <p
      v-if="view === 'list' && capture.status === 'idle' && capture.warning"
      class="mb-2 rounded-lg bg-amber-500/15 px-2 py-1 text-xs text-amber-200"
    >
      {{ capture.warning }}
    </p>
    <RenamePrompt
      v-if="view === 'list' && capture.lastSaved"
      class="mb-2"
      :saved-mp3="capture.lastSaved.mp3"
      :error="capture.renameError"
      @accept="capture.acceptRename($event)"
    />
    <div
      v-if="view === 'settings'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <BuddySettings />
    </div>
    <div
      v-else-if="view === 'captureSettings' && store.captureSettingsVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <CaptureSettings
        :key="store.captureSettingsVaultId"
        :vault-id="store.captureSettingsVaultId"
      />
    </div>
    <div
      v-else-if="view === 'recordings' && store.recordingsVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Recordings
        :key="store.recordingsVaultId"
        :vault-id="store.recordingsVaultId"
      />
    </div>
    <div
      v-else-if="view === 'recordMode' && store.recordModeVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <RecordMode
        :key="store.recordModeVaultId"
        :vault-id="store.recordModeVaultId"
      />
    </div>
    <div v-else class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1">
      <VaultList
        v-if="filtered.length > 0"
        :vaults="filtered"
        :busy-vault-id="store.busyVaultId"
        :busy-command="store.busyCommand"
        :capture-disabled="capture.status !== 'idle'"
        :recording-vault-id="capture.vaultId"
        :transcribing-vault-id="capture.transcribingVaultId"
        @open-vault="store.runAction('open_vault', $event)"
        @open-daily-note="store.runAction('open_daily_note', $event)"
        @capture="store.openRecordMode($event)"
        @capture-settings="store.openCaptureSettings($event)"
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
