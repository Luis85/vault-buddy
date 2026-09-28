<script setup lang="ts">
/**
 * The tutorial editor's application header (Task 16, F-48; visual-parity
 * Task 8: design D6, D7, D8, D10; concept spec §2, screens 01–02). Left to
 * right: the brand (`BrandMark`), the **Project** menu
 * (`ProjectMenuButton`), the document title — a button that opens Rename
 * tutorial, its pencil showing on hover and focus — and the actions: the
 * save state (`SaveStateIndicator`), Help, Checks, **Save project**
 * (bordered) and **Render video** (primary). 14px gaps (8 at or below
 * 1350px wide), 16px padding, on
 * `panel` with a `line` rule below; its height is the frame's first grid
 * row (56, or 52 in a short window).
 *
 * Save project keeps its native meaning (D8): it commits the session to the
 * project store through `useProjectSave`, the one path Ctrl+S and the
 * status bar share; its disabled reason shows beside it (R20). The Save ▾
 * split button is gone: its copies, Open a project file and Discard moved
 * into the Project menu (D6/D7), whose "Save a copy as project file…" opens
 * `SaveProjectDialog` from here. The destination vault's id is no longer
 * shown (the concept shows none; Render and Publish name the vault).
 *
 * The theme toggle left too: the preview header's View menu carries
 * "Light theme" (ruling P5, visual-parity Task 11). The library/inspector
 * drawer toggles left the header: the preview header's panel toggles open
 * the drawers at every width (design D5).
 *
 * Reads `editorProject` directly rather than taking title/dirty as props:
 * those are the store's own committed truth (R14).
 *
 * **Guide targets (Task 55; ADR R18):** the header row is `projectbar`, and
 * Save project is `header.save` (Help, Checks and Render video bind their
 * own, in their own components — `GuideHelpButton` also says "Session only"
 * when guide progress cannot be stored).
 */
import { computed, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useProjectSave } from "../../../composables/useProjectSave";
import { HEADER_COMPACT_MAX_WIDTH } from "../../../editor/panelLayout";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import RenameDialog from "../dialogs/RenameDialog.vue";
import SaveProjectDialog from "../dialogs/SaveProjectDialog.vue";
import EditorIcon from "../icons/EditorIcon.vue";
import BrandMark from "./BrandMark.vue";
import ChecksButton from "./ChecksButton.vue";
import GuideHelpButton from "./GuideHelpButton.vue";
import HeaderButton from "./HeaderButton.vue";
import ProjectMenuButton from "./ProjectMenuButton.vue";
import RenderVideoButton from "./RenderVideoButton.vue";
import SaveStateIndicator from "./SaveStateIndicator.vue";

const emit = defineEmits<{
  (e: "open-project", projectFileId: string): void;
  (e: "open-project-file"): void;
  (e: "discard-project"): void;
}>();

const editorProject = useEditorProjectStore();
const projectbarTarget = useGuideTarget("projectbar");
const saveTarget = useGuideTarget("header.save");

/** §1.4: at or below 1350px wide the header closes up to 8px gaps. */
const workspace = useEditorWorkspaceStore();
const gap = computed(() => (workspace.viewportWidth > HEADER_COMPACT_MAX_WIDTH ? "gap-3.5" : "gap-2"));

const title = computed(() => editorProject.snapshot?.title ?? "Untitled");
const { disabledReason: saveDisabledReason, save: onSave } = useProjectSave();

const renameOpen = ref(false);
const copyOpen = ref(false);
</script>

<template>
  <header
    :ref="projectbarTarget"
    data-testid="editor-header"
    class="flex min-w-0 items-center overflow-hidden border-b border-line bg-panel px-4"
    :class="gap"
  >
    <BrandMark />
    <ProjectMenuButton
      @open-project="(id) => emit('open-project', id)"
      @open-project-file="emit('open-project-file')"
      @rename="renameOpen = true"
      @save-copy="copyOpen = true"
      @discard-project="emit('discard-project')"
    />

    <div class="flex min-w-0 flex-1">
      <button
        type="button"
        data-testid="editor-header-title"
        title="Rename tutorial"
        class="group flex min-w-0 max-w-full items-center gap-1.5 border-transparent bg-transparent px-1.5 text-left text-sm font-semibold tracking-[-0.1px] text-fg"
        @click="renameOpen = true"
      >
        <span class="truncate">{{ title }}</span>
        <EditorIcon
          name="edit"
          :size="13"
          class="shrink-0 text-fg-muted opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100"
        />
      </button>
    </div>

    <div class="flex shrink-0 items-center gap-[7px]">
      <SaveStateIndicator />
      <GuideHelpButton />
      <ChecksButton />
      <HeaderButton
        :ref="saveTarget"
        icon="save"
        variant="bordered"
        data-testid="editor-header-save"
        :disabled="Boolean(saveDisabledReason)"
        :title="saveDisabledReason ?? 'Save project (Ctrl+S)'"
        @click="onSave"
      >
        Save project
      </HeaderButton>
      <span
        v-if="saveDisabledReason"
        data-testid="editor-header-save-reason"
        class="text-micro text-fg-subtle"
      >{{ saveDisabledReason }}</span>
      <RenderVideoButton />
    </div>

    <RenameDialog
      :open="renameOpen"
      @close="renameOpen = false"
    />
    <SaveProjectDialog
      :open="copyOpen"
      initial-format="portable"
      @close="copyOpen = false"
    />
  </header>
</template>
