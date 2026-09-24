<script setup lang="ts">
/**
 * The tutorial editor's application header (Task 16, F-48; SCREENS-AND-
 * INTERACTIONS.md §02: "The application header owns project title/status,
 * project menu, Help, Checks, Save project and Render video"). It owns
 * project-scoped command surface — everything else (preview tools, timeline,
 * inspector) is a later task's own row/panel per that same section's
 * "persistent command ownership" rule.
 *
 * Reads `editorProject` DIRECTLY rather than taking title/duration/vault/
 * dirty as props: those are exactly the store's own committed truth (R14),
 * and prop-drilling them through `EditorShell` would just be a second copy
 * of state the store already owns for no benefit — the same reasoning
 * `ScreenCaptureBar` reads `screenCapture` directly rather than through
 * `ActionPanel` props. The layout-only bits this component does NOT own —
 * compact/theme/drawer-open — stay props from `EditorShell`, because those
 * are view state (ARCHITECTURE-AND-STACK.md's `editorWorkspace` boundary),
 * never `editorProject`'s.
 *
 * `editor-shell-title` / `editor-shell-duration` / `editor-shell-vault` keep
 * their EXACT testids from Task 15's temporary shell bar (`EditorRoot.vue`,
 * pre-Task-16) — `tests/editorRoot.test.ts` and
 * `tests/screenCaptureEditHandoff.test.ts` assert them directly and are not
 * Task 16's files to rewrite ("keeping ... every existing editor test
 * green").
 *
 * **Guide targets (Task 55; ADR R18):** the header row is `projectbar`, and
 * Save project is `header.save` (Help, Checks and Render video bind their
 * own, in their own components — `GuideHelpButton` also says "Session only"
 * when guide progress cannot be stored).
 */
import { computed, nextTick, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import type { EditorCommand, PackageFormat } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { formatDuration } from "../../../utils/formatDuration";
import AppButton from "../../ui/AppButton.vue";
import IconButton from "../../ui/IconButton.vue";
import SaveProjectDialog from "../dialogs/SaveProjectDialog.vue";
import SaveProjectMenu from "../menus/SaveProjectMenu.vue";
import ChecksButton from "./ChecksButton.vue";
import GuideHelpButton from "./GuideHelpButton.vue";
import RenderVideoButton from "./RenderVideoButton.vue";

const props = defineProps<{
  isCompact: boolean;
  libraryOpen: boolean;
  inspectorOpen: boolean;
  theme: "dark" | "light";
}>();
const emit = defineEmits<{
  (e: "toggle-library"): void;
  (e: "toggle-inspector"): void;
  (e: "toggle-theme"): void;
  (e: "open-project-file"): void;
}>();

const editorProject = useEditorProjectStore();
const projectbarTarget = useGuideTarget("projectbar");
const saveTarget = useGuideTarget("header.save");

const title = computed(() => editorProject.snapshot?.title ?? "Untitled");
const durationLabel = computed(() => formatDuration(editorProject.durationMs));
const vault = computed(() => editorProject.project?.destination.vault ?? null);

/**
 * Save/Render's disabled reasons carry a visible reason string next to the
 * button (R20: "a disabled control carries a reason string") rather than
 * relying on a hover-only `title` attribute nobody sees on a touch device or
 * without hovering — the `TaskSubtasks.vue` `disabledReason` precedent.
 */
const saveDisabledReason = computed<string | null>(() => {
  if (!editorProject.sessionId) return "No project is open.";
  if (editorProject.saving) return "Saving…";
  return null;
});
/** Render video (Task 47) is `RenderVideoButton` — its own component, with
 * its own disabled reason and the Render dialog; a render's errors never
 * reach `saveError` below (Task 46's carry). */

/**
 * Status text (Task 16's own brief: "Saved, Unsaved changes, Saving…, Save
 * failed"), derived every render from the store's own fields — never a
 * timer (this task's mutation check: faking "Saved" from a `setTimeout`
 * must read wrong against a receipt that has not actually landed yet).
 * "Save failed" reads the store's save-only `saveError` (Task 39): the
 * shared `lastError` is also set by a refused edit or open, which is not a
 * failed save.
 */
const status = computed<string>(() => {
  if (editorProject.saving) return "Saving…";
  if (editorProject.saveError) return "Save failed";
  return editorProject.dirty ? "Unsaved changes" : "Saved";
});

const editingTitle = ref(false);
const titleDraft = ref("");
const titleInput = ref<HTMLInputElement | null>(null);

function startRename() {
  titleDraft.value = title.value;
  editingTitle.value = true;
  void nextTick(() => titleInput.value?.focus());
}
function commitRename() {
  if (!editingTitle.value) return;
  editingTitle.value = false;
  const next = titleDraft.value.trim();
  if (!next || next === title.value) return;
  const command: EditorCommand = { kind: "rename", title: next };
  void editorProject.execute(command);
}
function cancelRename() {
  editingTitle.value = false;
}
function onTitleEnter() {
  commitRename();
  titleInput.value?.blur();
}

function onSave() {
  void editorProject.save();
}

/** Task 39: the Save project menu. A copy opens `SaveProjectDialog` on the
 * chosen format; opening a project file is `EditorRoot`'s (it owns which
 * project the shell is showing). */
const packageDialogOpen = ref(false);
const packageFormat = ref<PackageFormat>("portable");
function onSaveMenu(item: "save" | "portable" | "lightweight" | "open") {
  if (item === "save") onSave();
  else if (item === "open") emit("open-project-file");
  else {
    packageFormat.value = item;
    packageDialogOpen.value = true;
  }
}
</script>

<template>
  <header
    :ref="projectbarTarget"
    data-testid="editor-header"
    class="flex flex-wrap items-center gap-2 rounded-control border border-line bg-panel px-3 py-2"
  >
    <IconButton
      v-if="props.isCompact"
      label="Library"
      title="Library"
      data-testid="editor-header-library-toggle"
      :aria-expanded="props.libraryOpen"
      @click="emit('toggle-library')"
    >
      📁
    </IconButton>

    <input
      v-if="editingTitle"
      ref="titleInput"
      v-model="titleDraft"
      data-testid="editor-header-title-input"
      aria-label="Project title"
      class="min-w-0 flex-1 rounded-control border border-focus bg-raised px-2 py-1 text-sm text-fg focus:outline-none"
      @keydown.enter="onTitleEnter"
      @keydown.esc="cancelRename"
      @blur="commitRename"
    >
    <button
      v-else
      type="button"
      data-testid="editor-shell-title"
      class="max-w-[24ch] cursor-pointer truncate rounded-control px-1 text-left text-sm font-medium text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      title="Rename project"
      @click="startRename"
    >
      {{ title }}
    </button>

    <span
      data-testid="editor-shell-duration"
      class="text-micro text-fg-subtle"
    >{{ durationLabel }}</span>
    <span
      data-testid="editor-header-status"
      class="text-micro text-fg-subtle"
    >{{ status }}</span>
    <span
      v-if="vault"
      data-testid="editor-shell-vault"
      class="text-micro text-fg-subtle"
    >{{ vault }}</span>

    <div class="ml-auto flex items-center gap-2">
      <GuideHelpButton />
      <ChecksButton />
      <IconButton
        :label="props.theme === 'light' ? 'Switch to dark theme' : 'Switch to light theme'"
        data-testid="editor-header-theme-toggle"
        @click="emit('toggle-theme')"
      >
        {{ props.theme === "light" ? "🌙" : "☀️" }}
      </IconButton>
      <AppButton
        :ref="saveTarget"
        variant="secondary"
        size="sm"
        data-testid="editor-header-save"
        :disabled="Boolean(saveDisabledReason)"
        :title="saveDisabledReason ?? undefined"
        @click="onSave"
      >
        Save project
      </AppButton>
      <SaveProjectMenu
        :disabled="!editorProject.sessionId"
        :save-disabled-reason="saveDisabledReason"
        @choose="onSaveMenu"
      />
      <span
        v-if="saveDisabledReason"
        data-testid="editor-header-save-reason"
        class="text-micro text-fg-subtle"
      >{{ saveDisabledReason }}</span>
      <RenderVideoButton />
    </div>

    <IconButton
      v-if="props.isCompact"
      label="Inspector"
      title="Inspector"
      data-testid="editor-header-inspector-toggle"
      :aria-expanded="props.inspectorOpen"
      @click="emit('toggle-inspector')"
    >
      ⚙️
    </IconButton>
    <SaveProjectDialog
      :open="packageDialogOpen"
      :initial-format="packageFormat"
      @close="packageDialogOpen = false"
    />
  </header>
</template>
