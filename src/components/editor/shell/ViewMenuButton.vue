<script setup lang="ts">
/**
 * The preview header's **View options** (visual-parity Task 11; concept
 * spec §4.1 item 5, design D5, D1): a ⋯ icon button opening "View &
 * workspace" in the one `MenuPanel` — Show media library, Show properties
 * and Focus preview (checkboxes showing the panels' real state, D5),
 * Reset panel layout, Light theme (a checkbox; the header's sun/moon
 * toggle it replaced is gone, ruling P5), Audio mixer… and Keyboard
 * shortcuts & help…. None of it is an edit: the panels and the theme are
 * `editorWorkspace` state (persisted with the workspace), the mixer and
 * the learning center are opened through the reveal bus by the components
 * that own them (`MixerPopover`, `GuideHelpButton`).
 */
import { computed, ref } from "vue";

import { VIEW_MENU_HEADING, VIEW_MENU_SUBTITLE, viewMenuItems } from "../../../editor/previewHeader";
import { requestReveal } from "../../../editor/revealBus";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import MenuPanel from "../menus/MenuPanel.vue";

const workspace = useEditorWorkspaceStore();
const trigger = ref<HTMLButtonElement | null>(null);
const open = ref(false);

const items = computed(() =>
  viewMenuItems({
    libraryVisible: workspace.libraryVisible,
    inspectorVisible: workspace.inspectorVisible,
    focusPreview: workspace.focusPreview,
    lightTheme: workspace.theme === "light",
    toggleLibrary: () => workspace.toggleLibrary(),
    toggleInspector: () => workspace.toggleInspector(),
    toggleFocusPreview: () => workspace.toggleFocusPreview(),
    resetLayout: () => workspace.resetPanelLayout(),
    toggleTheme: () => workspace.toggleTheme(),
    openMixer: () => requestReveal("mixer"),
    openShortcuts: () => requestReveal("shortcuts"),
  }),
);

/** The open menu closes itself on a press outside it; the trigger is
 * outside it, so without this a press there would close and reopen it. */
function onPointerDown(event: PointerEvent): void {
  if (open.value) event.stopPropagation();
}
</script>

<template>
  <button
    ref="trigger"
    type="button"
    data-testid="preview-view-menu"
    aria-haspopup="menu"
    :aria-expanded="open"
    aria-label="View options"
    title="View options"
    class="flex h-8 w-8 shrink-0 items-center justify-center border border-transparent bg-transparent p-1.5 text-fg-muted"
    @pointerdown="onPointerDown"
    @click="open = !open"
  >
    <EditorIcon name="more" />
  </button>
  <MenuPanel
    v-if="open && trigger"
    testid="preview-view-panel"
    :heading="VIEW_MENU_HEADING"
    :subtitle="VIEW_MENU_SUBTITLE"
    :items="items"
    :anchor="trigger"
    @close="open = false"
  />
</template>
