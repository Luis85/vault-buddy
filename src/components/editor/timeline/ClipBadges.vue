<script setup lang="ts">
/**
 * A clip's chips (visual-parity Task 18, concept spec §6.5): top-left, the
 * badges — a link glyph for a grouped clip, its speed ("2×") when it is
 * not 1, and STILL for an image — once the clip is wider than 60 px; and
 * top-right, a VIDEO clip's length ("9.5s") once it is wider than 140 px.
 * Narrower, neither is drawn at all, so a 3 px clip never shows a broken
 * chip. Both are pictures of the clip, not controls.
 *
 * `widthPx` and `durationMs` follow the live trim preview (`ClipItem`), so
 * the pill counts while a handle is dragged.
 */
import { computed } from "vue";

import type { Asset, Clip } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{
  clip: Clip;
  asset: Asset | null;
  kind: "video" | "audio";
  widthPx: number;
  durationMs: number;
}>();

/** Narrower than these, the chip is not drawn (the concept's own). */
const BADGES_MIN_PX = 60;
const PILL_MIN_PX = 140;

const speed = computed(() => props.clip.speed ?? 1);
const still = computed(() => props.asset?.media_type === "image");
const hasBadges = computed(
  () => props.widthPx > BADGES_MIN_PX && (Boolean(props.clip.group_id) || speed.value !== 1 || still.value),
);
const showPill = computed(() => props.kind === "video" && props.widthPx > PILL_MIN_PX);
</script>

<template>
  <span
    v-if="hasBadges"
    :data-testid="`clip-${clip.id}-badges`"
    class="absolute top-[3px] left-2 z-[3] flex max-w-[65%] items-center gap-[3px] rounded-[3px] bg-clip-badge px-1 py-px text-[8px] leading-3 text-clip-badge-ink"
  >
    <EditorIcon
      v-if="clip.group_id"
      name="link"
      :size="10"
    />
    <span v-if="speed !== 1">{{ speed }}×</span>
    <span v-if="still">STILL</span>
  </span>
  <span
    v-if="showPill"
    :data-testid="`clip-${clip.id}-duration`"
    class="absolute top-1 right-[7px] z-[3] rounded-[3px] bg-clip-pill px-1 py-px text-[8px] text-clip-pill-ink"
  >{{ (durationMs / 1000).toFixed(1) }}s</span>
</template>
