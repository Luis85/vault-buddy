<script setup lang="ts">
/**
 * The library's full-width bordered button (concept spec §3.4–3.6: Add
 * caption, Import SRT / VTT, Export timeline SRT, Add chapter at playhead,
 * Render a new video — the concept's `.btn`, left-aligned, 11px, an icon
 * first). Everything else (`data-testid`, `disabled`, `title`, the click)
 * falls through to the `<button>`, so a disabled one carries its reason
 * the way every other control does.
 */
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ icon: EditorIconName }>();
</script>

<template>
  <button
    type="button"
    class="flex w-full items-center gap-[7px] rounded-[7px] border border-line bg-panel px-2.5 py-[7px] text-left text-[11px] leading-4 text-fg hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40"
  >
    <EditorIcon
      :name="icon"
      :size="16"
      class="shrink-0"
    />
    <span class="min-w-0 truncate"><slot /></span>
  </button>
</template>
