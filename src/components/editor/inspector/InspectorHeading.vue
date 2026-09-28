<script setup lang="ts">
/**
 * The inspector's 48px heading (visual-parity Task 13; concept spec §5
 * `.inspector-heading`): what is selected ("Clip properties", "Track
 * properties"…) and the ✕ that hides the panel — the same D5 toggle the
 * preview header's properties button drives (`panelLayout.ts`), so at a
 * drawer width it closes the drawer. The ✕ hides its own panel, so focus
 * goes to that properties button — the control that brings the panel back
 * — rather than falling to the page (fix round 1).
 */
import { nextTick } from "vue";

import { PROPERTIES_TOGGLE_ID } from "../../../editor/previewHeader";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ title: string }>();

const workspace = useEditorWorkspaceStore();

async function hide(): Promise<void> {
  workspace.toggleInspector();
  await nextTick();
  document.getElementById(PROPERTIES_TOGGLE_ID)?.focus();
}
</script>

<template>
  <div
    data-testid="inspector-heading"
    class="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-line px-[15px] text-[10px] font-medium text-fg-muted"
  >
    <h2
      data-testid="inspector-title"
      class="min-w-0 truncate"
    >
      {{ title }}
    </h2>
    <button
      type="button"
      data-testid="inspector-hide"
      aria-label="Hide properties"
      title="Hide properties"
      class="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-md text-fg-muted hover:bg-hover hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="hide"
    >
      <EditorIcon name="x" />
    </button>
  </div>
</template>
