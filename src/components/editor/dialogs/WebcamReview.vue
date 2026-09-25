<script setup lang="ts">
/**
 * The webcam dialog's review half (Task 50; SCREENS 05): "Finishing the
 * take…" while Rust remuxes it, then the FINISHED file played through
 * `editor_media_url` (`ProductPlayer` — the real file, not the live
 * camera), Retake and Add to timeline. Retake says plainly that the take
 * stays in the media library: a finished take is a registered asset and is
 * never deleted (GAP-195). Presentational — the parent acts.
 */
import { computed } from "vue";

import type { WebcamView } from "../../../editor/webcamRecorder";
import AppButton from "../../ui/AppButton.vue";
import ProductPlayer from "../preview/ProductPlayer.vue";

const props = defineProps<{ view: WebcamView; placeError: string | null }>();
const emit = defineEmits<{ (e: "retake"): void; (e: "add"): void }>();

const committing = computed(() => props.view.state === "committing");
</script>

<template>
  <div class="flex flex-col gap-2">
    <p
      v-if="!view.take"
      role="status"
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
        class="text-danger-fg"
      >
        The take could not be added to the timeline. {{ placeError }}
      </p>
      <div class="flex justify-end gap-2">
        <AppButton
          variant="secondary"
          size="sm"
          data-testid="webcam-retake"
          :disabled="committing"
          @click="emit('retake')"
        >
          Retake
        </AppButton>
        <AppButton
          size="sm"
          data-testid="webcam-add"
          :disabled="committing"
          @click="emit('add')"
        >
          Add to timeline
        </AppButton>
      </div>
    </template>
  </div>
</template>
