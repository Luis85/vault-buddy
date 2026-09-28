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
 *
 * Visual-parity Task 22 (concept spec §9.7's close guard during review):
 * it sits in the dialog's footer — the question left, its two answers
 * right — and takes focus when it appears, on its safe answer (back), so
 * the keyboard is where the question is.
 */
import { computed, nextTick, onMounted, ref } from "vue";

import DialogButton from "./DialogButton.vue";

const props = defineProps<{ recording: boolean }>();
const emit = defineEmits<{ (e: "back"): void; (e: "confirm"): void }>();

const back = ref<InstanceType<typeof DialogButton> | null>(null);
onMounted(async () => {
  await nextTick();
  (back.value?.$el as HTMLElement | undefined)?.focus();
});

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
    class="flex min-w-0 flex-1 items-center justify-end gap-2"
  >
    <p class="mr-auto min-w-0 text-[11px] leading-[1.5] text-fg-secondary">
      {{ copy.text }}
    </p>
    <DialogButton
      ref="back"
      data-testid="webcam-confirm-back"
      @click="emit('back')"
    >
      {{ copy.back }}
    </DialogButton>
    <DialogButton
      :variant="copy.variant"
      data-testid="webcam-confirm-keep"
      @click="emit('confirm')"
    >
      {{ copy.confirm }}
    </DialogButton>
  </div>
</template>
