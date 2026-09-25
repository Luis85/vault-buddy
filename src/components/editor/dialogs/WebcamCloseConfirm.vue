<script setup lang="ts">
/**
 * The webcam dialog's close question (Task 50; SCREENS 05: "Closing …
 * handles unsaved take choices"). Two situations, each worded for what can
 * actually happen:
 *
 * - still RECORDING: closing discards the recording (its `.part` really is
 *   removed) — Keep recording, or Discard recording and close;
 * - a FINISHED take not yet on the timeline: nothing can delete it (a
 *   registered asset, GAP-195), so the choice is Back to the take, or Keep
 *   in library and close — never a "Discard" that would not discard.
 */
import { computed } from "vue";

import AppButton from "../../ui/AppButton.vue";

const props = defineProps<{ recording: boolean }>();
const emit = defineEmits<{ (e: "back"): void; (e: "confirm"): void }>();

const copy = computed(() =>
  props.recording
    ? {
        text: "You are still recording. Closing discards this recording.",
        back: "Keep recording",
        confirm: "Discard recording and close",
        variant: "danger" as const,
      }
    : {
        text: "This take is not on the timeline yet. It stays in the media library (a recorded take is never deleted), so you can add it later.",
        back: "Back to the take",
        confirm: "Keep in library and close",
        variant: "primary" as const,
      },
);
</script>

<template>
  <div
    data-testid="webcam-confirm"
    role="alertdialog"
    aria-label="Close the webcam dialog?"
    class="flex flex-col gap-2 rounded border border-line p-2"
  >
    <p>{{ copy.text }}</p>
    <div class="flex justify-end gap-2">
      <AppButton
        variant="secondary"
        size="sm"
        data-testid="webcam-confirm-back"
        @click="emit('back')"
      >
        {{ copy.back }}
      </AppButton>
      <AppButton
        :variant="copy.variant"
        size="sm"
        data-testid="webcam-confirm-keep"
        @click="emit('confirm')"
      >
        {{ copy.confirm }}
      </AppButton>
    </div>
  </div>
</template>
