<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useCaptureStore } from "../stores/capture";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { CaptureConfig, PandocStatus, Recording } from "../types";
import TranscriptionSettings from "./TranscriptionSettings.vue";

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const capture = useCaptureStore();
const notifications = useNotificationsStore();

const OPTIONS = [
  { key: "meeting", title: "Meeting", hint: "Microphone + desktop audio", testId: "mode-meeting" },
  { key: "voice-note", title: "Voice Note", hint: "Microphone only", testId: "mode-voice-note" },
] as const;

// Gates persist() (not rendering) until the vault's real config has landed
// (set in loadConfig's `finally`, so it also flips on a failed read — see
// below). TranscriptionSettings renders immediately against the defaults
// below so recording is never blocked on the read, but a toggle made before
// the read resolves must only update the local control, not hit disk: the
// default-seeded `config` would otherwise persist() over the vault's real
// recordingFolder/bitrateKbps/devices/createNote/followUpTemplate, and the
// in-flight read would then clobber the toggle right back out of
// `config.value` anyway. Mirrors CaptureSettings.vue's loading/finally gate.
const loaded = ref(false);

// Full vault capture config, seeded with the same fallback values
// CaptureSettings.vue's refs default to. A read failure below must never
// block recording, so these settings stay editable — and savable — against
// the defaults even then.
const config = ref<CaptureConfig>({
  mode: "meeting",
  recordingFolder: null,
  bitrateKbps: 128,
  createNote: true,
  followUpTemplate: true,
  inputDevice: null,
  outputDevice: null,
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: null,
  transcriptTimestamps: true,
});

// How many recordings the vault already holds, for the Browse card's pill.
// null = unknown (still loading, or the scan failed) → the pill stays
// hidden; a real 0 IS shown — an empty vault is worth knowing before
// clicking through.
const recordingCount = ref<number | null>(null);

const browseLabel = computed(() =>
  recordingCount.value === null
    ? "Browse past recordings"
    : `Browse past recordings (${recordingCount.value} in this vault)`,
);

// App-global Pandoc install status, null until detect_pandoc resolves (or
// forever on a failed/absent Tauri runtime — see detectPandoc below). A null
// status is treated the same as "not installed": the Import button stays
// disabled rather than optimistically enabled against unknown state.
const pandoc = ref<PandocStatus | null>(null);

// Single computed (not two) so the "not installed" check isn't duplicated
// between a disabled-flag computed and a hint-text computed.
const importStatus = computed(() => {
  if (!pandoc.value?.installed) {
    return { disabled: true, hint: "Install Pandoc in Settings to import documents" };
  }
  if (!pandoc.value.sandboxSupported) {
    return { disabled: true, hint: "Update Pandoc (2.15+ needed)" };
  }
  return { disabled: false, hint: "Convert a Word, ODT, or RTF file into a note" };
});

// Bundles the four transcription fields for TranscriptionSettings' v-model.
// The setter merges the change back into the FULL loaded config (preserving
// mode/folder/bitrate/devices/etc. untouched) and persists it — same
// command + arg shape as CaptureSettings.vue's save().
const transcription = computed({
  get: () => ({
    transcribe: config.value.transcribe,
    transcriptionModel: config.value.transcriptionModel,
    transcriptionLanguage: config.value.transcriptionLanguage ?? "",
    transcriptTimestamps: config.value.transcriptTimestamps,
  }),
  set: (v: {
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
  }) => {
    config.value = {
      ...config.value,
      transcribe: v.transcribe,
      transcriptionModel: v.transcriptionModel,
      transcriptionLanguage: v.transcriptionLanguage.trim() || null,
      transcriptTimestamps: v.transcriptTimestamps,
    };
    // Never persist against the default-seeded config — see `loaded` above.
    if (loaded.value) void persist();
  },
});

async function persist() {
  try {
    await invoke("set_capture_config", { id: props.vaultId, cfg: config.value });
  } catch (e) {
    // RecordMode has no settings-save UI of its own (unlike CaptureSettings'
    // Save button + error banner) — the vault's full Capture Settings view is
    // where the user can see the error and retry, but a failed save must
    // still surface something HERE too, or toggling from this view looks
    // like it silently worked. logWarning stays as the file breadcrumb.
    logWarning(`transcription settings save failed (vault ${props.vaultId}): ${String(e)}`);
    notifications.error(`Couldn't save transcription settings: ${String(e)}`);
  }
}

async function loadConfig() {
  // A config read failure must never block recording — config keeps the
  // defaults above, so the transcription settings stay editable too.
  try {
    config.value = await invoke<CaptureConfig>("get_capture_config", { id: props.vaultId });
  } catch {
    // stale config never blocks recording — mirror the backend's rule
  } finally {
    // Set on BOTH success and failure: a read failure must still let the
    // user save against the defaults (documented above), so persistence
    // unblocks here either way — only the source of `config` differs.
    loaded.value = true;
  }
}

async function loadRecordingCount() {
  try {
    const list = await invoke<Recording[]>("list_recordings", { id: props.vaultId });
    recordingCount.value = list.length;
  } catch (e) {
    // Advisory count — degrade to a hidden pill, never block the view (the
    // vault list's task badges follow the same rule).
    logWarning(`list_recordings failed (vault ${props.vaultId}): ${String(e)}`);
  }
}

async function detectPandoc() {
  // Swallowed like every other guarded read here: an unavailable Tauri
  // runtime or a detection failure just leaves `pandoc` null, which
  // `importStatus` treats as "not installed" — the button degrades to
  // disabled rather than block the rest of the view.
  try {
    pandoc.value = await invoke<PandocStatus>("detect_pandoc");
  } catch (e) {
    logWarning(`detect_pandoc failed (vault ${props.vaultId}): ${String(e)}`);
  }
}

onMounted(() => {
  // Independent reads: a hung/failed config read must not block the count
  // and vice versa, so neither awaits the other.
  void loadConfig();
  void loadRecordingCount();
  void detectPandoc();
});

function start(mode: "meeting" | "voice-note") {
  void capture.start(props.vaultId, mode);
  store.showList(); // recording bar shows on the list view
}

async function importDocument() {
  try {
    const path = await open({
      multiple: false,
      filters: [{ name: "Documents", extensions: ["docx", "odt", "rtf"] }],
    });
    if (typeof path !== "string") return; // cancelled — no-op
    const notePath = await invoke<string>("convert_document", {
      id: props.vaultId,
      sourcePath: path,
    });
    const name = notePath.split(/[\\/]/).pop() ?? notePath;
    notifications.success(`Imported ${name}`);
  } catch (e) {
    logWarning(`convert_document failed (vault ${props.vaultId}): ${String(e)}`);
    notifications.error(`Couldn't import document: ${String(e)}`);
  }
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <div class="flex flex-col gap-2">
      <button
        v-for="option in OPTIONS"
        :key="option.key"
        type="button"
        :data-testid="option.testId"
        :aria-label="`Start a ${option.title.toLowerCase()} recording`"
        class="w-full cursor-pointer rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="start(option.key)"
      >
        <span class="block text-sm font-medium text-slate-100">{{ option.title }}</span>
        <span class="block text-xs text-slate-400">{{ option.hint }}</span>
      </button>
      <button
        type="button"
        data-testid="mode-browse"
        :aria-label="browseLabel"
        class="flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="store.openRecordings(props.vaultId)"
      >
        <span class="min-w-0">
          <span class="block text-sm font-medium text-slate-100">Browse recordings</span>
          <span class="block text-xs text-slate-400">See past recordings in this vault</span>
        </span>
        <span
          v-if="recordingCount !== null"
          data-testid="recording-count"
          class="shrink-0 rounded-full bg-white/10 px-2 py-0.5 text-xs text-slate-300"
        >{{ recordingCount }}</span>
      </button>
      <button
        type="button"
        data-testid="import-document"
        aria-label="Import a document into this vault"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-left transition-colors enabled:cursor-pointer enabled:hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
        :disabled="importStatus.disabled"
        @click="importDocument"
      >
        <span class="block text-sm font-medium text-slate-100">Import Document</span>
        <span class="block text-xs text-slate-400">{{ importStatus.hint }}</span>
      </button>
    </div>
    <div class="flex flex-col gap-3 border-t border-white/10 pt-3">
      <TranscriptionSettings v-model="transcription" />
    </div>
  </div>
</template>
