<script setup lang="ts">
/**
 * The guide's highlight (Task 56; visual-parity Task 23; concept spec
 * §9.2): the ring around the lesson's REAL control and the label chip that
 * names it. Both are fixed-position, pointer-transparent and hidden from
 * assistive technology — the coach card says the same thing in words — and
 * both are mounted by `GuideCoach` outside the preview section, so no guide
 * pixel can reach a render.
 *
 * - **Ring**: 2px `guide-edge`, radius 8, with a faint 4px halo, drawn at
 *   the target's box grown by 4px on every side. Dimming (a preference, off
 *   while motion is reduced) is a wide shadow in `guide-dim` around it, so
 *   it never blocks a click. Forced colours draw it as a 3px `Highlight`
 *   border (`.vb-guide-ring`, `style.css`).
 * - **Label**: at the target's left, 30px above it (below near the top of
 *   the window), at most 250 wide; hidden under 700px wide and while a menu
 *   is open (`placeLabel`). A hidden label stays laid out (`invisible`),
 *   so its own width is known the moment it may show again.
 */
import { computed, nextTick, ref, watch } from "vue";

import type { Rect, Size } from "../../../editor/guide/position";
import { placeLabel } from "../../../editor/guide/position";

const props = defineProps<{
  rect: Rect;
  label: string;
  viewport: Size;
  menuOpen: boolean;
  dimmed: boolean;
  animated: boolean;
}>();

const px = (n: number) => `${Math.round(n)}px`;
const ringStyle = computed(() => ({
  left: px(props.rect.x - 4),
  top: px(props.rect.y - 4),
  width: px(props.rect.width + 8),
  height: px(props.rect.height + 8),
}));

const labelEl = ref<HTMLElement | null>(null);
const labelWidth = ref(0);
watch(
  () => props.label,
  () => void nextTick(() => (labelWidth.value = labelEl.value?.offsetWidth ?? 0)),
  { immediate: true },
);
const labelAt = computed(() => placeLabel(props.rect, labelWidth.value, props.viewport, props.menuOpen));
</script>

<template>
  <div
    data-guide-layer="ring"
    data-testid="guide-ring"
    aria-hidden="true"
    class="vb-guide-ring guide-ring pointer-events-none fixed z-[39] rounded-[8px] border-2 border-guide-edge"
    :class="{ 'guide-ring-dim': dimmed, 'guide-ring-animated': animated }"
    :style="ringStyle"
  />
  <span
    ref="labelEl"
    data-guide-layer="label"
    data-testid="guide-target-label"
    aria-hidden="true"
    class="pointer-events-none fixed z-[39] max-w-[250px] overflow-hidden rounded-[5px] border border-guide-edge bg-accent-bg px-[9px] py-1 text-[10px] font-[550] text-ellipsis whitespace-nowrap text-accent-ink"
    :class="{ invisible: labelAt === null }"
    :data-hidden="labelAt === null ? 'true' : undefined"
    :style="{ left: px(labelAt?.x ?? 0), top: px(labelAt?.y ?? 0) }"
  >{{ label }}</span>
</template>

<style scoped>
.guide-ring {
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--color-guide-edge) 15%, transparent);
}
.guide-ring-dim {
  box-shadow:
    0 0 0 4px color-mix(in srgb, var(--color-guide-edge) 15%, transparent),
    0 0 0 9999px var(--color-guide-dim);
}
.guide-ring-animated {
  transition: left 150ms ease, top 150ms ease, width 150ms ease, height 150ms ease;
}
</style>
