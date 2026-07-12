<script setup lang="ts">
import { computed } from "vue";

import type { AudioDevice, AudioDevices } from "../types";
import SelectMenu from "./SelectMenu.vue";
import TranscriptionSettings from "./TranscriptionSettings.vue";

interface RecordingSettingsValue {
  recordingFolder: string;
  bitrateKbps: number;
  createNote: boolean;
  followUpTemplate: boolean;
  inputDevice: string;
  outputDevice: string;
  transcribe: boolean;
  transcriptionModel: string;
  transcriptionLanguage: string;
  transcriptTimestamps: boolean;
}

// Controlled component: no persistence of its own — same idiom as
// TranscriptionSettings.vue. Every field is a computed get/set proxy onto
// props.modelValue; the setter always spread-merges onto the CURRENT prop
// (never a locally cached copy) and emits, so CaptureSettings.vue stays the
// single owner of the underlying reactive state.
const props = defineProps<{
  modelValue: RecordingSettingsValue;
  devices: AudioDevices;
  folderError: string | null;
}>();
const emit = defineEmits<{ "update:modelValue": [value: RecordingSettingsValue] }>();

function patch(change: Partial<RecordingSettingsValue>) {
  emit("update:modelValue", { ...props.modelValue, ...change });
}

const recordingFolder = computed({
  get: () => props.modelValue.recordingFolder,
  set: (v: string) => patch({ recordingFolder: v }),
});
const bitrateKbps = computed({
  get: () => props.modelValue.bitrateKbps,
  set: (v: number) => patch({ bitrateKbps: v }),
});
const createNote = computed({
  get: () => props.modelValue.createNote,
  set: (v: boolean) => patch({ createNote: v }),
});
const followUpTemplate = computed({
  get: () => props.modelValue.followUpTemplate,
  set: (v: boolean) => patch({ followUpTemplate: v }),
});
const inputDevice = computed({
  get: () => props.modelValue.inputDevice,
  set: (v: string) => patch({ inputDevice: v }),
});
const outputDevice = computed({
  get: () => props.modelValue.outputDevice,
  set: (v: string) => patch({ outputDevice: v }),
});

// Bundles the four transcription fields for TranscriptionSettings' v-model —
// same adapter idiom CaptureSettings.vue used before this extraction.
const transcriptionBundle = computed({
  get: () => ({
    transcribe: props.modelValue.transcribe,
    transcriptionModel: props.modelValue.transcriptionModel,
    transcriptionLanguage: props.modelValue.transcriptionLanguage,
    transcriptTimestamps: props.modelValue.transcriptTimestamps,
  }),
  set: (v: {
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
  }) => patch(v),
});

const BITRATES = [128, 160, 192];
// Option list for the bitrate SelectMenu dropdown ({ value, label }).
const bitrateOptions = BITRATES.map((b) => ({ value: b, label: `${b} kbps` }));

// A configured device that is not currently connected must stay selectable
// (unplugging a headset must not silently rewrite the config) — it is
// surfaced with a "(not connected)" suffix instead.
function withConfigured(list: AudioDevice[], configured: string) {
  const options = list.map((d) => ({ value: d.name, label: d.name }));
  if (configured && !list.some((d) => d.name === configured)) {
    options.push({ value: configured, label: `${configured} (not connected)` });
  }
  return options;
}
const inputMenuOptions = computed(() => [
  { value: "", label: "System default" },
  ...withConfigured(props.devices.inputs, inputDevice.value),
]);
const outputMenuOptions = computed(() => [
  { value: "", label: "System default" },
  ...withConfigured(props.devices.outputs, outputDevice.value),
]);
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Recording
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <div>
        <label
          class="mb-1 block text-sm text-slate-200"
          for="capture-folder"
        >
          Recording folder
          <span class="block text-xs text-slate-500">Inside the vault</span>
        </label>
        <input
          id="capture-folder"
          v-model="recordingFolder"
          data-testid="folder-input"
          type="text"
          placeholder="Meetings or Voice Notes"
          class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        >
        <p
          v-if="folderError"
          data-testid="folder-error"
          class="mt-1 text-xs text-red-300"
        >
          {{ folderError }}
        </p>
      </div>
      <div class="flex items-center justify-between gap-2">
        <label
          for="capture-bitrate"
          class="text-sm text-slate-200"
        >Bitrate</label>
        <SelectMenu
          id="capture-bitrate"
          v-model="bitrateKbps"
          :options="bitrateOptions"
          data-testid="bitrate-select"
        />
      </div>
    </div>
  </section>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Companion note
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between">
        <label
          for="capture-note-toggle"
          class="text-sm text-slate-200"
        >
          Companion note
          <span class="block text-xs text-slate-500">.md with metadata + embed</span>
        </label>
        <input
          id="capture-note-toggle"
          v-model="createNote"
          data-testid="note-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
        >
      </div>
      <div
        v-if="createNote"
        class="flex items-center justify-between border-l border-white/10 pl-3"
      >
        <label
          for="capture-follow-up-toggle"
          class="text-sm text-slate-200"
        >
          Follow-up template
          <span class="block text-xs text-slate-500">Action items · Decisions · Notes</span>
        </label>
        <input
          id="capture-follow-up-toggle"
          v-model="followUpTemplate"
          data-testid="follow-up-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
        >
      </div>
    </div>
  </section>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Transcription
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <TranscriptionSettings v-model="transcriptionBundle" />
    </div>
  </section>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Audio devices
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <div>
        <label
          class="mb-1 block text-sm text-slate-200"
          for="capture-input-device"
        >
          Microphone
        </label>
        <SelectMenu
          id="capture-input-device"
          v-model="inputDevice"
          :options="inputMenuOptions"
          aria-label="Microphone"
          data-testid="input-device-select"
          wide
        />
      </div>
      <div>
        <label
          class="mb-1 block text-sm text-slate-200"
          for="capture-output-device"
        >
          Desktop audio from
          <span class="block text-xs text-slate-500">Loopback · used for meeting recordings</span>
        </label>
        <SelectMenu
          id="capture-output-device"
          v-model="outputDevice"
          :options="outputMenuOptions"
          aria-label="Desktop audio device"
          data-testid="output-device-select"
          wide
        />
      </div>
    </div>
  </section>
</template>
