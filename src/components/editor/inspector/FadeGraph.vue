<script setup lang="ts">
/**
 * The Fades tab's envelope (visual-parity Task 15; concept spec §5 "Fades",
 * `.fade-graph`): a 58px well on the app background drawing the clip's
 * fade in and fade out in accent (`fadeGraph.ts`), with faint guides at
 * full level and silence, a 10% fill under the envelope and a dot at each
 * knee. It follows the committed values; it is a picture, not a control.
 */
import { computed } from "vue";

import { fadeGraphPaths } from "../../../editor/fadeGraph";

const props = defineProps<{ fadeInMs: number; fadeOutMs: number; durationMs: number }>();

const graph = computed(() => fadeGraphPaths(props.fadeInMs, props.fadeOutMs, props.durationMs));
</script>

<template>
  <div class="h-[58px] rounded-[7px] border border-line bg-app text-accent">
    <svg
      data-testid="fades-section-graph"
      role="img"
      aria-label="Fade envelope"
      viewBox="0 0 240 70"
      class="block h-full w-full"
    >
      <path
        d="M10 55H230M10 15H230"
        stroke="currentColor"
        opacity=".14"
      />
      <path
        :d="graph.envelope"
        fill="none"
        stroke="currentColor"
        stroke-width="2.5"
      />
      <path
        :d="graph.fill"
        fill="currentColor"
        opacity=".1"
      />
      <circle
        v-for="(x, i) in graph.knees"
        :key="i"
        :cx="x"
        cy="15"
        r="3"
        fill="currentColor"
      />
    </svg>
  </div>
</template>
