<script setup lang="ts">
/**
 * One of the preview header's two panel toggles (visual-parity Task 11;
 * concept spec §4.1, §11): an icon button whose `aria-pressed` is the
 * panel's real visibility and whose title says what a press does ("Hide
 * media library" / "Show properties"). Muted while the panel is hidden,
 * the secondary ink while it shows.
 */
import { computed } from "vue";

import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{ pressed: boolean; title: string; icon: EditorIconName; narrow?: boolean }>();

const buttonClass = computed(() => [
  props.narrow ? "w-[30px]" : "w-8",
  props.pressed ? "text-fg-secondary" : "text-fg-muted",
]);
</script>

<template>
  <button
    type="button"
    :aria-pressed="pressed"
    :title="title"
    :aria-label="title"
    class="flex h-8 shrink-0 items-center justify-center border border-transparent bg-transparent p-1.5"
    :class="buttonClass"
  >
    <EditorIcon :name="icon" />
  </button>
</template>
