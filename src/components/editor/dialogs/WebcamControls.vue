<script setup lang="ts">
/**
 * The webcam dialog's footer (Task 50; visual-parity Task 22, concept spec
 * §9.7): one set of buttons per recorder state — Enable camera, Start
 * recording, Cancel countdown, Discard recording · Stop & review, Retake ·
 * Add to timeline, and Cancel request while the camera is being asked for
 * — read from `webcamFooter` (`editor/webcamPhase.ts`). Each button carries
 * its own reason in its `title`, and the footer's note says it on screen
 * (`FooterReason`, D14). `placeReason` holds Add to timeline back when the
 * chosen insert time is outside the project. Presentational — the parent drives the
 * recorder. Renders as footer siblings (no wrapper), so it lays out in
 * `DialogHost`'s own footer row.
 */
import { computed } from "vue";

import { type WebcamAction, webcamFooter } from "../../../editor/webcamPhase";
import type { WebcamState } from "../../../editor/webcamRecorder";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";

const props = defineProps<{ state: WebcamState; hasTake: boolean; placeReason: string | null }>();
const emit = defineEmits<{ (e: "act", action: WebcamAction): void }>();

const footer = computed(() => webcamFooter(props.state, props.hasTake, props.placeReason));
</script>

<template>
  <FooterReason
    data-testid="webcam-footer-reason"
    :text="footer.note"
  />
  <DialogButton
    v-for="button in footer.buttons"
    :key="button.action"
    :data-testid="`webcam-${button.action}`"
    :variant="button.variant"
    :icon="button.icon"
    :reason="button.reason"
    @click="emit('act', button.action)"
  >
    {{ button.label }}
  </DialogButton>
</template>
