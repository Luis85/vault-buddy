<script setup lang="ts">
/**
 * One labelled inspector field over a `useInspectorDraft` buffer (Task 31;
 * visual-parity Task 14, concept spec §5 `.field`): the label at 10px over
 * an 11px input the editor's base styles draw, Enter/blur submit, Escape
 * reverts, an invalid entry stays visible with its correction underneath
 * (SCREENS-AND-INTERACTIONS.md §04). With a `step` it is a number input,
 * so the arrow keys and the spinner move it by that much.
 */
import type { InspectorDraft } from "../../../composables/useInspectorDraft";

const props = withDefaults(
  defineProps<{
    field: InspectorDraft;
    label: string;
    testid: string;
    disabled?: boolean;
    /** Task 35: a cue's Text field reuses this for its Enter/blur/Escape
     * draft behaviour, and must not ask for a numeric keypad. */
    inputmode?: "decimal" | "text";
    step?: number;
    min?: number;
    max?: number;
  }>(),
  { disabled: false, inputmode: "decimal", step: undefined, min: undefined, max: undefined },
);

function onInput(event: Event): void {
  const buffer = props.field.draft;
  buffer.value = (event.target as HTMLInputElement).value;
}
</script>

<template>
  <label class="flex min-w-0 flex-col gap-[5px] text-[10px] text-fg-secondary">
    <span>{{ label }}</span>
    <input
      :data-testid="testid"
      :type="step === undefined ? 'text' : 'number'"
      :inputmode="inputmode"
      :step="step"
      :min="min"
      :max="max"
      class="w-full min-w-0 text-[11px] text-fg disabled:opacity-50"
      :disabled="disabled"
      :value="field.draft.value"
      @input="onInput"
      @keydown.enter="field.submit()"
      @keydown.escape="field.revert()"
      @blur="field.submit()"
    >
    <span
      v-if="field.error.value"
      :data-testid="`${testid}-error`"
      role="alert"
      class="text-danger-fg"
    >{{ field.error.value }}</span>
  </label>
</template>
