<script setup lang="ts">
/**
 * The webcam dialog's "Set up your take" column (visual-parity Task 22;
 * concept spec §9.7's `.webcam-settings`). Presentational — every choice
 * is a `v-model`, and a camera or microphone change once the camera is
 * live asks the parent to request it again (`reselect`); nothing here
 * touches a device.
 *
 * - **Camera**: the devices the recorder lists once the camera is on (the
 *   platform hides device labels before permission), so the choice waits
 *   for it and says so on screen, not only in a tooltip.
 * - **Include microphone**, the **Microphone** picker and the **mic level
 *   preview** (`WebcamMicSettings`, fix round 1, Ruling T22-1).
 * - **Insert at timeline time (seconds)**: where Add to timeline places
 *   the take (`placePresenterTake`'s `atMs`), the playhead when the dialog
 *   opened unless changed; a time outside the project holds Add to
 *   timeline back with its reason (`insertTimeMs`).
 * - **Mirror preview & overlay**: mirrors the live preview only (the
 *   recording is never mirrored), and Add to timeline sets the placed
 *   clip's own `mirror` flag in the same `setLayout` — the flag the
 *   inspector's Flip toggles afterward.
 */
import { computed } from "vue";

import { formatOutputTime } from "../../../editor/captionRules";
import { insertTimeMs, webcamSettingsReason } from "../../../editor/webcamPhase";
import type { WebcamView } from "../../../editor/webcamRecorder";
import WebcamMicSettings from "./WebcamMicSettings.vue";

const props = defineProps<{ view: WebcamView; stream: MediaStream | null; durationMs: number }>();
const emit = defineEmits<{ (e: "reselect"): void }>();
const cameraId = defineModel<string>("cameraId", { required: true });
const withMic = defineModel<boolean>("withMic", { required: true });
const micId = defineModel<string>("micId", { required: true });
const insertAt = defineModel<string | number>("insertAt", { required: true });
const mirror = defineModel<boolean>("mirror", { required: true });

/** A camera can be switched only while it is live and idle. */
const canSwitch = computed(() => props.view.state === "ready");
const reason = computed(() => webcamSettingsReason(props.view.state));
const placement = computed(() => {
  const at = insertTimeMs(insertAt.value, props.durationMs);
  return "ms" in at
    ? `Added at ${formatOutputTime(at.ms)}, on a new track above your other video tracks.`
    : at.reason;
});
</script>

<template>
  <div
    data-testid="webcam-settings"
    class="flex min-w-0 flex-col gap-3"
  >
    <h3 class="text-xs font-semibold text-fg">
      Set up your take
    </h3>
    <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
      Camera
      <select
        v-model="cameraId"
        data-testid="webcam-device"
        :disabled="!canSwitch"
        :title="reason || undefined"
        aria-describedby="webcam-device-reason"
        class="text-xs"
        @change="emit('reselect')"
      >
        <option value="">
          System default
        </option>
        <option
          v-for="camera in view.cameras"
          :key="camera.deviceId"
          :value="camera.deviceId"
        >
          {{ camera.label }}
        </option>
      </select>
    </label>
    <p
      id="webcam-device-reason"
      data-testid="webcam-device-reason"
      role="status"
      aria-live="polite"
      class="-mt-1.5 min-h-[15px] text-[10px] text-fg-muted"
    >
      {{ reason }}
    </p>
    <WebcamMicSettings
      v-model:with-mic="withMic"
      v-model:mic-id="micId"
      :view="view"
      :stream="stream"
      @reselect="emit('reselect')"
    />
    <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
      Insert at timeline time (seconds)
      <input
        v-model="insertAt"
        data-testid="webcam-insert-at"
        type="number"
        min="0"
        step="0.01"
        aria-describedby="webcam-placement"
        class="text-xs"
      >
    </label>
    <p
      id="webcam-placement"
      data-testid="webcam-placement"
      class="-mt-1.5 text-[10px] leading-[1.6] text-fg-muted"
    >
      {{ placement }}
    </p>
    <label class="flex items-center gap-2 text-[11px] text-fg">
      <input
        v-model="mirror"
        data-testid="webcam-mirror"
        type="checkbox"
      >
      Mirror preview & overlay
    </label>
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      Records a webcam take alongside your existing edit — not another screen capture. The recording itself is never
      mirrored. Change its size, position, crop and fades afterward.
    </p>
  </div>
</template>
