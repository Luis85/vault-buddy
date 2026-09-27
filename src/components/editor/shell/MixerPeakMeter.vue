<script setup lang="ts">
/**
 * The mixer's preview meter (Task 27, F-25): the preview controller's
 * SAMPLE PEAK of what it is playing (post-gain, post-monitoring), polled
 * every `POLL_MS` while this is mounted — the mixer mounts it only while
 * open. Labelled as a peak in dBFS, never as a loudness figure, which a
 * sample peak is not. `null` (no Web Audio, nothing playing yet) says so
 * rather than drawing an empty bar as if it had measured silence.
 */
import { computed } from "vue";

import { usePolledValue } from "../../../composables/usePolledValue";
import { formatDb } from "../../../editor/mixRules";

const props = defineProps<{ readPeak: () => number | null }>();

const POLL_MS = 100;

// The interval poll itself is `usePolledValue` (visual-parity Task 12): the
// transport's own peak bar (`TransportBar.vue`) samples this same
// `readPeak()` the same way, and a second hand-rolled mount/unmount timer
// here would be exactly the clone the quality ratchet forbids.
const peak = usePolledValue(() => props.readPeak(), POLL_MS);

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
