<script setup lang="ts">
/**
 * One before-you-share finding in the Checks dialog (Task 54; SCREENS 07):
 * its severity tag, its sentence, and the one button that reveals what it
 * is about (`checkReveal.CHECK_ACTION_LABELS`). Presentational — the dialog owns
 * what the button does. A finding with no action (an empty timeline) has
 * no button rather than a disabled one that goes nowhere.
 */
import { CHECK_ACTION_LABELS } from "../../../editor/checkReveal";
import type { CheckFinding } from "../../../editorTypes";

defineProps<{ finding: CheckFinding; tag: string }>();
const emit = defineEmits<{ (e: "act"): void }>();
</script>

<template>
  <li
    :data-testid="`check-${finding.id}`"
    class="flex items-start gap-2 border-b border-line py-2 last:border-b-0"
  >
    <span class="mt-0.5 shrink-0 rounded bg-accent/15 px-1 font-mono text-micro text-accent-fg">{{ tag }}</span>
    <div class="flex min-w-0 flex-1 flex-col items-start gap-1">
      <p class="text-xs text-fg">
        {{ finding.message }}
      </p>
      <button
        v-if="finding.action"
        type="button"
        :data-testid="`check-action-${finding.id}`"
        class="cursor-pointer rounded-control border border-line px-2 py-0.5 text-micro text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="emit('act')"
      >
        {{ CHECK_ACTION_LABELS[finding.action] }} ›
      </button>
    </div>
  </li>
</template>
