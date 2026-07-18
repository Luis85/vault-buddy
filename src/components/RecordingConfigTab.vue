<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { AudioDevices, CaptureConfig, RecordingSettingsValue } from "../types";
import RecordingSettings from "./RecordingSettings.vue";

// Folders are the only free-text fields in the bundle → debounce; everything
// else is a toggle/select → save immediately.
const TEXT_KEYS = new Set<keyof RecordingSettingsValue>([
  "meetingFolder",
  "voiceNoteFolder",
  "transcriptionVocabulary",
]);

// The Recording tab of Vault settings. Owns the capture-config + devices load,
// hosts the controlled RecordingSettings, and auto-saves the whole
// set_capture_config struct. Folder text debounces; every other control
// (toggles/selects) saves immediately. `mode` is a pass-through — the UI can't
// edit it, but the loaded value is sent back unchanged.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const mode = ref<"meeting" | "voice-note">("meeting");
const devices = ref<AudioDevices>({ inputs: [], outputs: [] });
const rec = ref<RecordingSettingsValue>({
  meetingFolder: "",
  voiceNoteFolder: "",
  bitrateKbps: 128,
  createNote: true,
  followUpTemplate: true,
  inputDevice: "",
  outputDevice: "",
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
  transcriptionVocabulary: "",
  transcriptionVad: true,
  recordingDateFolders: false,
});

const autosave = useAutosave(
  async () => {
    const r = rec.value;
    await invoke("set_capture_config", {
      id: props.vaultId,
      cfg: {
        mode: mode.value,
        meetingFolder: r.meetingFolder.trim() || null,
        voiceNoteFolder: r.voiceNoteFolder.trim() || null,
        bitrateKbps: r.bitrateKbps,
        createNote: r.createNote,
        followUpTemplate: r.followUpTemplate,
        inputDevice: r.inputDevice || null,
        outputDevice: r.outputDevice || null,
        transcribe: r.transcribe,
        transcriptionModel: r.transcriptionModel,
        transcriptionLanguage: r.transcriptionLanguage.trim() || null,
        transcriptTimestamps: r.transcriptTimestamps,
        transcriptionVocabulary: r.transcriptionVocabulary.trim() || null,
        transcriptionVad: r.transcriptionVad,
        recordingDateFolders: r.recordingDateFolders,
      },
    });
  },
  { label: "capture settings" },
);

// RecordingSettings emits the whole bundle on any change and only on user
// interaction (never on the load assignment below), so this handler is a safe
// single trigger point. Diff which keys changed: a change confined to the
// free-text folder fields debounces; anything else (a toggle/select) saves now.
function onUpdate(next: RecordingSettingsValue) {
  const cur = rec.value;
  const changed = (Object.keys(next) as (keyof RecordingSettingsValue)[]).filter(
    (k) => next[k] !== cur[k],
  );
  rec.value = next;
  if (changed.length === 0) return;
  if (changed.every((k) => TEXT_KEYS.has(k))) autosave.schedule();
  else autosave.saveNow();
}

// Split the one autosave error into the inline folder line vs a form-level
// line, preserving the pre-autosave UX.
const folderError = computed(() =>
  autosave.error.value && autosave.error.value.toLowerCase().includes("folder") ? autosave.error.value : null,
);
const formError = computed(() =>
  autosave.error.value && !autosave.error.value.toLowerCase().includes("folder") ? autosave.error.value : null,
);

onMounted(async () => {
  try {
    const [cfg, devs] = await Promise.all([
      invoke<CaptureConfig>("get_capture_config", { id: props.vaultId }),
      invoke<AudioDevices>("list_audio_devices"),
    ]);
    mode.value = cfg.mode;
    rec.value = {
      meetingFolder: cfg.meetingFolder ?? "",
      voiceNoteFolder: cfg.voiceNoteFolder ?? "",
      bitrateKbps: cfg.bitrateKbps,
      createNote: cfg.createNote,
      followUpTemplate: cfg.followUpTemplate,
      inputDevice: cfg.inputDevice ?? "",
      outputDevice: cfg.outputDevice ?? "",
      transcribe: cfg.transcribe,
      transcriptionModel: cfg.transcriptionModel,
      transcriptionLanguage: cfg.transcriptionLanguage ?? "",
      transcriptTimestamps: cfg.transcriptTimestamps,
      transcriptionVocabulary: cfg.transcriptionVocabulary ?? "",
      transcriptionVad: cfg.transcriptionVad,
      recordingDateFolders: cfg.recordingDateFolders,
    };
    devices.value = devs;
  } catch (e) {
    loadError.value = String(e);
    logWarning(`get_capture_config failed (vault ${props.vaultId}): ${String(e)}`);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <div
    class="flex flex-col gap-3"
    @focusout="autosave.flush()"
  >
    <p
      v-if="loading"
      class="text-xs text-slate-400"
    >
      Loading…
    </p>
    <p
      v-else-if="loadError"
      data-testid="recording-load-error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <template v-else>
      <RecordingSettings
        :model-value="rec"
        :devices="devices"
        :folder-error="folderError"
        @update:model-value="onUpdate"
      />
      <p
        v-if="formError"
        data-testid="recording-form-error"
        class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
      >
        {{ formError }}
      </p>
    </template>
  </div>
</template>
