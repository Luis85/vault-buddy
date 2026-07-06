<script setup lang="ts">
import { computed } from "vue";
import SelectMenu from "./SelectMenu.vue";

interface TranscriptionSettingsValue {
  transcribe: boolean;
  transcriptionModel: string;
  transcriptionLanguage: string; // "" = auto-detect
  transcriptTimestamps: boolean;
}

// Controlled component: no persistence of its own. Every field is a
// computed get/set proxy onto props.modelValue — the setter always
// spread-merges onto the CURRENT prop (never a locally cached copy) and
// emits, so the prop is never mutated in place and CaptureSettings.vue (or
// any future consumer, e.g. the Record view) stays the single owner of
// the underlying reactive state.
const props = defineProps<{ modelValue: TranscriptionSettingsValue }>();
const emit = defineEmits<{ "update:modelValue": [value: TranscriptionSettingsValue] }>();

function patch(change: Partial<TranscriptionSettingsValue>) {
  emit("update:modelValue", { ...props.modelValue, ...change });
}

const transcribe = computed({
  get: () => props.modelValue.transcribe,
  set: (v: boolean) => patch({ transcribe: v }),
});
const transcriptionModel = computed({
  get: () => props.modelValue.transcriptionModel,
  set: (v: string) => patch({ transcriptionModel: v }),
});
const transcriptionLanguage = computed({
  get: () => props.modelValue.transcriptionLanguage,
  set: (v: string) => patch({ transcriptionLanguage: v }),
});
const transcriptTimestamps = computed({
  get: () => props.modelValue.transcriptTimestamps,
  set: (v: boolean) => patch({ transcriptTimestamps: v }),
});

const MODELS = ["base", "small", "medium"] as const;
const LANGUAGES = [
  { code: "", name: "Auto-detect" },
  { code: "en", name: "English" },
  { code: "de", name: "German" },
  { code: "es", name: "Spanish" },
  { code: "fr", name: "French" },
  { code: "it", name: "Italian" },
  { code: "pt", name: "Portuguese" },
  { code: "nl", name: "Dutch" },
  { code: "pl", name: "Polish" },
  { code: "zh", name: "Chinese" },
  { code: "ja", name: "Japanese" },
  { code: "ru", name: "Russian" },
  { code: "ar", name: "Arabic" },
] as const;

function capitalize(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

// Option lists for the SelectMenu dropdowns ({ value, label }).
const modelOptions = MODELS.map((m) => ({ value: m, label: capitalize(m) }));
const languageOptions = LANGUAGES.map((l) => ({ value: l.code, label: l.name }));
</script>

<template>
  <section class="flex items-center justify-between">
    <label for="capture-transcribe-toggle" class="text-sm text-slate-200">
      Transcribe recordings
      <span class="block text-xs text-slate-500">Local speech-to-text · no cloud</span>
    </label>
    <input
      id="capture-transcribe-toggle"
      v-model="transcribe"
      data-testid="transcribe-toggle"
      type="checkbox"
      class="h-4 w-4 accent-violet-500"
    />
  </section>
  <div v-if="transcribe" class="flex flex-col gap-3 border-l border-white/10 pl-3">
    <section class="flex items-center justify-between gap-2">
      <label for="capture-transcription-model" class="text-sm text-slate-200">Model</label>
      <SelectMenu
        id="capture-transcription-model"
        v-model="transcriptionModel"
        :options="modelOptions"
        data-testid="transcription-model-select"
      />
    </section>
    <section class="flex items-center justify-between gap-2">
      <label for="capture-transcription-language" class="text-sm text-slate-200">Language</label>
      <SelectMenu
        id="capture-transcription-language"
        v-model="transcriptionLanguage"
        :options="languageOptions"
        data-testid="transcription-language-select"
      />
    </section>
    <section class="flex items-center justify-between">
      <label for="capture-transcript-timestamps-toggle" class="text-sm text-slate-200">
        Timestamps
        <span class="block text-xs text-slate-500">Insert time markers in the transcript</span>
      </label>
      <input
        id="capture-transcript-timestamps-toggle"
        v-model="transcriptTimestamps"
        data-testid="transcript-timestamps-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
      />
    </section>
  </div>
</template>
