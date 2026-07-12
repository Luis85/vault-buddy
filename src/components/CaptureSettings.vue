<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref, watch } from "vue";

import { useOptionalFolderField } from "../composables/useOptionalFolderField";
import { logWarning } from "../logging";
import type {
  AudioDevices,
  CaptureConfig,
  DocumentsConfig,
  TasksConfig,
} from "../types";
import RecordingSettings from "./RecordingSettings.vue";
import TaskListSettings from "./TaskListSettings.vue";
import VaultFolderSetting from "./VaultFolderSetting.vue";

const props = defineProps<{ vaultId: string }>();

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
const meetingFolder = ref("");
const voiceNoteFolder = ref("");
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
// Whether NEW recordings land in a dated YYYY/MM subfolder. Part of the same
// CaptureConfig load/save round trip as the fields above (no separate
// loaded/edited race to guard — get_capture_config/set_capture_config always
// carry the whole config together).
const recordingDateFolders = ref(true);

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
// The lists card below (TaskListSettings) reads its lists + config only at
// mount, but the tasks folder saved HERE decides which root those lists live
// under — a persisted folder change swaps the lists universe out from under
// the card, and saving a default/order from the stale card would persist
// old-root list names against the new root (a later unpicked add would then
// create that list there). savedTasksFolder is the last value known persisted
// (null until the load reports it); a successful save that changes it bumps
// listsCardNonce, whose :key remounts the card to reload. An unchanged save
// leaves the card — and any unsaved edits in it — alone (Codex, PR #53
// re-review).
const savedTasksFolder = ref<string | null>(null);
const listsCardNonce = ref(0);

// The per-vault documents folder — same independent-command shape as
// tasksFolder above (its own get/set_documents_config pair, saved with the
// form's single Save button as its own independent invoke, gated the same
// way so a documents-config failure can't block the capture or tasks save
// and vice versa).
const documentsFolder = ref(""); // "" shows the "Documents" placeholder / clears to default
const documentsFolderError = ref<string | null>(null);
const documentsFolderLoaded = ref(false);
const documentsFolderEdited = ref(false);
// Whether NEW imports land in a dated YYYY/MM subfolder. Rides the SAME
// get_documents_config/set_documents_config round trip as documentsFolder
// above (one command carries both fields), so it shares that field's
// loaded/edited gate rather than needing one of its own.
const documentDateFolders = ref(true);

// Bundles the recording/note/transcription/device fields for
// RecordingSettings' v-model. The setter fans a merged update back out to
// the individual refs so save()/onMounted()/watch (below) keep working on
// them unchanged — this computed is purely an adapter for the extracted
// controlled component, same idiom as the former transcriptionSettings
// adapter it replaces (RecordingSettings now owns that nested adapter).
const recordingBundle = computed({
  get: () => ({
    meetingFolder: meetingFolder.value,
    voiceNoteFolder: voiceNoteFolder.value,
    bitrateKbps: bitrateKbps.value,
    createNote: createNote.value,
    followUpTemplate: followUpTemplate.value,
    inputDevice: inputDevice.value,
    outputDevice: outputDevice.value,
    transcribe: transcribe.value,
    transcriptionModel: transcriptionModel.value,
    transcriptionLanguage: transcriptionLanguage.value,
    transcriptTimestamps: transcriptTimestamps.value,
    recordingDateFolders: recordingDateFolders.value,
  }),
  set: (v: {
    meetingFolder: string;
    voiceNoteFolder: string;
    bitrateKbps: number;
    createNote: boolean;
    followUpTemplate: boolean;
    inputDevice: string;
    outputDevice: string;
    transcribe: boolean;
    transcriptionModel: string;
    transcriptionLanguage: string;
    transcriptTimestamps: boolean;
    recordingDateFolders: boolean;
  }) => {
    meetingFolder.value = v.meetingFolder;
    voiceNoteFolder.value = v.voiceNoteFolder;
    bitrateKbps.value = v.bitrateKbps;
    createNote.value = v.createNote;
    followUpTemplate.value = v.followUpTemplate;
    inputDevice.value = v.inputDevice;
    outputDevice.value = v.outputDevice;
    transcribe.value = v.transcribe;
    transcriptionModel.value = v.transcriptionModel;
    transcriptionLanguage.value = v.transcriptionLanguage;
    transcriptTimestamps.value = v.transcriptTimestamps;
    recordingDateFolders.value = v.recordingDateFolders;
  },
});

// Any edit invalidates the "Saved ✓" confirmation. During the initial load
// saveState is already "idle", so the load-time assignments are idle→idle
// no-ops; this only becomes visible after a save set it to "saved".
watch(
  [
    meetingFolder,
    voiceNoteFolder,
    createNote,
    followUpTemplate,
    bitrateKbps,
    inputDevice,
    outputDevice,
    transcribe,
    transcriptionModel,
    transcriptionLanguage,
    transcriptTimestamps,
    recordingDateFolders,
    tasksFolder,
    documentsFolder,
    documentDateFolders,
  ],
  () => {
    if (saveState.value === "saved") saveState.value = "idle";
  },
);

// The optional folders' shared load/save pair lives in the composable so the
// tasks and documents reads/writes can't drift apart (and this form stays
// under its LOC cap).
const { loadOptionalField, saveOptionalField } = useOptionalFolderField(() => props.vaultId);

onMounted(async () => {
  try {
    const [cfg, devs] = await Promise.all([
      invoke<CaptureConfig>("get_capture_config", { id: props.vaultId }),
      invoke<AudioDevices>("list_audio_devices"),
    ]);
    mode.value = cfg.mode;
    meetingFolder.value = cfg.meetingFolder ?? "";
    voiceNoteFolder.value = cfg.voiceNoteFolder ?? "";
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
    recordingDateFolders.value = cfg.recordingDateFolders;
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
  // Separate invokes (not in the Promise.all above) so an optional-folder read
  // can't block the capture form from loading — both folders are optional.
  await loadOptionalField<TasksConfig>({
    cmd: "get_tasks_config",
    editedRef: tasksFolderEdited,
    loadedRef: tasksFolderLoaded,
    targetRef: tasksFolder,
    extract: (cfg) => cfg.tasksFolder,
    onPersisted: (persisted) => (savedTasksFolder.value = persisted),
  });
  await loadOptionalField<DocumentsConfig>({
    cmd: "get_documents_config",
    editedRef: documentsFolderEdited,
    loadedRef: documentsFolderLoaded,
    targetRef: documentsFolder,
    extract: (cfg) => cfg.documentsFolder,
    onLoaded: (cfg) => {
      // Same "don't clobber an edit with a late-resolving load" rule the
      // composable applies to documentsFolder itself — this field rides the
      // same command's response, so it shares that guard.
      if (!documentsFolderEdited.value) documentDateFolders.value = cfg.documentDateFolders;
    },
  });
});

async function save() {
  saveState.value = "saving";
  saveError.value = null;
  folderError.value = null;
  tasksFolderError.value = null;
  documentsFolderError.value = null;
  let failed = false;
  try {
    await invoke("set_capture_config", {
      id: props.vaultId,
      cfg: {
        mode: mode.value,
        meetingFolder: meetingFolder.value.trim() || null,
        voiceNoteFolder: voiceNoteFolder.value.trim() || null,
        bitrateKbps: bitrateKbps.value,
        createNote: createNote.value,
        followUpTemplate: followUpTemplate.value,
        inputDevice: inputDevice.value || null,
        outputDevice: outputDevice.value || null,
        transcribe: transcribe.value,
        transcriptionModel: transcriptionModel.value,
        transcriptionLanguage: transcriptionLanguage.value.trim() || null,
        transcriptTimestamps: transcriptTimestamps.value,
        recordingDateFolders: recordingDateFolders.value,
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
  // Mirror saveOptionalField's own run-gate: only a save that actually RAN
  // (loaded-or-edited) can have persisted a change worth reloading the lists
  // card for. `false` from the helper also means "skipped".
  const tasksSaveRan = tasksFolderLoaded.value || tasksFolderEdited.value;
  const savingTasksFolder = tasksFolder.value.trim();
  if (
    await saveOptionalField({
      cmd: "set_tasks_config",
      key: "tasksFolder",
      value: tasksFolder.value,
      loaded: tasksFolderLoaded.value,
      edited: tasksFolderEdited.value,
      errorRef: tasksFolderError,
    })
  ) {
    failed = true;
  } else if (tasksSaveRan && savingTasksFolder !== savedTasksFolder.value) {
    // Persisted value changed (a null baseline — failed load — counts as
    // changed, the conservative side): the lists card's root moved; remount
    // it so its lists/config reload against the new root.
    savedTasksFolder.value = savingTasksFolder;
    listsCardNonce.value += 1;
  }
  if (
    await saveOptionalField({
      cmd: "set_documents_config",
      key: "documentsFolder",
      value: documentsFolder.value,
      loaded: documentsFolderLoaded.value,
      edited: documentsFolderEdited.value,
      errorRef: documentsFolderError,
      extra: { documentDateFolders: documentDateFolders.value },
    })
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
    <!-- Three domain super-groups, one level above the buddy-settings-style
         sub-cards below (Companion note, Tasks folder, …): each carries its
         own data-testid and a domain h2 as ITS OWN first heading. -->
    <section data-testid="group-recording">
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Recording
      </h2>
      <!-- Plain wrapper, not another bordered card: RecordingSettings already
           renders its own bordered sub-cards (Folders, Audio, Companion note,
           Transcription), same as VaultFolderSetting/
           TaskListSettings do for the Tasks/Documents groups below — an
           extra border here would double-nest around each of them. -->
      <div class="flex flex-col gap-3">
        <RecordingSettings
          v-model="recordingBundle"
          :devices="devices"
          :folder-error="folderError"
        />
      </div>
    </section>
    <section data-testid="group-tasks">
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Tasks
      </h2>
      <div class="flex flex-col gap-3">
        <!-- heading is the field label, not "Tasks" again — the group h2
             above already carries the domain name. -->
        <VaultFolderSetting
          v-model="tasksFolder"
          heading="Tasks folder"
          label="Tasks folder"
          placeholder="Tasks"
          input-id="tasks-folder"
          input-testid="tasks-folder-input"
          error-testid="tasks-folder-error"
          :error="tasksFolderError"
          @edit="tasksFolderEdited = true"
        />
        <!-- Self-contained (own load/save) so its lists-config failure can't
             block the capture/folder saves — the independent-save pattern. -->
        <TaskListSettings
          :key="listsCardNonce"
          :vault-id="vaultId"
        />
      </div>
    </section>
    <section data-testid="group-documents">
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Documents
      </h2>
      <div class="flex flex-col gap-3">
        <!-- heading is the field label, not "Documents" again — the group h2
             above already carries the domain name. -->
        <VaultFolderSetting
          v-model="documentsFolder"
          heading="Documents folder"
          label="Documents folder"
          placeholder="Documents"
          input-id="documents-folder"
          input-testid="documents-folder-input"
          error-testid="documents-folder-error"
          :error="documentsFolderError"
          @edit="documentsFolderEdited = true"
        />
        <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
          <label
            for="document-date-folders"
            class="text-sm text-slate-200"
          >
            Organize into year/month folders
            <span class="block text-xs text-slate-500">Off = one flat folder</span>
          </label>
          <input
            id="document-date-folders"
            v-model="documentDateFolders"
            data-testid="document-date-folders-toggle"
            type="checkbox"
            class="h-4 w-4 accent-violet-500"
            @change="documentsFolderEdited = true"
          >
        </div>
      </div>
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
      <span
        v-if="saveState === 'saved'"
        class="text-xs text-emerald-300"
      >
        Saved ✓
      </span>
    </div>
  </form>
</template>
