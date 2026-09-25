<script setup lang="ts">
/**
 * The coach's **Start over** (Task 56; ONBOARDING.md: "Start over requires
 * confirmation and resets guide state only"): a two-step, in-card confirm —
 * the retired `ExportBar`'s precedent, never a native dialog (that would
 * suspend the very coach it sits in). Confirming emits `restart`; the store
 * resets the guide's progress and nothing else. A lesson change disarms it
 * (`key` on the step id, from the card).
 */
import { ref } from "vue";

import AppButton from "../../ui/AppButton.vue";

const emit = defineEmits<{ (e: "restart"): void }>();
const confirming = ref(false);

function confirm(): void {
  confirming.value = false;
  emit("restart");
}
</script>

<template>
  <button
    v-if="!confirming"
    type="button"
    data-testid="guide-start-over"
    class="cursor-pointer rounded px-1 text-fg-subtle underline hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    @click="confirming = true"
  >
    Start over
  </button>
  <span
    v-else
    class="flex flex-wrap items-center gap-1"
  >
    <span class="text-fg-muted">Start the walkthrough over? Your project is not changed.</span>
    <AppButton
      size="sm"
      variant="danger"
      data-testid="guide-start-over-confirm"
      @click="confirm"
    >Start over</AppButton>
    <AppButton
      size="sm"
      variant="ghost"
      data-testid="guide-start-over-cancel"
      @click="confirming = false"
    >Keep going</AppButton>
  </span>
</template>
