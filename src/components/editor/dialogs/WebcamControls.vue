<script setup lang="ts">
/**
 * The webcam dialog's footer (Task 50; visual-parity Task 22, concept spec
 * §9.7): one set of buttons per recorder state — Enable camera, Start
 * recording, Cancel countdown, Discard recording · Stop & review, Retake ·
 * Add to timeline — read from `webcamFooter` (`editor/webcamPhase.ts`), and
 * the reason they cannot act right now on screen (`FooterReason`) as well
 * as in their `title` (D14). Presentational — the parent drives the
 * recorder. Renders as footer siblings (no wrapper), so it lays out in
 * `DialogHost`'s own footer row.
 */
import { computed } from "vue";

import { type WebcamAction, webcamFooter } from "../../../editor/webcamPhase";
import type { WebcamState } from "../../../editor/webcamRecorder";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";

const props = defineProps<{ state: WebcamState; hasTake: boolean }>();
const emit = defineEmits<{ (e: "act", action: WebcamAction): void }>();

const footer = computed(() => webcamFooter(props.state, props.hasTake));
</script>

<template>
  <FooterReason
    data-testid="webcam-footer-reason"
    :text="footer.reason"
  />
  <DialogButton
    v-for="button in footer.buttons"
    :key="button.action"
    :data-testid="`webcam-${button.action}`"
    :variant="button.variant"
    :icon="button.icon"
    :reason="footer.reason"
    @click="emit('act', button.action)"
  >
    {{ button.label }}
  </DialogButton>
</template>
