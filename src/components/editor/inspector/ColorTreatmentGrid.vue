<script setup lang="ts">
/**
 * The colour treatments as picture tiles (visual-parity Task 13; concept
 * spec §5 `.filter-grid`): three columns of a sample picture filtered the
 * way each treatment renders (`adjustmentsFilter`, the preview's own
 * mapping), labelled Original, Clear, Warm, Soft, Mono (`COLOR_TREATMENTS`).
 * The tile the selection already wears carries `.active`. A refusal (a
 * locked track) disables every tile with its reason.
 */
import { adjustmentsFilter, COLOR_TREATMENTS } from "../../../editor/colorPresets";

type Treatment = (typeof COLOR_TREATMENTS)[number];

defineProps<{ activeId: string | null; reason: string | null }>();
const emit = defineEmits<(e: "pick", treatment: Treatment) => void>();
</script>

<template>
  <div
    class="grid grid-cols-3 gap-2"
    role="group"
    aria-label="Colour treatments"
  >
    <button
      v-for="t in COLOR_TREATMENTS"
      :key="t.id"
      type="button"
      :data-testid="`color-treatment-${t.id}`"
      :aria-pressed="t.id === activeId"
      :aria-disabled="reason ? 'true' : undefined"
      :title="reason ?? undefined"
      class="flex min-w-0 flex-col gap-1.5 rounded-[7px] border p-1 text-[10px] focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="[
        t.id === activeId ? 'active border-accent bg-accent-bg text-accent-ink' : 'border-line text-fg-secondary',
        reason ? 'cursor-not-allowed opacity-45' : 'cursor-pointer hover:bg-hover',
      ]"
      @click="reason || emit('pick', t)"
    >
      <span
        class="relative block aspect-[1.25] overflow-hidden rounded bg-linear-135 from-swatch-from to-swatch-to"
        :style="{ filter: adjustmentsFilter(t.adjustments) }"
        aria-hidden="true"
      >
        <i class="absolute left-[-20%] top-[30%] h-[74%] w-[64%] rounded-full bg-swatch-shape" />
        <b class="absolute right-[9%] top-[20%] text-[21px] text-swatch-ink">Aa</b>
      </span>
      <span class="truncate">{{ t.label }}</span>
    </button>
  </div>
</template>
