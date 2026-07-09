<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import type {
  AudioDevice,
  AudioDevices,
  CaptureConfig,
  TasksConfig,
} from "../types";
import SelectMenu from "./SelectMenu.vue";
import TranscriptionSettings from "./TranscriptionSettings.vue";

const props = defineProps<{ vaultId: string }>();

const BITRATES = [128, 160, 192];

const loading = ref(true);
const loadError = ref<string | null>(null);
const saveState = ref<"idle" | "saving" | "saved">("idle");
const saveError = ref<string | null>(null);
const folderError = ref<string | null>(null);

const mode = ref<"meeting" | "voice-note">("meeting");
const recordingFolder = ref("");
const createNote = ref(true);
const followUpTemplate = ref(true);
const bitrateKbps = ref(128);
const inputDevice = ref(""); // "" = system default
const outputDevice = ref("");
const devices = ref<AudioDevices>({ inputs: [], outputs: [] });
const transcribe = ref(false);
const transcriptionModel = ref("small");
const transcriptionLanguage = ref(""); // "" = auto-detect (maps to null on save)
const transcriptTimestamps = ref(true);

// The per-vault tasks folder lives in the same app-side config but has its own
// command pair and its own Save — independent of the capture-config save (which
// already preserves tasks_folder), so a tasks-config failure can't block the
// capture form and vice versa.
const tasksFolder = ref(""); // "" shows the "Tasks" placeholder / clears to default
const tasksFolderError = ref<string | null>(null);

// A configured device that is not currently connected must stay
// selectable (unplugging a headset must not silently rewrite the
// config) — it is surfaced with a "(not connected)" suffix instead.
function withConfigured(list: AudioDevice[], configured: string) {
  const options = list.map((d) => ({ value: d.name, label: d.name }));
  if (configured && !list.some((d) => d.name === configured)) {
    options.push({ value: configured, label: `${configured} (not connected)` });
  }
  return options;
}
const inputOptions = computed(() =>
  withConfigured(devices.value.inputs, inputDevice.value),
);
const outputOptions = computed(() =>
  withConfigured(devices.value.outputs, outputDevice.value),
);

const folderPlaceholder = computed(() =>
  mode.value === "meeting" ? "Meetings" : "Voice Notes",
);

// Option list for the bitrate SelectMenu dropdown ({ value, label }).
const bitrateOptions = BITRATES.map((b) => ({ value: b, label: `${b} kbps` }));
const inputMenuOptions = computed(() => [
  { value: "", label: "System default" },
  ...inputOptions.value,
]);
const outputMenuOptions = computed(() => [
  { value: "", label: "System default" },
  ...outputOptions.value,
]);

// Bundles the four transcription fields for TranscriptionSettings' v-model.
// The setter fans a merged update back out to the individual refs so
// save()/onMounted()/watch (below) keep working on them unchanged — this
// computed is purely an adapter for the extracted controlled component.
const transcriptionSettings = computed({
  get: () => ({
    transcribe: transcribe.value,
    transcriptionModel: transcriptionModel.value,
    transcriptionLanguage: transcriptionLanguage.value,
    transcriptTimestamps: transcriptTimestamps.value,
  }),
  set: (v: {
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
  }) => {
    transcribe.value = v.transcribe;
    transcriptionModel.value = v.transcriptionModel;
    transcriptionLanguage.value = v.transcriptionLanguage;
    transcriptTimestamps.value = v.transcriptTimestamps;
  },
});

// Any edit invalidates the "Saved ✓" confirmation. During the initial load
// saveState is already "idle", so the load-time assignments are idle→idle
// no-ops; this only becomes visible after a save set it to "saved".
watch(
  [
    mode,
    recordingFolder,
    createNote,
    followUpTemplate,
    bitrateKbps,
    inputDevice,
    outputDevice,
    transcribe,
    transcriptionModel,
    transcriptionLanguage,
    transcriptTimestamps,
  ],
  () => {
    if (saveState.value === "saved") saveState.value = "idle";
  },
);

onMounted(async () => {
  try {
    const [cfg, devs] = await Promise.all([
      invoke<CaptureConfig>("get_capture_config", { id: props.vaultId }),
      invoke<AudioDevices>("list_audio_devices"),
    ]);
    mode.value = cfg.mode;
    recordingFolder.value = cfg.recordingFolder ?? "";
    createNote.value = cfg.createNote;
    followUpTemplate.value = cfg.followUpTemplate;
    bitrateKbps.value = cfg.bitrateKbps;
    inputDevice.value = cfg.inputDevice ?? "";
    outputDevice.value = cfg.outputDevice ?? "";
    devices.value = devs;
    transcribe.value = cfg.transcribe;
    transcriptionModel.value = cfg.transcriptionModel;
    transcriptionLanguage.value = cfg.transcriptionLanguage ?? "";
    transcriptTimestamps.value = cfg.transcriptTimestamps;
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
  // Separate invoke (not in the Promise.all above) so a tasks-config failure
  // can't block the capture form from loading — the tasks folder is optional.
  try {
    const tcfg = await invoke<TasksConfig>("get_tasks_config", {
      id: props.vaultId,
    });
    tasksFolder.value = tcfg.tasksFolder ?? "";
  } catch (e) {
    logWarning(`get_tasks_config failed (vault ${props.vaultId}): ${String(e)}`);
  }
});

async function save() {
  saveState.value = "saving";
  saveError.value = null;
  folderError.value = null;
  const folder = recordingFolder.value.trim();
  try {
    await invoke("set_capture_config", {
      id: props.vaultId,
      cfg: {
        mode: mode.value,
        recordingFolder: folder ? folder : null,
        bitrateKbps: bitrateKbps.value,
        createNote: createNote.value,
        followUpTemplate: followUpTemplate.value,
        inputDevice: inputDevice.value || null,
        outputDevice: outputDevice.value || null,
        transcribe: transcribe.value,
        transcriptionModel: transcriptionModel.value,
        transcriptionLanguage: transcriptionLanguage.value.trim() || null,
        transcriptTimestamps: transcriptTimestamps.value,
      },
    });
    saveState.value = "saved";
  } catch (e) {
    saveState.value = "idle";
    // Folder rejections are field-level; everything else is form-level.
    // Form state is preserved either way so the user can correct and retry.
    const message = String(e);
    if (message.toLowerCase().includes("folder")) folderError.value = message;
    else saveError.value = message;
    logWarning(`capture settings save failed (vault ${props.vaultId}): ${message}`);
  }
}

async function saveTasksFolder() {
  tasksFolderError.value = null;
  const value = tasksFolder.value.trim();
  try {
    await invoke("set_tasks_config", {
      id: props.vaultId,
      tasksFolder: value === "" ? null : value,
    });
  } catch (e) {
    tasksFolderError.value = String(e);
    logWarning(`set_tasks_config failed (vault ${props.vaultId}): ${String(e)}`);
  }
}
</script>

<template>
  <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
  <p v-else-if="loadError" class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200">
    {{ loadError }}
  </p>
  <form v-else class="flex flex-col gap-3" @submit.prevent="save">
    <section>
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Default recording mode
      </h2>
      <div class="flex gap-1" role="radiogroup" aria-label="Default recording mode">
        <button
          v-for="m in [
            { key: 'meeting', label: 'Meeting' },
            { key: 'voice-note', label: 'Voice Note' },
          ] as const"
          :key="m.key"
          type="button"
          role="radio"
          class="cursor-pointer rounded-lg border px-2 py-0.5 text-xs transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            mode === m.key
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          :aria-checked="mode === m.key"
          :data-testid="`mode-${m.key}`"
          @click="mode = m.key"
        >
          {{ m.label }}
        </button>
      </div>
    </section>
    <section>
      <label class="mb-1 block text-sm text-slate-200" for="capture-folder">
        Recording folder
        <span class="block text-xs text-slate-500">Inside the vault</span>
      </label>
      <input
        id="capture-folder"
        v-model="recordingFolder"
        data-testid="folder-input"
        type="text"
        :placeholder="folderPlaceholder"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      />
      <p
        v-if="folderError"
        data-testid="folder-error"
        class="mt-1 text-xs text-red-300"
      >
        {{ folderError }}
      </p>
    </section>
    <section class="flex items-center justify-between">
      <label for="capture-note-toggle" class="text-sm text-slate-200">
        Companion note
        <span class="block text-xs text-slate-500">.md with metadata + embed</span>
      </label>
      <input
        id="capture-note-toggle"
        v-model="createNote"
        data-testid="note-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
      />
    </section>
    <div
      v-if="createNote"
      class="flex items-center justify-between border-l border-white/10 pl-3"
    >
      <label for="capture-follow-up-toggle" class="text-sm text-slate-200">
        Follow-up template
        <span class="block text-xs text-slate-500">Action items · Decisions · Notes</span>
      </label>
      <input
        id="capture-follow-up-toggle"
        v-model="followUpTemplate"
        data-testid="follow-up-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
      />
    </div>
    <TranscriptionSettings v-model="transcriptionSettings" />
    <section class="flex items-center justify-between gap-2">
      <label for="capture-bitrate" class="text-sm text-slate-200">Bitrate</label>
      <SelectMenu
        id="capture-bitrate"
        v-model="bitrateKbps"
        :options="bitrateOptions"
        data-testid="bitrate-select"
      />
    </section>
    <section>
      <label class="mb-1 block text-sm text-slate-200" for="capture-input-device">
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
    </section>
    <section v-if="mode === 'meeting'">
      <label class="mb-1 block text-sm text-slate-200" for="capture-output-device">
        Desktop audio from
        <span class="block text-xs text-slate-500">Loopback output device</span>
      </label>
      <SelectMenu
        id="capture-output-device"
        v-model="outputDevice"
        :options="outputMenuOptions"
        aria-label="Desktop audio device"
        data-testid="output-device-select"
        wide
      />
    </section>
    <section>
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Tasks
      </h2>
      <label class="mb-1 block text-sm text-slate-200" for="tasks-folder">
        Tasks folder
        <span class="block text-xs text-slate-500">Inside the vault</span>
      </label>
      <div class="flex items-center gap-1">
        <input
          id="tasks-folder"
          v-model="tasksFolder"
          data-testid="tasks-folder-input"
          type="text"
          placeholder="Tasks"
          aria-label="Tasks folder"
          class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
          @keydown.enter.prevent="saveTasksFolder"
        />
        <button
          type="button"
          data-testid="tasks-folder-save"
          class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="saveTasksFolder"
        >
          Save
        </button>
      </div>
      <p
        v-if="tasksFolderError"
        data-testid="tasks-folder-error"
        class="mt-1 text-xs text-red-300"
      >
        {{ tasksFolderError }}
      </p>
    </section>
    <p
      v-if="saveError"
      data-testid="save-error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ saveError }}
    </p>
    <div class="flex items-center gap-2">
      <button
        type="submit"
        data-testid="save-button"
        class="cursor-pointer rounded-lg bg-violet-600/80 px-3 py-1 text-xs font-semibold text-white hover:bg-violet-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
        :disabled="saveState === 'saving'"
      >
        {{ saveState === "saving" ? "Saving…" : "Save" }}
      </button>
      <span v-if="saveState === 'saved'" class="text-xs text-emerald-300">
        Saved ✓
      </span>
    </div>
  </form>
</template>
