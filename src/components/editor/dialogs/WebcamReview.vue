<script setup lang="ts">
/**
 * The webcam dialog's review view (Task 50; SCREENS 05): "Finishing the
 * take…" while Rust remuxes it, then the FINISHED file played through
 * `editor_media_url` (`ProductPlayer` — the real file, not the live
 * camera). Retake says plainly that the take stays in the media library: a
 * finished take is a registered asset and is never deleted (GAP-195).
 * Presentational — Retake and Add to timeline are the footer's
 * (`WebcamControls`, visual-parity Task 22), and a refused Add to timeline
 * is said here, where the take is.
 */
import type { WebcamView } from "../../../editor/webcamRecorder";
import ProductPlayer from "../preview/ProductPlayer.vue";

defineProps<{ view: WebcamView; placeError: string | null }>();
</script>

<template>
  <div class="flex min-w-0 flex-col gap-2 text-[11px]">
    <p
      v-if="!view.take"
      role="status"
      class="flex aspect-video items-center justify-center rounded-[10px] border border-line bg-webcam-view text-webcam-sub"
    >
      Finishing the take…
    </p>
    <template v-else>
      <ProductPlayer
        :media="{ assetId: view.take.assetId }"
        label="Webcam take"
        subject="webcam take"
      />
      <p class="text-fg-muted">
        Retake keeps this take in the media library; recording again never deletes it.
      </p>
      <p
        v-if="placeError !== null"
        role="alert"
        data-testid="webcam-place-error"
        class="text-danger-fg"
      >
        The take could not be added to the timeline. {{ placeError }}
      </p>
    </template>
  </div>
</template>
