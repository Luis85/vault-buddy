<script setup lang="ts">
/**
 * One labelled numeric inspector field over a `useInspectorDraft` buffer
 * (Task 31): Enter/blur submit, Escape reverts, an invalid entry stays
 * visible with its correction underneath (SCREENS-AND-INTERACTIONS.md
 * §04). The Layout and Speed sections each have several of these; one
 * component keeps their markup from being copied field by field.
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
  }>(),
  { disabled: false, inputmode: "decimal" },
);

function onInput(event: Event): void {
  const buffer = props.field.draft;
  buffer.value = (event.target as HTMLInputElement).value;
}
</script>

<template>
  <label class="flex flex-col gap-0.5">
    <span class="text-fg-subtle">{{ label }}</span>
    <input
      :data-testid="testid"
      type="text"
      :inputmode="inputmode"
      class="rounded border border-line bg-stage px-1 py-0.5 text-fg disabled:opacity-50"
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
      class="text-danger"
    >{{ field.error.value }}</span>
  </label>
</template>
