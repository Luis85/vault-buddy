<script setup lang="ts">
/**
 * One mixer level (Task 27): a linear `0..max` range slider and its mono
 * readout, committing ONCE on release. Dragging (`input`) only moves the
 * readout — and the slider stays bound to that dragged value, or the
 * readout's own re-render would patch the slider back to the committed
 * value under the pointer. The release (`change`) calls `commit` once; a
 * refusal (`false`) snaps the slider back to the committed value.
 *
 * Visual-parity Task 22 (concept spec §9.8): the range and its readout are
 * two cells of the mixer row's `125px 1fr 52px` grid — this renders them
 * as siblings, and the row owns the name cell. The readout is dB for a
 * track (`formatDb`), a percent for the master (`format`).
 */
import { computed, ref } from "vue";

import { formatDb } from "../../../editor/mixRules";

const props = withDefaults(
  defineProps<{
    sliderLabel: string;
    testid: string;
    readoutTestid: string;
    /** The committed linear value. */
    value: number;
    max: number;
    disabled?: boolean;
    commit: (value: number) => Promise<boolean>;
    format?: (value: number) => string;
  }>(),
  { disabled: false, format: formatDb },
);

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
  <input
    :data-testid="testid"
    type="range"
    min="0"
    :max="max"
    step="0.01"
    class="w-full min-w-0"
    :aria-label="sliderLabel"
    :disabled="disabled"
    :value="shown"
    @input="onInput"
    @change="onChange"
  >
  <span
    :data-testid="readoutTestid"
    class="text-right font-mono text-[11px] tabular-nums text-fg-secondary"
  >{{ format(shown) }}</span>
</template>
