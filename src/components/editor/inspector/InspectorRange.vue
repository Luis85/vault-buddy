<script setup lang="ts">
/**
 * One inspector range (visual-parity Task 14; concept spec §5 "Range fields
 * show label left + mono value right above the slider"): the value follows
 * the thumb while it moves, and `commit` runs once on release — a drag is
 * one edit, not one per pixel. A new committed value (an undo) moves the
 * thumb, and a refused commit (`false`, `useInspectorDraft`'s rule) puts it
 * back where the projection says it is.
 */
import { ref, watch } from "vue";

const props = defineProps<{
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix: string;
  testid: string;
  commit: (value: number) => Promise<boolean>;
}>();

const live = ref(props.value);
watch(
  () => props.value,
  (next) => (live.value = next),
);

function onInput(event: Event): void {
  live.value = Number((event.target as HTMLInputElement).value);
}
async function onChange(event: Event): Promise<void> {
  if ((await props.commit(Number((event.target as HTMLInputElement).value))) === false) live.value = props.value;
}
</script>

<template>
  <label class="flex min-w-0 flex-col gap-[5px] text-[10px] text-fg-secondary">
    <span class="flex justify-between">
      {{ label }}
      <span
        :data-testid="`${testid}-value`"
        class="vb-mono"
      >{{ Math.round(live * 100) / 100 }}{{ suffix }}</span>
    </span>
    <input
      :data-testid="testid"
      type="range"
      :min="min"
      :max="max"
      :step="step"
      :value="live"
      @input="onInput"
      @change="onChange"
    >
  </label>
</template>
