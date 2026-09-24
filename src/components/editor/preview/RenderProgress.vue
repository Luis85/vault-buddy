<script setup lang="ts">
/**
 * A render's progress (Task 47; F-41): its phase in words and a bar. The
 * Render dialog and the Review dialog share it so both obey the one rule
 * `renderPercent` states — 100 % only from a `complete` terminal, never
 * from `fraction: 1` (R20).
 */
import { computed } from "vue";

import type { RenderProgressView } from "../../../editor/renderProgress";
import { PHASE_LABELS, renderPercent } from "../../../editor/renderProgress";

const props = defineProps<{ job: RenderProgressView }>();

const percent = computed(() => renderPercent(props.job));
</script>

<template>
  <div class="flex flex-col gap-1">
    <div class="flex justify-between text-xs text-fg-secondary">
      <span data-testid="render-progress-phase">{{ PHASE_LABELS[job.phase] }}</span>
      <span>{{ percent }} %</span>
    </div>
    <div
      role="progressbar"
      aria-label="Render progress"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="percent"
      data-testid="render-progress-bar"
      class="h-2 overflow-hidden rounded-full bg-white/10"
    >
      <div
        class="h-full bg-accent transition-[width]"
        :style="{ width: `${percent}%` }"
      />
    </div>
  </div>
</template>
