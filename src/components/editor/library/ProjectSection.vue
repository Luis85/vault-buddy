<script setup lang="ts">
/**
 * The library's Project section (visual-parity Task 10; design D9; concept
 * spec §3.6): not one of the four tabs but a section of the library that
 * the Project menu's "Workspace & rendered products", the status bar's
 * right slot and the Render dialog's completion open
 * (`revealWorkspaceProducts` → the `projectSection` reveal `LibraryPanel`
 * answers). It keeps the editable project and what it rendered side by
 * side: "YOUR WORKSPACE" with the revision on screen, the project summary,
 * the rendered products (`ProductLibrary`: Watch, Publish, Restore) or
 * "No renders yet", and **Render a new video**, which opens the Render
 * dialog (the header button's own `render` reveal, refused for the same
 * reasons — `renderRefusal`). **Back to media** returns to the Media tab.
 *
 * The guide's `library.products` lesson points here (the owning
 * registration; the header's Project menu is its fallback while the
 * section is closed), and preparing that lesson opens it.
 */
import { computed } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { renderRefusal } from "../../../editor/renderRefusal";
import { requestReveal } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import LibraryButton from "./LibraryButton.vue";
import LibraryHeading from "./LibraryHeading.vue";
import ProductLibrary from "./ProductLibrary.vue";

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
/** The guide's `library.products` (Task 55; moved here by Task 10). */
const guideTarget = useGuideTarget("library.products");

const revision = computed(() => `r${editorProject.snapshot?.revision ?? 0}`);
const title = computed(() => editorProject.snapshot?.title ?? editorProject.project?.title ?? "");
const saveHint = computed(() => (editorProject.dirty ? "Unsaved changes" : "All changes saved"));
const renderReason = computed(() => renderRefusal(editorProject.sessionId, editorProject.durationMs));

function renderNew(): void {
  if (renderReason.value === null) requestReveal("render");
}
</script>

<template>
  <div
    :ref="guideTarget"
    data-testid="library-project-section"
    class="h-full overflow-y-auto text-fg"
  >
    <button
      type="button"
      data-testid="library-project-back"
      class="mb-2.5 inline-flex min-h-0 items-center gap-1.5 px-0 py-1 text-[10px] text-fg-muted hover:bg-transparent hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="workspace.setLibraryTab('media')"
    >
      <EditorIcon
        name="arrowLeft"
        :size="14"
      />
      Back to media
    </button>
    <LibraryHeading
      label="YOUR WORKSPACE"
      :pill="revision"
      testid="project-section"
      accent
    />
    <div
      data-testid="project-summary"
      class="mb-2.5 mt-1 flex flex-col gap-2 rounded-[9px] border border-line p-4"
    >
      <EditorIcon
        name="folder"
        :size="25"
        class="text-accent"
      />
      <b class="break-words text-[12px] font-semibold">{{ title }}</b>
      <span class="text-[10px] text-fg-muted">{{ saveHint }}</span>
    </div>
    <p class="mb-4 text-[11px] leading-relaxed text-fg-secondary">
      The project is your working file. Rendered videos are its products.
    </p>
    <ProductLibrary />
    <LibraryButton
      icon="video"
      data-testid="project-section-render"
      class="mt-1 justify-center"
      :disabled="renderReason !== null"
      :title="renderReason ?? 'Open the Render dialog'"
      @click="renderNew"
    >
      Render a new video
    </LibraryButton>
  </div>
</template>
