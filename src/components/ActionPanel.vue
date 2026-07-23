<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onMounted, onUnmounted, watch } from "vue";

import { useVaultFilter } from "../composables/useVaultFilter";
import { useCaptureStore } from "../stores/capture";
import { useDocumentImportsStore } from "../stores/documentImports";
import { useSettingsStatusStore } from "../stores/settingsStatus";
import { useVaultsStore } from "../stores/vaults";
import AppIcon from "./AppIcon.vue";
import BuddySettings from "./BuddySettings.vue";
import CaptureSettings from "./CaptureSettings.vue";
import DocumentImportSettings from "./DocumentImportSettings.vue";
import ImportProgress from "./ImportProgress.vue";
import ImportVaultPicker from "./ImportVaultPicker.vue";
import NotificationHost from "./NotificationHost.vue";
import RecordingBar from "./RecordingBar.vue";
import Recordings from "./Recordings.vue";
import RecordMode from "./RecordMode.vue";
import RenamePrompt from "./RenamePrompt.vue";
import Search from "./Search.vue";
import Tasks from "./Tasks.vue";
import Transcriptions from "./Transcriptions.vue";
import TranscriptionSummary from "./TranscriptionSummary.vue";
import Banner from "./ui/Banner.vue";
import Chip from "./ui/Chip.vue";
import CountBadge from "./ui/CountBadge.vue";
import EmptyState from "./ui/EmptyState.vue";
import Field from "./ui/Field.vue";
import IconButton from "./ui/IconButton.vue";
import UpdateView from "./UpdateView.vue";
import VaultList from "./VaultList.vue";

const store = useVaultsStore();
const capture = useCaptureStore();
const documentImports = useDocumentImportsStore();

// store-backed so a failed update install can reopen the (destroyed)
// panel directly on the settings view
const { view } = storeToRefs(store);

// The shared auto-save status, shown as a transient indicator beside the title
// while in a settings view (Buddy or Vault settings).
const saveStatus = useSettingsStatusStore();
const isSettingsView = computed(
  () => view.value === "settings" || view.value === "captureSettings",
);
const saveStatusLabel = computed(() => {
  if (saveStatus.state === "saving") return "Saving…";
  if (saveStatus.state === "saved") return "Saved ✓";
  if (saveStatus.state === "error") return "⚠ Couldn't save";
  return "";
});
// A stale saving/saved/error must not linger when navigating between views.
watch(
  () => store.view,
  () => saveStatus.reset(),
);

// One line per view; the fallback is the vault list's title.
const VIEW_TITLES: Record<string, string> = {
  settings: "Buddy settings",
  captureSettings: "Vault settings",
  recordings: "Recordings",
  recordMode: "Capture knowledge",
  transcriptions: "Transcriptions",
  tasks: "Tasks",
  search: "Search",
  importPicker: "Import document",
  documentImport: "Document import",
  update: "Update",
};
// The tasks view is dual-mode: a null vault id is the cross-vault aggregate.
const title = computed(() =>
  view.value === "tasks" && store.tasksVaultId === null
    ? "All tasks"
    : (VIEW_TITLES[view.value] ?? "Vaults"),
);

// `/` jumps to search from the vault list — unless the keystroke is going
// into a text field (the vault filter must keep receiving "/"). Ctrl/Cmd+F
// does the same regardless of focus and suppresses the WebView find bar.
// Gated on the list view so the search input, settings forms, and the task
// composer never lose keystrokes to a global handler.
function onPanelKeydown(event: KeyboardEvent) {
  if (view.value !== "list") return;
  const inText =
    event.target instanceof HTMLInputElement ||
    event.target instanceof HTMLTextAreaElement;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
    event.preventDefault();
    store.openSearch();
  } else if (
    event.key === "/" &&
    !inText &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.altKey
  ) {
    event.preventDefault();
    store.openSearch();
  }
}
onMounted(() => window.addEventListener("keydown", onPanelKeydown));
onUnmounted(() => window.removeEventListener("keydown", onPanelKeydown));

// Shared with ImportVaultPicker's filter (useVaultFilter) — this view's
// showFilter additionally gates on the list view being active.
const { filter, aboveThreshold, filtered, onFilterEscape } = useVaultFilter(
  () => store.vaults,
);
const showFilter = computed(() => view.value === "list" && aboveThreshold.value);
const totalOpenTasks = computed(() =>
  Object.values(store.taskCounts).reduce((a, b) => a + b, 0),
);
const tasksKey = computed(() => store.tasksVaultId ?? "all");

// The panel window is only hidden/shown, not unmounted, so onUnmounted no
// longer fires on close and transient UI used to survive a close-and-reopen.
// `shownNonce` bumps each time Rust re-shows the panel (see PanelRoot /
// toggle_panel's panel-shown event): treat it as the reopen signal and clear
// what a close used to reset — the filter text and a STALE rename prompt
// only (GAP-29: a tray-stopped recording arms `lastSaved` while the panel is
// hidden, and an unconditional dismiss here killed that prompt before it
// ever rendered — the 30 s window only worked if the panel was already
// open). `dismissRenameIfStale` keeps anything younger than
// RENAME_PROMPT_MS. (The record chooser is now a store-owned view, reset by
// `refresh`/`showList`, so it needs no local teardown here.)
watch(
  () => store.shownNonce,
  () => {
    filter.value = "";
    capture.dismissRenameIfStale();
  },
);
</script>

<template>
  <div
    class="relative flex h-full w-full flex-col rounded-2xl border border-white/10 bg-slate-900/90 p-3 shadow-[0_2px_6px_rgba(0,0,0,0.35)] backdrop-blur"
  >
    <div class="mb-2 flex items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <h1 class="truncate text-sm font-bold text-slate-100">
          {{ title }}
        </h1>
        <span
          v-if="isSettingsView && saveStatus.state !== 'idle'"
          data-testid="save-status"
          role="status"
          aria-live="polite"
          class="shrink-0 text-xs"
          :class="{
            'text-slate-400': saveStatus.state === 'saving',
            'text-emerald-300': saveStatus.state === 'saved',
            'text-red-300': saveStatus.state === 'error',
          }"
        >{{ saveStatusLabel }}</span>
      </div>
      <div class="flex shrink-0 items-center gap-2">
        <Chip v-if="view === 'list' && store.vaults.length > 0">
          {{ store.vaults.length }}
        </Chip>
        <IconButton
          v-if="view === 'list' && store.vaults.length > 0"
          :label="`All tasks across every vault${totalOpenTasks > 0 ? ` — ${totalOpenTasks} open` : ''}`"
          title="All tasks"
          data-testid="all-tasks"
          class="relative"
          @click="store.openAllTasks()"
        >
          <AppIcon>
            <path d="M9 11l3 3 8-8" />
            <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
          </AppIcon>
          <!-- Corner badge like VaultList's per-vault Tasks button — widens
               with digits (capped at 99+), never grows the header row. -->
          <CountBadge
            :count="totalOpenTasks"
            data-testid="all-tasks-count"
            class="absolute -right-0.5 -top-0.5"
          />
        </IconButton>
        <IconButton
          v-if="view === 'list'"
          label="Search vaults"
          title="Search vaults"
          data-testid="search-toggle"
          @click="store.openSearch()"
        >
          <AppIcon>
            <circle
              cx="11"
              cy="11"
              r="8"
            />
            <path d="m21 21-4.35-4.35" />
          </AppIcon>
        </IconButton>
        <IconButton
          v-if="view === 'list'"
          label="Buddy settings"
          title="Buddy settings"
          data-testid="settings-toggle"
          @click="store.openSettings()"
        >
          <AppIcon>
            <circle
              cx="12"
              cy="12"
              r="3"
            />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.09a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.09a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.09a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </AppIcon>
        </IconButton>
        <IconButton
          v-else
          label="Back"
          title="Back"
          data-testid="back-button"
          @click="store.back()"
        >
          <AppIcon>
            <path d="M19 12H5M12 19l-7-7 7-7" />
          </AppIcon>
        </IconButton>
      </div>
    </div>
    <Field
      v-if="showFilter"
      v-model="filter"
      type="search"
      placeholder="Filter vaults…"
      aria-label="Filter vaults"
      class="mb-2"
      @keydown.escape="onFilterEscape"
    />
    <Banner
      v-if="view === 'list' && store.error"
      tone="danger"
      class="mb-2"
    >
      {{ store.error }}
    </Banner>
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
    <TranscriptionSummary
      v-if="view === 'list'"
      class="mb-2"
    />
    <!-- A running document import stays visible after leaving the intake
         views (or a panel reopen landing on the list default) — the same
         list-view visibility RecordingBar/TranscriptionSummary give their
         domains' background work. Gated on `active` so the card's elapsed
         tick only runs while a conversion is actually in flight. -->
    <ImportProgress
      v-if="view === 'list' && documentImports.active"
      class="mb-2"
    />
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
    <div
      v-else-if="view === 'transcriptions'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Transcriptions />
    </div>
    <div
      v-else-if="view === 'tasks'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Tasks
        :key="tasksKey"
        :vault-id="store.tasksVaultId"
      />
    </div>
    <div
      v-else-if="view === 'search'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Search />
    </div>
    <div
      v-else-if="view === 'importPicker'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <ImportVaultPicker />
    </div>
    <div
      v-else-if="view === 'documentImport'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <p class="mb-2 text-xs text-slate-400">
        Vault Buddy converts Word, ODT, and RTF files into notes using Pandoc —
        set it up here, then import from a vault's Capture knowledge screen.
      </p>
      <DocumentImportSettings />
    </div>
    <div
      v-else-if="view === 'update'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <UpdateView />
    </div>
    <div
      v-else
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <VaultList
        v-if="filtered.length > 0"
        :vaults="filtered"
        :busy-vault-id="store.busyVaultId"
        :busy-command="store.busyCommand"
        :capture-disabled="capture.status !== 'idle'"
        :recording-vault-id="capture.vaultId"
        :transcribing-vault-id="capture.transcribingVaultId"
        :task-counts="store.taskCounts"
        @open-vault="store.runAction('open_vault', $event)"
        @open-daily-note="store.runAction('open_daily_note', $event)"
        @capture="store.openRecordMode($event)"
        @capture-settings="store.openCaptureSettings($event)"
        @open-tasks="store.openTasks($event)"
      />
      <EmptyState
        v-else-if="store.vaults.length > 0"
        :title="`No vaults match &quot;${filter}&quot;.`"
      />
      <EmptyState
        v-else-if="store.loaded"
        title="Obsidian not found — no vaults discovered. Is Obsidian installed and has it been opened at least once?"
      >
        <template #icon>
          <AppIcon :size="28">
            <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2Z" />
          </AppIcon>
        </template>
      </EmptyState>
    </div>
    <NotificationHost />
  </div>
</template>
