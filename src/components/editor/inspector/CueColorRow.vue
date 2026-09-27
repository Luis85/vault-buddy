<script setup lang="ts">
/**
 * A teaching cue's colour (visual-parity Task 15; concept spec §5 "Teaching
 * cue", `.color-row`): the concept's five 27px round swatches, the one the
 * cue wears pressed, then a custom colour input. Each is one `updateEffect`
 * of `color`; picking the colour the cue already has sends nothing. A
 * locked track refuses every swatch with its reason.
 */
const SWATCHES = ["#ffd279", "#ffffff", "#ac93f1", "#7bd4bc", "#f297a7"] as const;

const props = defineProps<{ color: string; reason: string | null }>();
const emit = defineEmits<(e: "pick", color: string) => void>();

function pick(color: string): void {
  if (!props.reason && color !== props.color.toLowerCase()) emit("pick", color);
}
</script>

<template>
  <div class="flex items-center gap-1.5">
    <button
      v-for="c in SWATCHES"
      :key="c"
      type="button"
      :data-testid="`effect-swatch-${c.slice(1)}`"
      :aria-label="`Set color ${c}`"
      :aria-pressed="color.toLowerCase() === c"
      :aria-disabled="reason ? 'true' : undefined"
      :title="reason ?? undefined"
      class="grid h-[27px] min-h-[27px] w-[27px] shrink-0 place-items-center rounded-full border p-[3px] focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="[
        color.toLowerCase() === c ? 'active border-accent bg-accent-bg' : 'border-line',
        reason ? 'cursor-not-allowed opacity-45' : 'cursor-pointer hover:bg-hover',
      ]"
      @click="pick(c)"
    >
      <i
        class="block h-[19px] w-[19px] rounded-full"
        :style="{ background: c }"
      />
    </button>
    <input
      data-testid="effect-field-color"
      type="color"
      aria-label="Custom annotation color"
      class="h-[27px] w-7 cursor-pointer border-0 bg-transparent p-0 disabled:cursor-not-allowed disabled:opacity-45"
      :value="color"
      :disabled="reason !== null"
      :title="reason ?? undefined"
      @change="pick(($event.target as HTMLInputElement).value)"
    >
  </div>
</template>
