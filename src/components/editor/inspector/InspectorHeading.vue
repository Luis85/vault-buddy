<script setup lang="ts">
/**
 * The inspector's 48px heading (visual-parity Task 13; concept spec §5
 * `.inspector-heading`): what is selected ("Clip properties", "Track
 * properties"…) and the ✕ that hides the panel — the same D5 toggle the
 * preview header's properties button drives (`panelLayout.ts`), so at a
 * drawer width it closes the drawer.
 */
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ title: string }>();

const workspace = useEditorWorkspaceStore();
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
      @click="workspace.toggleInspector()"
    >
      <EditorIcon name="x" />
    </button>
  </div>
</template>
