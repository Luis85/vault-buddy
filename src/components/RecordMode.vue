<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useVaultsStore } from "../stores/vaults";
import { useCaptureStore } from "../stores/capture";
import { useNotificationsStore } from "../stores/notifications";
import { logWarning } from "../logging";
import type { CaptureConfig } from "../types";
import TranscriptionSettings from "./TranscriptionSettings.vue";

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const capture = useCaptureStore();
const notifications = useNotificationsStore();

const OPTIONS = [
  { key: "meeting", title: "Meeting", hint: "Microphone + desktop audio", testId: "mode-meeting" },
  { key: "voice-note", title: "Voice Note", hint: "Microphone only", testId: "mode-voice-note" },
] as const;

const defaultMode = ref<"meeting" | "voice-note">("meeting");

// Gates persist() (not rendering) until the vault's real config has landed
// (set in onMounted's `finally`, so it also flips on a failed read — see
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

onMounted(async () => {
  // The chooser needs the vault's DEFAULT mode; a config read failure must
  // never block recording — fall back to meeting (config keeps the defaults
  // above, so the transcription settings stay editable too).
  try {
    const cfg = await invoke<CaptureConfig>("get_capture_config", { id: props.vaultId });
    defaultMode.value = cfg.mode;
    config.value = cfg;
  } catch {
    // stale config never blocks recording — mirror the backend's rule
  } finally {
    // Set on BOTH success and failure: a read failure must still let the
    // user save against the defaults (documented above), so persistence
    // unblocks here either way — only the source of `config` differs.
    loaded.value = true;
  }
});

function start(mode: "meeting" | "voice-note") {
  void capture.start(props.vaultId, mode);
  store.showList(); // recording bar shows on the list view
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <TranscriptionSettings v-model="transcription" />
    <div class="flex flex-col gap-2 border-t border-white/10 pt-3">
      <button
        v-for="option in OPTIONS"
        :key="option.key"
        type="button"
        :data-testid="option.testId"
        :aria-label="`Start a ${option.title.toLowerCase()} recording`"
        class="w-full cursor-pointer rounded-lg border px-3 py-2 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="
          option.key === defaultMode
            ? 'border-violet-400 bg-violet-500/20'
            : 'border-white/10 bg-white/5 hover:bg-white/10'
        "
        @click="start(option.key)"
      >
        <span class="block text-sm font-medium text-slate-100">{{ option.title }}</span>
        <span class="block text-xs text-slate-400">{{ option.hint }}</span>
      </button>
      <button
        type="button"
        data-testid="mode-browse"
        aria-label="Browse past recordings"
        class="mt-1 w-full cursor-pointer border-t border-white/10 pt-2 text-left text-xs text-slate-400 transition-colors hover:text-slate-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="store.openRecordings(props.vaultId)"
      >
        Browse recordings…
        <span class="block text-slate-500">See past recordings in this vault</span>
      </button>
    </div>
  </div>
</template>
