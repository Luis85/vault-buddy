<script setup lang="ts">
/**
 * The Layout tab's four corner presets (visual-parity Task 14; concept spec
 * §5, `editor.css: .corner-presets`): a 2×2 grid of bordered buttons, each
 * with the concept's corner glyph — a small frame with an accent dot in the
 * corner it places the picture in.
 */
import type { Corner } from "../../../editor/layoutGeometry";

const emit = defineEmits<(e: "corner", corner: Corner) => void>();

const CORNERS: { id: Corner; label: string; dot: string }[] = [
  { id: "tl", label: "Top left", dot: "left-0.5 top-0.5" },
  { id: "tr", label: "Top right", dot: "right-0.5 top-0.5" },
  { id: "bl", label: "Bottom left", dot: "bottom-0.5 left-0.5" },
  { id: "br", label: "Bottom right", dot: "bottom-0.5 right-0.5" },
];
</script>

<template>
  <div
    class="mt-0.5 grid grid-cols-2 gap-1.5"
    role="group"
    aria-label="Place overlay in a corner"
  >
    <button
      v-for="corner in CORNERS"
      :key="corner.id"
      type="button"
      :data-testid="`layout-corner-${corner.id}`"
      :title="corner.label"
      :aria-label="`Place ${corner.label.toLowerCase()}`"
      class="flex min-h-[34px] items-center justify-start gap-[7px] border border-line px-2 py-[5px] text-[10px] text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="emit('corner', corner.id)"
    >
      <i
        data-testid="corner-icon"
        aria-hidden="true"
        class="relative inline-block h-[15px] w-[21px] shrink-0 rounded-[2px] border border-fg-muted"
      >
        <span
          class="absolute h-[5px] w-[6px] rounded-[1px] bg-accent"
          :class="corner.dot"
        />
      </i>
      <span class="truncate">{{ corner.label }}</span>
    </button>
  </div>
</template>
