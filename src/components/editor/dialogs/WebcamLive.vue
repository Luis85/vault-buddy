<script setup lang="ts">
/**
 * The webcam dialog's camera view (Task 50; SCREENS 05; visual-parity
 * Task 22, concept spec §9.7): a 16:9 view — "Camera is off" until the
 * camera is enabled, then the live preview with the 3-2-1 countdown and the
 * recording chip over it — and under it the status row (a dot and "Camera
 * off" / "Camera on" / "Recording", read from `webcamStatus`) and its help
 * line. Presentational: nothing here touches a device; the parent hands in
 * the recorder's stream.
 *
 * The concept's status-row right side names a "720p target"; the native
 * recorder asks the camera for its own default, so it says where the take
 * goes instead.
 */
import { computed, nextTick, ref, watch } from "vue";

import { webcamStatus } from "../../../editor/webcamPhase";
import type { WebcamView } from "../../../editor/webcamRecorder";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{ view: WebcamView; stream: MediaStream | null }>();

const liveVideo = ref<HTMLVideoElement | null>(null);
const live = computed(() => ["ready", "countdown", "recording"].includes(props.view.state));
const recording = computed(() => props.view.state === "recording");
const status = computed(() => webcamStatus(props.view.state));

const DOTS = { off: "bg-fg-muted", live: "bg-audio", recording: "bg-webcam-recording" } as const;
const dot = computed(() => DOTS[status.value.tone]);

/** Show the live stream once its element exists. */
watch([() => props.stream, live, liveVideo], async () => {
  await nextTick();
  if (liveVideo.value) liveVideo.value.srcObject = live.value ? props.stream : null;
});
</script>

<template>
  <div class="flex min-w-0 flex-col">
    <div
      data-testid="webcam-view"
      class="relative aspect-video overflow-hidden rounded-[10px] border border-line bg-webcam-view"
    >
      <video
        v-if="live"
        ref="liveVideo"
        data-testid="webcam-live"
        aria-label="Camera preview"
        autoplay
        muted
        playsinline
        class="absolute inset-0 h-full w-full object-cover"
      />
      <div
        v-else
        data-testid="webcam-empty"
        class="absolute inset-0 flex flex-col items-center justify-center gap-3.5 bg-[radial-gradient(ellipse_at_50%_60%,var(--color-webcam-glow),var(--color-webcam-view)_70%)] text-center"
      >
        <EditorIcon
          name="webcam"
          :size="38"
          class="text-webcam-icon"
        />
        <b class="text-[15px] font-semibold text-webcam-ink">Camera is off</b>
        <p class="text-[11px] leading-[1.7] text-webcam-sub">
          Enable your camera when you are ready.<br>Nothing is accessed until you do.
        </p>
      </div>
      <p
        v-if="view.count !== null"
        data-testid="webcam-countdown"
        aria-live="assertive"
        class="absolute inset-0 flex items-center justify-center bg-webcam-scrim text-[80px] font-bold text-white"
      >
        {{ view.count }}
      </p>
      <p
        v-if="recording"
        role="status"
        class="absolute right-3 top-3 rounded-[5px] bg-webcam-chip px-2.5 py-1.5 text-xs text-white"
      >
        Recording
      </p>
    </div>
    <div
      data-testid="webcam-status"
      class="flex justify-between gap-3 py-[11px] text-[11px] text-fg-muted"
    >
      <span class="flex items-center gap-2">
        <span
          class="h-1.5 w-1.5 shrink-0 rounded-full"
          :class="dot"
        />
        {{ status.label }}
      </span>
      <span>Local recording · saved into this project</span>
    </div>
    <p class="text-[11px] leading-[1.6] text-fg-muted">
      {{ status.message }}
    </p>
  </div>
</template>
