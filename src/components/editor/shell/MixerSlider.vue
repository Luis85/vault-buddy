<script setup lang="ts">
/**
 * One mixer level (Task 27): a linear `0..max` range slider with a dB
 * readout, committing ONCE on release. Dragging (`input`) only moves the
 * readout — and the slider stays bound to that dragged value, or the
 * readout's own re-render would patch the slider back to the committed
 * value under the pointer. The release (`change`) calls `commit` once; a
 * refusal (`false`) snaps the slider back to the committed value.
 */
import { computed, ref } from "vue";

import { formatDb } from "../../../editor/mixRules";

const props = defineProps<{
  label: string;
  sliderLabel: string;
  testid: string;
  /** The committed linear value. */
  value: number;
  max: number;
  disabled?: boolean;
  commit: (value: number) => Promise<boolean>;
}>();

const dragging = ref<number | null>(null);
const shown = computed(() => dragging.value ?? props.value);

function onInput(event: Event): void {
  dragging.value = Number((event.target as HTMLInputElement).value);
}
async function onChange(event: Event): Promise<void> {
  const el = event.target as HTMLInputElement;
  const ok = await props.commit(Number(el.value));
  dragging.value = null;
  if (!ok) el.value = String(props.value);
}
</script>

<template>
  <label class="flex flex-col gap-0.5">
    <span class="flex justify-between">
      <span class="truncate text-fg">{{ label }}</span>
      <span class="tabular-nums">{{ formatDb(shown) }}</span>
    </span>
    <input
      :data-testid="testid"
      type="range"
      min="0"
      :max="max"
      step="0.01"
      class="accent-violet-500"
      :aria-label="sliderLabel"
      :disabled="disabled"
      :value="shown"
      @input="onInput"
      @change="onChange"
    >
  </label>
</template>
