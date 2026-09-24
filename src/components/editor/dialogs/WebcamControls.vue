<script setup lang="ts">
/**
 * The webcam dialog's recording buttons (Task 50; SCREENS 05: enable,
 * countdown, record/stop and cancel are distinct states), one set per
 * recorder state. Presentational — the parent drives the recorder.
 */
import { computed } from "vue";

import type { WebcamState } from "../../../editor/webcamRecorder";
import AppButton from "../../ui/AppButton.vue";

const props = defineProps<{ state: WebcamState }>();
const emit = defineEmits<{
  (e: "enable"): void;
  (e: "record"): void;
  (e: "cancel"): void;
  (e: "stop"): void;
}>();

const idle = computed(() => props.state === "idle" || props.state === "requesting");
const recording = computed(() => props.state === "recording");
const cancellable = computed(() => props.state === "countdown" || recording.value);
const cancelLabel = computed(() => (recording.value ? "Discard recording" : "Cancel"));
</script>

<template>
  <div class="flex justify-end gap-2">
    <AppButton
      v-if="idle"
      size="sm"
      data-testid="webcam-enable"
      :disabled="state === 'requesting'"
      @click="emit('enable')"
    >
      Enable camera
    </AppButton>
    <AppButton
      v-if="state === 'ready'"
      size="sm"
      data-testid="webcam-record"
      @click="emit('record')"
    >
      Record
    </AppButton>
    <AppButton
      v-if="cancellable"
      variant="secondary"
      size="sm"
      data-testid="webcam-cancel"
      @click="emit('cancel')"
    >
      {{ cancelLabel }}
    </AppButton>
    <AppButton
      v-if="recording"
      size="sm"
      data-testid="webcam-stop"
      @click="emit('stop')"
    >
      Stop
    </AppButton>
  </div>
</template>
