<script setup lang="ts">
/**
 * A teaching cue's colour (visual-parity Task 15; concept spec §5 "Teaching
 * cue", `.color-row`): the concept's five 27px round swatches, the one the
 * cue wears pressed, then a custom colour input. Each is one `updateEffect`
 * of `color`; picking the colour the cue already has sends nothing. A
 * locked track refuses every swatch with its reason.
 *
 * Each swatch is named by its colour ("Set color gold"), never its hex
 * code, and keeps its fill in forced colours (`vb-swatch-fill`, the
 * forced-colours block in `style.css`), where the chosen one is outlined.
 */
import { announceDisabled } from "../../../editor/disabledAnnouncer";

const SWATCHES = [
  { color: "#ffd279", name: "gold" },
  { color: "#ffffff", name: "white" },
  { color: "#ac93f1", name: "violet" },
  { color: "#7bd4bc", name: "teal" },
  { color: "#f297a7", name: "pink" },
] as const;

const props = defineProps<{ color: string; reason: string | null }>();
const emit = defineEmits<(e: "pick", color: string) => void>();

function pick(color: string): void {
  if (props.reason) announceDisabled(props.reason);
  else if (color !== props.color.toLowerCase()) emit("pick", color);
}
</script>

<template>
  <div class="flex items-center gap-1.5">
    <button
      v-for="s in SWATCHES"
      :key="s.color"
      type="button"
      :data-testid="`effect-swatch-${s.color.slice(1)}`"
      :aria-label="`Set color ${s.name}`"
      :aria-pressed="color.toLowerCase() === s.color"
      :aria-disabled="reason ? 'true' : undefined"
      :title="reason ?? undefined"
      class="vb-swatch grid h-[27px] min-h-[27px] w-[27px] shrink-0 place-items-center rounded-full border p-[3px] focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="[
        color.toLowerCase() === s.color ? 'active border-accent bg-accent-bg' : 'border-line',
        reason ? 'cursor-not-allowed opacity-45' : 'cursor-pointer hover:bg-hover',
      ]"
      @click="pick(s.color)"
    >
      <i
        class="vb-swatch-fill block h-[19px] w-[19px] rounded-full"
        :style="{ background: s.color }"
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
