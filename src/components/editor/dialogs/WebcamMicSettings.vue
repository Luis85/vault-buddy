<script setup lang="ts">
/**
 * The webcam dialog's microphone choices (visual-parity Task 22 fix
 * round 1, Ruling T22-1; concept spec §9.7): "Include microphone", the
 * Microphone picker — the camera picker's own mechanism: the devices the
 * recorder lists once the camera is on, and a change while it is live asks
 * the parent to request the stream again (`reselect`) — and the mic level
 * preview (`WebcamMicMeter`). The picker waits for the checkbox and for the
 * camera, and says why on screen (D14). Nothing here touches a device.
 */
import { computed } from "vue";

import { micPickerReason } from "../../../editor/webcamPhase";
import type { WebcamView } from "../../../editor/webcamRecorder";
import WebcamMicMeter from "./WebcamMicMeter.vue";

const props = defineProps<{ view: WebcamView; stream: MediaStream | null }>();
const emit = defineEmits<{ (e: "reselect"): void }>();
const withMic = defineModel<boolean>("withMic", { required: true });
const micId = defineModel<string>("micId", { required: true });

const canToggleMic = computed(() => props.view.state === "idle" || props.view.state === "ready");
const reason = computed(() => micPickerReason(props.view.state, withMic.value));
</script>

<template>
  <label class="flex items-center gap-2 text-[11px] text-fg">
    <input
      v-model="withMic"
      data-testid="webcam-mic"
      type="checkbox"
      :disabled="!canToggleMic"
      @change="emit('reselect')"
    >
    Include microphone
  </label>
  <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
    Microphone
    <select
      v-model="micId"
      data-testid="webcam-mic-device"
      :disabled="reason !== ''"
      :title="reason || undefined"
      aria-describedby="webcam-mic-reason"
      class="text-xs"
      @change="emit('reselect')"
    >
      <option value="">
        System default
      </option>
      <option
        v-for="mic in view.microphones"
        :key="mic.deviceId"
        :value="mic.deviceId"
      >
        {{ mic.label }}
      </option>
    </select>
  </label>
  <p
    id="webcam-mic-reason"
    data-testid="webcam-mic-reason"
    role="status"
    aria-live="polite"
    class="-mt-1.5 min-h-[15px] text-[10px] text-fg-muted"
  >
    {{ reason }}
  </p>
  <WebcamMicMeter :stream="stream" />
</template>
