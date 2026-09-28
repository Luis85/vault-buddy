<script setup lang="ts">
/**
 * The webcam dialog's "Mic level preview" (visual-parity Task 22 fix
 * round 1; concept spec §9.7's 5 px teal `.webcam-meter`): the live
 * microphone's sample peak (`MicLevel`), polled like the mixer's meter
 * (`usePolledValue`). A new stream replaces the meter, no stream (the
 * camera off, the microphone off, the dialog closed) disposes it, and so
 * does unmounting — the measurement goes off with the camera light.
 */
import { onBeforeUnmount, watch } from "vue";

import { usePolledValue } from "../../../composables/usePolledValue";
import { MicLevel } from "../../../editor/micLevel";

const props = defineProps<{ stream: MediaStream | null }>();

const POLL_MS = 100;

let level: MicLevel | null = null;

function release(): void {
  level?.dispose();
  level = null;
}

watch(
  () => props.stream,
  (stream) => {
    release();
    level = MicLevel.create(stream);
  },
  { immediate: true },
);
onBeforeUnmount(release);

const peak = usePolledValue(() => level?.read() ?? 0, POLL_MS);
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <div
      data-testid="webcam-mic-meter"
      role="meter"
      aria-label="Microphone level (peak)"
      aria-valuemin="0"
      aria-valuemax="1"
      :aria-valuenow="peak"
      class="h-[5px] overflow-hidden rounded bg-line"
    >
      <div
        class="h-full bg-audio"
        :style="{ width: `${peak * 100}%` }"
      />
    </div>
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      Mic level preview (peak). Live audio is never played through your speakers.
    </p>
  </div>
</template>
