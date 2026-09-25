<script setup lang="ts">
/**
 * The webcam dialog's camera half (Task 50; SCREENS 05): the live preview
 * with the 3-2-1 countdown and the recording badge over it, and the camera/
 * microphone choices. Presentational — the choices are `v-model`s, and a
 * change once the camera is live asks the parent to request it again
 * (`reselect`); nothing here touches a device.
 */
import { computed, nextTick, ref, watch } from "vue";

import type { WebcamView } from "../../../editor/webcamRecorder";

const props = defineProps<{ view: WebcamView; stream: MediaStream | null }>();
const emit = defineEmits<{ (e: "reselect"): void }>();
const cameraId = defineModel<string>("cameraId", { required: true });
const withMic = defineModel<boolean>("withMic", { required: true });

const liveVideo = ref<HTMLVideoElement | null>(null);
const live = computed(() => ["ready", "countdown", "recording"].includes(props.view.state));
const recording = computed(() => props.view.state === "recording");
/** A camera can be switched only while it is live and idle. */
const canSwitch = computed(() => props.view.state === "ready");
const canToggleMic = computed(() => props.view.state === "idle" || canSwitch.value);

/** Show the live stream once its element exists. */
watch([() => props.stream, live, liveVideo], async () => {
  await nextTick();
  if (liveVideo.value) liveVideo.value.srcObject = live.value ? props.stream : null;
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <div
      v-if="live"
      class="relative"
    >
      <video
        ref="liveVideo"
        data-testid="webcam-live"
        aria-label="Camera preview"
        autoplay
        muted
        playsinline
        class="max-h-[40vh] w-full rounded-control bg-black"
      />
      <p
        v-if="view.count !== null"
        data-testid="webcam-countdown"
        aria-live="assertive"
        class="absolute inset-0 flex items-center justify-center text-5xl font-semibold text-white"
      >
        {{ view.count }}
      </p>
      <p
        v-if="recording"
        role="status"
        class="absolute left-2 top-2 rounded bg-black/60 px-1 text-danger-fg"
      >
        Recording
      </p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <label
        v-if="view.cameras.length > 0"
        class="flex items-center gap-1"
      >
        Camera
        <select
          v-model="cameraId"
          data-testid="webcam-device"
          :disabled="!canSwitch"
          class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
          @change="emit('reselect')"
        >
          <option value="">
            Default camera
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
      <label class="flex items-center gap-1">
        <input
          v-model="withMic"
          data-testid="webcam-mic"
          type="checkbox"
          :disabled="!canToggleMic"
          class="accent-violet-500"
          @change="emit('reselect')"
        >
        Record microphone too
      </label>
    </div>
  </div>
</template>
