<script setup lang="ts">
/**
 * A clip's fade picture (visual-parity Task 18, concept spec §6.5
 * `.fade-shape`): over the whole body, in a 100 × 47 box stretched to it,
 * a faint gold triangle from the bottom-left corner up to the fade-in knee
 * and one down from the fade-out knee to the bottom-right corner, each
 * with its 1.2 px gold diagonal (non-scaling, so it stays thin however
 * wide the clip is). Only a nonzero fade draws — the knees' handles
 * (`ClipItem`) are the grab targets, even at zero.
 *
 * `fadeInPct` / `fadeOutPct` are the knees as percentages of the body:
 * `ClipItem` derives them once, from the live drag preview, for this and
 * for the handles alike.
 */
defineProps<{
  clipId: string;
  fadeInPct: number;
  fadeOutPct: number;
}>();
</script>

<template>
  <svg
    v-if="fadeInPct > 0 || fadeOutPct < 100"
    :data-testid="`clip-${clipId}-fade-shape`"
    class="absolute inset-0 z-[2] h-full w-full"
    viewBox="0 0 100 47"
    preserveAspectRatio="none"
    aria-hidden="true"
  >
    <g
      v-if="fadeInPct > 0"
      :data-testid="`clip-${clipId}-fade-in`"
    >
      <path
        :d="`M0 47L${fadeInPct} 2V47Z`"
        fill="var(--color-gold)"
        opacity=".12"
      />
      <path
        :d="`M0 47L${fadeInPct} 2`"
        stroke="var(--color-gold)"
        stroke-width="1.2"
        vector-effect="non-scaling-stroke"
        fill="none"
      />
    </g>
    <g
      v-if="fadeOutPct < 100"
      :data-testid="`clip-${clipId}-fade-out`"
    >
      <path
        :d="`M${fadeOutPct} 2L100 47H${fadeOutPct}Z`"
        fill="var(--color-gold)"
        opacity=".12"
      />
      <path
        :d="`M${fadeOutPct} 2L100 47`"
        stroke="var(--color-gold)"
        stroke-width="1.2"
        vector-effect="non-scaling-stroke"
        fill="none"
      />
    </g>
  </svg>
</template>
