<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, type Ref, ref, watch } from "vue";

import { logWarning } from "../logging";
import type {
  AudioDevice,
  AudioDevices,
  CaptureConfig,
  DocumentsConfig,
  TasksConfig,
} from "../types";
import SelectMenu from "./SelectMenu.vue";
import TranscriptionSettings from "./TranscriptionSettings.vue";
import VaultFolderSetting from "./VaultFolderSetting.vue";

const props = defineProps<{ vaultId: string }>();

const BITRATES = [128, 160, 192];

const loading = ref(true);
const loadError = ref<string | null>(null);
const saveState = ref<"idle" | "saving" | "saved">("idle");
const saveError = ref<string | null>(null);
const folderError = ref<string | null>(null);

// Pass-through only: the "default recording mode" control is gone (the mode
// is a per-recording choice in the Record view), but the loaded value is
// still sent back unchanged on save so the IPC contract and config.json
// schema stay as they are.
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

// The per-vault tasks folder lives in the same app-side config but keeps its
// own command pair (the capture-config save already preserves tasks_folder).
// It saves with the form's single Save button, as an independent invoke in
// save() below — so a tasks-config failure can't block the capture save and
// vice versa; its errors stay field-level.
const tasksFolder = ref(""); // "" shows the "Tasks" placeholder / clears to default
const tasksFolderError = ref<string | null>(null);
// Gate for save()'s tasks write. The form is submittable BEFORE the
// get_tasks_config read below resolves (it runs after the capture `loading`
// gate flips, so the form never blocks on it) — and stays usable after a
// failed read. Writing unconditionally would send the default-seeded ""
// (→ null) and clear a configured folder this mount never saw. So the write
// requires the value to have actually loaded, or an explicit user edit
// (typed input is explicit intent, even when the read failed). Mirrors
// RecordMode.vue's `loaded` persist gate.
const tasksFolderLoaded = ref(false);
const tasksFolderEdited = ref(false);

// The per-vault documents folder — same independent-command shape as
// tasksFolder above (its own get/set_documents_config pair, saved with the
// form's single Save button as its own independent invoke, gated the same
// way so a documents-config failure can't block the capture or tasks save
// and vice versa).
const documentsFolder = ref(""); // "" shows the "Documents" placeholder / clears to default
const documentsFolderError = ref<string | null>(null);
const documentsFolderLoaded = ref(false);
const documentsFolderEdited = ref(false);

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
    tasksFolder,
    documentsFolder,
  ],
  () => {
    if (saveState.value === "saved") saveState.value = "idle";
  },
);

// Load an optional per-vault folder field through its own command, off the
// capture form's critical path: a failure warns and continues (the folder is
// optional), and the resolved value is dropped if the user already started
// typing (their edit owns the field — the same rule as RecordMode's pre-load
// toggle guard). Shared verbatim by the tasks and documents folders so the
// two reads can't drift apart.
async function loadOptionalField<T>(
  cmd: string,
  editedRef: Ref<boolean>,
  loadedRef: Ref<boolean>,
  targetRef: Ref<string>,
  extract: (cfg: T) => string | null,
) {
  try {
    const cfg = await invoke<T>(cmd, { id: props.vaultId });
    if (!editedRef.value) targetRef.value = extract(cfg) ?? "";
    loadedRef.value = true;
  } catch (e) {
    logWarning(`${cmd} failed (vault ${props.vaultId}): ${String(e)}`);
  }
}

// Save an optional per-vault folder field through its own command. Gated on
// loaded-or-edited: a value that is neither is the default seed, and writing
// it would clear the vault's real folder. A failure is a field-level error
// (returned so the caller can withhold the "Saved ✓") — deliberately NOT
// short-circuited by the capture-config save, so neither write can block the
// other's. Shared verbatim by the tasks and documents folders.
async function saveOptionalField(
  cmd: string,
  key: string,
  value: string,
  loaded: boolean,
  edited: boolean,
  errorRef: Ref<string | null>,
): Promise<boolean> {
  if (!loaded && !edited) return false;
  const trimmed = value.trim();
  try {
    await invoke(cmd, { id: props.vaultId, [key]: trimmed === "" ? null : trimmed });
    return false;
  } catch (e) {
    errorRef.value = String(e);
    logWarning(`${cmd} failed (vault ${props.vaultId}): ${String(e)}`);
    return true;
  }
}

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
  // Separate invokes (not in the Promise.all above) so an optional-folder read
  // can't block the capture form from loading — both folders are optional.
  await loadOptionalField<TasksConfig>(
    "get_tasks_config",
    tasksFolderEdited,
    tasksFolderLoaded,
    tasksFolder,
    (cfg) => cfg.tasksFolder,
  );
  await loadOptionalField<DocumentsConfig>(
    "get_documents_config",
    documentsFolderEdited,
    documentsFolderLoaded,
    documentsFolder,
    (cfg) => cfg.documentsFolder,
  );
});

async function save() {
  saveState.value = "saving";
  saveError.value = null;
  folderError.value = null;
  tasksFolderError.value = null;
  documentsFolderError.value = null;
  const folder = recordingFolder.value.trim();
  let failed = false;
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
  } catch (e) {
    failed = true;
    // Folder rejections are field-level; everything else is form-level.
    // Form state is preserved either way so the user can correct and retry.
    const message = String(e);
    if (message.toLowerCase().includes("folder")) folderError.value = message;
    else saveError.value = message;
    logWarning(`capture settings save failed (vault ${props.vaultId}): ${message}`);
  }
  // Both optional folders save with the same button through their own
  // commands — each independent (a failure of one can't block the other or
  // the capture save) and each a field-level error. The `|| failed` ordering
  // keeps a prior failure sticky while still ALWAYS attempting both writes.
  if (
    await saveOptionalField(
      "set_tasks_config",
      "tasksFolder",
      tasksFolder.value,
      tasksFolderLoaded.value,
      tasksFolderEdited.value,
      tasksFolderError,
    )
  ) {
    failed = true;
  }
  if (
    await saveOptionalField(
      "set_documents_config",
      "documentsFolder",
      documentsFolder.value,
      documentsFolderLoaded.value,
      documentsFolderEdited.value,
      documentsFolderError,
    )
  ) {
    failed = true;
  }
  // "Saved ✓" must mean the WHOLE form landed — either failure withholds it.
  saveState.value = failed ? "idle" : "saved";
}
</script>

<template>
  <p
    v-if="loading"
    class="text-xs text-slate-400"
  >
    Loading…
  </p>
  <p
    v-else-if="loadError"
    class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
  >
    {{ loadError }}
  </p>
  <form
    v-else
    class="flex flex-col gap-3"
    @submit.prevent="save"
  >
    <section>
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
    </section>
    <section class="flex items-center justify-between">
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
    </section>
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
    <TranscriptionSettings v-model="transcriptionSettings" />
    <section class="flex items-center justify-between gap-2">
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
    </section>
    <section>
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
    </section>
    <section>
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
    </section>
    <VaultFolderSetting
      v-model="tasksFolder"
      heading="Tasks"
      label="Tasks folder"
      placeholder="Tasks"
      input-id="tasks-folder"
      input-testid="tasks-folder-input"
      error-testid="tasks-folder-error"
      :error="tasksFolderError"
      @edit="tasksFolderEdited = true"
    />
    <VaultFolderSetting
      v-model="documentsFolder"
      heading="Document import"
      label="Documents folder"
      placeholder="Documents"
      input-id="documents-folder"
      input-testid="documents-folder-input"
      error-testid="documents-folder-error"
      :error="documentsFolderError"
      @edit="documentsFolderEdited = true"
    />
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
      <span
        v-if="saveState === 'saved'"
        class="text-xs text-emerald-300"
      >
        Saved ✓
      </span>
    </div>
  </form>
</template>
