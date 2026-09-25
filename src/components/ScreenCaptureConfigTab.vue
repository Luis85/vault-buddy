<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { useSettingsLoad } from "../composables/useSettingsLoad";
import type { ScreenCaptureConfig } from "../types";
import SelectMenu from "./SelectMenu.vue";
import Banner from "./ui/Banner.vue";
import VaultFolderSetting from "./VaultFolderSetting.vue";

// The Screen Capture tab of Vault settings (spec §12, docs/Gaps.md GAP-103).
// Self-contained like its three siblings: loads its own config, auto-saves
// every field through the one set_screen_capture_config command. A failed
// read shows an inline error and NO editable fields, so a seeded default can
// never be auto-saved over a value we failed to read.
//
// Until this landed all seven fields (now eight) were config.json hand-edits — READ in
// production and settable nowhere. Since Task 59 retired the phase-5 export,
// quality, fps and stems are read by the capture worker, and the folder and
// the two templates by the tutorial editor's Publish; the date-folder and
// note toggles are read by nothing (docs/Gaps.md GAP-211).
const props = defineProps<{ vaultId: string }>();

const { loading, loadError, load } = useSettingsLoad();
const screenCaptureFolder = ref("");
const screenCaptureDateFolders = ref(false);
const screenQuality = ref("balanced");
const screenFps = ref(30);
const screenCreateNote = ref(true);
const screenExtraFrontmatter = ref("");
const screenBodyTemplate = ref("");
const screenAudioStems = ref(false);

// SelectMenu, not a native <select>, and that is the point rather than a
// preference. A <select>'s option popup is drawn by the OS, and it takes its
// surface from the control's own background -- `bg-white/5` composites to
// near-white, on which the panel's `text-fg` (slate-100) is invisible. The
// page already sets `color-scheme: dark`, which is not enough once an
// explicit background is in play. Every other dropdown in the app is a
// SelectMenu for this reason; this tab was the only raw <select> left.
//
// The option VALUES are the keys the Rust parser reads -- one spelling for
// the wire, config.json and ScreenQuality::from_key -- and the fps values
// stay NUMBERS, since SelectMenu emits the option's own value and screenFps
// crosses IPC as a u32 that serde will not coerce from a string.
const QUALITIES = [
  { value: "low", label: "Low — smallest files" },
  { value: "balanced", label: "Balanced" },
  { value: "high", label: "High — sharpest text" },
];

// Rust REFUSES anything but these two rather than normalising, so the
// control offers exactly them and cannot send a third.
const FRAME_RATES = [
  { value: 30, label: "30 fps" },
  { value: 60, label: "60 fps" },
];

// Written in a script string, never inline in template text: Vue's mustache
// tokenizer finds the FIRST `}}` textually, so typing it into the markup
// would terminate the interpolation early and corrupt it. Same note as
// RecordingSettings.vue and DocumentsConfigTab.vue.
//
// The six names are READ OFF core::editor::note's own `vars` array — the
// Tutorial note a Publish writes, the one reader of these templates since
// Task 59 retired the phase-5 export's Screen Capture note: both fields draw
// from the SAME set, and there is no {{title}} and no {{embed}} — the body
// template is appended AFTER the embed rather than wrapping it.
const TEMPLATE_PLACEHOLDER_HINT =
  "Placeholders for both fields: {{date}}, {{recordedAt}}, {{duration}}, {{product}}, {{revision}}, {{range}}. Identity frontmatter (type, created-by, recorded, duration, product, revision, range, chapters) is always written and cannot be overridden.";

const autosave = useAutosave(
  async () => {
    await invoke("set_screen_capture_config", {
      id: props.vaultId,
      cfg: {
        screenCaptureFolder: screenCaptureFolder.value.trim() || null,
        screenCaptureDateFolders: screenCaptureDateFolders.value,
        screenQuality: screenQuality.value,
        screenFps: screenFps.value,
        screenCreateNote: screenCreateNote.value,
        screenExtraFrontmatter: screenExtraFrontmatter.value.trim() || null,
        screenBodyTemplate: screenBodyTemplate.value.trim() || null,
        screenAudioStems: screenAudioStems.value,
      },
    });
  },
  { label: "screen capture settings" },
);

onMounted(() =>
  load<ScreenCaptureConfig>(
    "get_screen_capture_config",
    props.vaultId,
    (cfg) => {
      screenCaptureFolder.value = cfg.screenCaptureFolder ?? "";
      screenCaptureDateFolders.value = cfg.screenCaptureDateFolders;
      screenQuality.value = cfg.screenQuality;
      screenFps.value = cfg.screenFps;
      screenCreateNote.value = cfg.screenCreateNote;
      screenExtraFrontmatter.value = cfg.screenExtraFrontmatter ?? "";
      screenBodyTemplate.value = cfg.screenBodyTemplate ?? "";
      screenAudioStems.value = cfg.screenAudioStems ?? false;
    },
  ),
);

// Typed fields debounce; toggles and selects save immediately. onMounted
// assigns the refs directly (not via these handlers), so none fire on load.
function onFolderInput(value: string) {
  screenCaptureFolder.value = value;
  autosave.schedule();
}
function onDateFoldersToggle(event: Event) {
  screenCaptureDateFolders.value = (event.target as HTMLInputElement).checked;
  autosave.saveNow();
}
function onCreateNoteToggle(event: Event) {
  screenCreateNote.value = (event.target as HTMLInputElement).checked;
  autosave.saveNow();
}
function onAudioStemsToggle(event: Event) {
  screenAudioStems.value = (event.target as HTMLInputElement).checked;
  autosave.saveNow();
}
function onQualityChange(value: string | number) {
  screenQuality.value = String(value);
  autosave.saveNow();
}
function onFpsChange(value: string | number) {
  screenFps.value = Number(value);
  autosave.saveNow();
}
function onExtraFrontmatterInput(event: Event) {
  screenExtraFrontmatter.value = (event.target as HTMLTextAreaElement).value;
  autosave.schedule();
}
function onBodyTemplateInput(event: Event) {
  screenBodyTemplate.value = (event.target as HTMLTextAreaElement).value;
  autosave.schedule();
}
</script>

<template>
  <!-- focusout flushes a pending debounced save when focus leaves. -->
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
      tone="danger"
      data-testid="screen-load-error"
    >
      {{ loadError }}
    </Banner>
    <template v-else>
      <VaultFolderSetting
        :model-value="screenCaptureFolder"
        heading="Screen captures folder"
        label="Screen captures folder"
        placeholder="Screen Captures"
        input-id="screen-capture-folder"
        input-testid="screen-capture-folder-input"
        error-testid="screen-capture-folder-error"
        :error="autosave.error.value"
        @update:model-value="onFolderInput"
      />
      <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          for="screen-date-folders"
          class="text-sm text-slate-200"
        >
          Organize into year/month folders
          <span class="block text-xs text-fg-subtle">Off = one flat folder</span>
        </label>
        <input
          id="screen-date-folders"
          data-testid="screen-date-folders-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="screenCaptureDateFolders"
          @change="onDateFoldersToggle"
        >
      </div>
      <div class="flex flex-col gap-1 rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          class="text-sm text-slate-200"
          for="screen-quality"
        >
          Quality
        </label>
        <SelectMenu
          id="screen-quality"
          data-testid="screen-quality-select"
          aria-label="Quality"
          wide
          :model-value="screenQuality"
          :options="QUALITIES"
          @update:model-value="onQualityChange"
        />
        <p class="text-xs text-fg-subtle">
          Applies to the NEXT recording. A render picks its own quality in the
          editor's Render dialog.
        </p>
      </div>
      <div class="flex flex-col gap-1 rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          class="text-sm text-slate-200"
          for="screen-fps"
        >
          Frame rate
        </label>
        <SelectMenu
          id="screen-fps"
          data-testid="screen-fps-select"
          aria-label="Frame rate"
          wide
          :model-value="screenFps"
          :options="FRAME_RATES"
          @update:model-value="onFpsChange"
        />
        <p class="text-xs text-fg-subtle">
          Applies to the NEXT recording, not one already staged.
        </p>
      </div>
      <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          for="screen-create-note"
          class="text-sm text-slate-200"
        >
          Write a companion note
          <span class="block text-xs text-fg-subtle">Off = the video only</span>
        </label>
        <input
          id="screen-create-note"
          data-testid="screen-create-note-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="screenCreateNote"
          @change="onCreateNoteToggle"
        >
      </div>
      <!-- Task 53: HOW a capture is recorded, so it cannot reach one already
           staged; the label says so rather than leaving it to be discovered. -->
      <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          for="screen-audio-stems"
          class="text-sm text-slate-200"
        >
          Keep each audio input as a separate track (new recordings only)
          <span class="block text-xs text-fg-subtle">The mixed track is always recorded too</span>
        </label>
        <input
          id="screen-audio-stems"
          data-testid="screen-audio-stems-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="screenAudioStems"
          @change="onAudioStemsToggle"
        >
      </div>
      <div class="flex flex-col gap-1 rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          class="text-sm text-slate-200"
          for="screen-extra-frontmatter"
        >
          Extra frontmatter
        </label>
        <textarea
          id="screen-extra-frontmatter"
          data-testid="screen-extra-frontmatter"
          :value="screenExtraFrontmatter"
          rows="3"
          placeholder="area: Demos"
          class="w-full resize-y rounded-control border border-white/10 bg-white/5 px-2 py-1 font-mono text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
          @input="onExtraFrontmatterInput"
        />
        <p class="text-xs text-fg-subtle">
          {{ TEMPLATE_PLACEHOLDER_HINT }}
        </p>
      </div>
      <div class="flex flex-col gap-1 rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          class="text-sm text-slate-200"
          for="screen-body-template"
        >
          Body template
          <span class="block text-xs text-fg-subtle">Added below the video embed</span>
        </label>
        <textarea
          id="screen-body-template"
          data-testid="screen-body-template"
          :value="screenBodyTemplate"
          rows="3"
          placeholder="## Notes"
          class="w-full resize-y rounded-control border border-white/10 bg-white/5 px-2 py-1 font-mono text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
          @input="onBodyTemplateInput"
        />
        <p class="text-xs text-fg-subtle">
          {{ TEMPLATE_PLACEHOLDER_HINT }}
        </p>
      </div>
    </template>
  </div>
</template>
