<script setup lang="ts">
import { computed } from "vue";

import type { AudioDevice, AudioDevices, RecordingSettingsValue } from "../types";
import SelectMenu from "./SelectMenu.vue";
import TranscriptionSettings from "./TranscriptionSettings.vue";

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

const meetingFolder = computed({
  get: () => props.modelValue.meetingFolder,
  set: (v: string) => patch({ meetingFolder: v }),
});
const voiceNoteFolder = computed({
  get: () => props.modelValue.voiceNoteFolder,
  set: (v: string) => patch({ voiceNoteFolder: v }),
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
const noteExtraFrontmatter = computed({
  get: () => props.modelValue.noteExtraFrontmatter,
  set: (v: string) => patch({ noteExtraFrontmatter: v }),
});
const noteBodyTemplate = computed({
  get: () => props.modelValue.noteBodyTemplate,
  set: (v: string) => patch({ noteBodyTemplate: v }),
});
const inputDevice = computed({
  get: () => props.modelValue.inputDevice,
  set: (v: string) => patch({ inputDevice: v }),
});
const outputDevice = computed({
  get: () => props.modelValue.outputDevice,
  set: (v: string) => patch({ outputDevice: v }),
});
const recordingDateFolders = computed({
  get: () => props.modelValue.recordingDateFolders,
  set: (v: boolean) => patch({ recordingDateFolders: v }),
});

// Shown under both template textareas below. The literal `{{...}}`
// placeholder syntax must live in a script string, never typed directly into
// template text: Vue's mustache tokenizer finds the FIRST `}}` textually (no
// brace-depth awareness), so writing it inline in the template would
// terminate the interpolation early and corrupt the markup.
const TEMPLATE_PLACEHOLDER_HINT =
  "Placeholders: {{date}}, {{recordedAt}}, {{duration}}, {{vault}}, {{type}}. Identity fields and the audio/transcript embeds are always added. A non-empty body template replaces the follow-up scaffold.";

// Bundles the six transcription fields for TranscriptionSettings' v-model —
// same adapter idiom CaptureSettings.vue used before this extraction.
const transcriptionBundle = computed({
  get: () => ({
    transcribe: props.modelValue.transcribe,
    transcriptionModel: props.modelValue.transcriptionModel,
    transcriptionLanguage: props.modelValue.transcriptionLanguage,
    transcriptTimestamps: props.modelValue.transcriptTimestamps,
    transcriptionVocabulary: props.modelValue.transcriptionVocabulary,
    transcriptionVad: props.modelValue.transcriptionVad,
  }),
  set: (v: {
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
    transcriptionVocabulary: string;
    transcriptionVad: boolean;
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
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Folders
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <div>
        <label
          class="mb-1 block text-sm text-slate-200"
          for="capture-meeting-folder"
        >
          Meeting folder
          <span class="block text-xs text-fg-subtle">Inside the vault</span>
        </label>
        <input
          id="capture-meeting-folder"
          v-model="meetingFolder"
          data-testid="meeting-folder-input"
          type="text"
          placeholder="Meetings"
          class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
        >
      </div>
      <div>
        <label
          class="mb-1 block text-sm text-slate-200"
          for="capture-voice-note-folder"
        >
          Voice Note folder
          <span class="block text-xs text-fg-subtle">Inside the vault</span>
        </label>
        <input
          id="capture-voice-note-folder"
          v-model="voiceNoteFolder"
          data-testid="voice-note-folder-input"
          type="text"
          placeholder="Voice Notes"
          class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
        >
      </div>
      <p
        v-if="folderError"
        data-testid="folder-error"
        class="mt-1 text-xs text-red-300"
      >
        {{ folderError }}
      </p>
      <div class="flex items-center justify-between">
        <label
          for="recording-date-folders"
          class="text-sm text-slate-200"
        >
          Organize into year/month folders
          <span class="block text-xs text-fg-subtle">Off = one flat folder</span>
        </label>
        <input
          id="recording-date-folders"
          v-model="recordingDateFolders"
          data-testid="recording-date-folders-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
        >
      </div>
    </div>
  </section>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Audio
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
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
          <span class="block text-xs text-fg-subtle">Loopback · used for meeting recordings</span>
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
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Companion note
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between">
        <label
          for="capture-note-toggle"
          class="text-sm text-slate-200"
        >
          Companion note
          <span class="block text-xs text-fg-subtle">.md with metadata + embed</span>
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
          <span class="block text-xs text-fg-subtle">Action items · Decisions · Notes</span>
        </label>
        <input
          id="capture-follow-up-toggle"
          v-model="followUpTemplate"
          data-testid="follow-up-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
        >
      </div>
      <div
        v-if="createNote"
        class="flex flex-col gap-1 border-l border-white/10 pl-3"
      >
        <label
          class="text-sm text-slate-200"
          for="capture-note-extra-frontmatter"
        >
          Extra frontmatter
        </label>
        <textarea
          id="capture-note-extra-frontmatter"
          v-model="noteExtraFrontmatter"
          data-testid="note-extra-frontmatter"
          rows="3"
          placeholder="attendees: [Alex, Sam]"
          class="w-full resize-y rounded-control border border-white/10 bg-white/5 px-2 py-1 font-mono text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
        />
        <p class="text-xs text-fg-subtle">
          {{ TEMPLATE_PLACEHOLDER_HINT }}
        </p>
      </div>
      <div
        v-if="createNote"
        class="flex flex-col gap-1 border-l border-white/10 pl-3"
      >
        <label
          class="text-sm text-slate-200"
          for="capture-note-body-template"
        >
          Body template
        </label>
        <textarea
          id="capture-note-body-template"
          v-model="noteBodyTemplate"
          data-testid="note-body-template"
          rows="3"
          placeholder="## Summary"
          class="w-full resize-y rounded-control border border-white/10 bg-white/5 px-2 py-1 font-mono text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
        />
        <p class="text-xs text-fg-subtle">
          {{ TEMPLATE_PLACEHOLDER_HINT }}
        </p>
      </div>
    </div>
  </section>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Transcription
    </h2>
    <div class="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/5 p-2">
      <TranscriptionSettings v-model="transcriptionBundle" />
    </div>
  </section>
</template>
