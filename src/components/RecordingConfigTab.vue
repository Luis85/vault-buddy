<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { AudioDevices, CaptureConfig, RecordingSettingsValue } from "../types";
import RecordingSettings from "./RecordingSettings.vue";
import Banner from "./ui/Banner.vue";

// Folders are the only free-text fields in the bundle → debounce; everything
// else is a toggle/select → save immediately.
const TEXT_KEYS = new Set<keyof RecordingSettingsValue>([
  "meetingFolder",
  "voiceNoteFolder",
  "transcriptionVocabulary",
  "noteExtraFrontmatter",
  "noteBodyTemplate",
]);

// The Recording tab of Vault settings. Owns the capture-config + devices load,
// hosts the controlled RecordingSettings, and auto-saves the whole
// set_capture_config struct. Folder text debounces; every other control
// (toggles/selects) saves immediately. `mode` is a pass-through — the UI can't
// edit it, but the loaded value is sent back unchanged.
const props = defineProps<{ vaultId: string }>();

// The DTO's optional-string fields surface as null; every input/textarea in
// the form binds a plain string ("" means "unset"). Centralizing the
// null->"" fallback here (instead of a `??` at each of the form's eight
// optional-string fields) keeps onMounted's load below the complexity
// ratchet — each inline `??` is its own branch in the loading function.
function orEmpty(v: string | null): string {
  return v ?? "";
}

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
  noteExtraFrontmatter: "",
  noteBodyTemplate: "",
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
        noteExtraFrontmatter: r.noteExtraFrontmatter.trim() || null,
        noteBodyTemplate: r.noteBodyTemplate.trim() || null,
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
      meetingFolder: orEmpty(cfg.meetingFolder),
      voiceNoteFolder: orEmpty(cfg.voiceNoteFolder),
      bitrateKbps: cfg.bitrateKbps,
      createNote: cfg.createNote,
      followUpTemplate: cfg.followUpTemplate,
      inputDevice: orEmpty(cfg.inputDevice),
      outputDevice: orEmpty(cfg.outputDevice),
      transcribe: cfg.transcribe,
      transcriptionModel: cfg.transcriptionModel,
      transcriptionLanguage: orEmpty(cfg.transcriptionLanguage),
      transcriptTimestamps: cfg.transcriptTimestamps,
      transcriptionVocabulary: orEmpty(cfg.transcriptionVocabulary),
      transcriptionVad: cfg.transcriptionVad,
      noteExtraFrontmatter: orEmpty(cfg.noteExtraFrontmatter),
      noteBodyTemplate: orEmpty(cfg.noteBodyTemplate),
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
      class="text-xs text-fg-muted"
    >
      Loading…
    </p>
    <Banner
      v-else-if="loadError"
      data-testid="recording-load-error"
      tone="danger"
    >
      {{ loadError }}
    </Banner>
    <template v-else>
      <RecordingSettings
        :model-value="rec"
        :devices="devices"
        :folder-error="folderError"
        @update:model-value="onUpdate"
      />
      <Banner
        v-if="formError"
        data-testid="recording-form-error"
        tone="danger"
      >
        {{ formError }}
      </Banner>
    </template>
  </div>
</template>
