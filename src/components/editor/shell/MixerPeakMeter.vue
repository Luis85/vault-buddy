<script setup lang="ts">
/**
 * The mixer's preview meter (Task 27, F-25): the preview controller's
 * SAMPLE PEAK of what it is playing (post-gain, post-monitoring), polled
 * every `POLL_MS` while this is mounted — the mixer mounts it only while
 * open. Labelled as a peak in dBFS, never as a loudness figure, which a
 * sample peak is not. `null` (no Web Audio, nothing playing yet) says so
 * rather than drawing an empty bar as if it had measured silence.
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { formatDb } from "../../../editor/mixRules";

const props = defineProps<{ readPeak: () => number | null }>();

const POLL_MS = 100;

const peak = ref<number | null>(null);
let timer: ReturnType<typeof setInterval> | null = null;
function sample(): void {
  peak.value = props.readPeak();
}
onMounted(() => {
  sample();
  timer = setInterval(sample, POLL_MS);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

const text = computed(() => (peak.value === null ? "No preview audio" : `${formatDb(peak.value)}FS`));
const fill = computed(() => `${Math.min(1, peak.value ?? 0) * 100}%`);
</script>

<template>
  <div
    data-testid="mixer-peak"
    class="flex flex-col gap-0.5"
  >
    <span class="flex justify-between">
      Preview peak
      <span class="tabular-nums">{{ text }}</span>
    </span>
    <div
      role="meter"
      aria-label="Preview peak"
      aria-valuemin="0"
      aria-valuemax="1"
      :aria-valuenow="peak ?? 0"
      class="h-1.5 overflow-hidden rounded bg-stage"
    >
      <div
        class="h-full bg-accent"
        :style="{ width: fill }"
      />
    </div>
  </div>
</template>
