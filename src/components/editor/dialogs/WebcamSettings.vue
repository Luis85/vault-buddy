<script setup lang="ts">
/**
 * The webcam dialog's "Set up your take" column (visual-parity Task 22;
 * concept spec §9.7's `.webcam-settings`): the camera, whether the
 * microphone is recorded too, and where the take will land. Presentational
 * — the choices are `v-model`s, and a change once the camera is live asks
 * the parent to request it again (`reselect`); nothing here touches a
 * device.
 *
 * Only what the native recorder can do is offered (design D14): the
 * concept's microphone picker, mic level meter, "Insert at timeline time"
 * field and mirror toggle have no native backend and are omitted. Where
 * the take lands is said instead — Add to timeline places it at the
 * playhead, on a new track above the others (`placeTake`).
 *
 * The camera list is readable only once the camera is on (the platform
 * hides device labels before permission), so the choice waits for it and
 * says so on screen, not only in a tooltip.
 */
import { computed } from "vue";

import { formatOutputTime } from "../../../editor/captionRules";
import { webcamSettingsReason } from "../../../editor/webcamPhase";
import type { WebcamView } from "../../../editor/webcamRecorder";

const props = defineProps<{ view: WebcamView; playheadMs: number }>();
const emit = defineEmits<{ (e: "reselect"): void }>();
const cameraId = defineModel<string>("cameraId", { required: true });
const withMic = defineModel<boolean>("withMic", { required: true });

/** A camera can be switched only while it is live and idle. */
const canSwitch = computed(() => props.view.state === "ready");
const canToggleMic = computed(() => props.view.state === "idle" || canSwitch.value);
const reason = computed(() => webcamSettingsReason(props.view.state));
const at = computed(() => formatOutputTime(props.playheadMs));
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
    <label class="flex items-center gap-2 text-[11px] text-fg">
      <input
        v-model="withMic"
        data-testid="webcam-mic"
        type="checkbox"
        :disabled="!canToggleMic"
        aria-describedby="webcam-device-reason"
        @change="emit('reselect')"
      >
      Include microphone
    </label>
    <p
      data-testid="webcam-placement"
      class="text-[10px] leading-[1.6] text-fg-muted"
    >
      Added at the playhead ({{ at }}), on a new track above your other video tracks.
    </p>
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      Records a webcam take alongside your existing edit — not another screen capture. Change its size, position,
      crop and fades afterward.
    </p>
  </div>
</template>
