<script setup lang="ts">
/**
 * The Fades tab's "Blend · 0.5s / Blend · 1s" preset row (visual-parity
 * Task 15; concept spec §5 "Fades"). The caller decides what a blend does —
 * add a transition into the next clip, or set an existing one's overlap —
 * and why a length is refused; each button carries its own reason.
 */
import InspectorButton from "./InspectorButton.vue";

const BLENDS_MS = [500, 1_000] as const;

defineProps<{
  testid: string;
  reasonFor: (ms: number) => string | null;
  /** The overlap already in place, which reads as pressed; absent when
   * the row adds a blend rather than changing one. */
  currentMs?: number;
}>();
const emit = defineEmits<(e: "blend", ms: number) => void>();
</script>

<template>
  <div class="flex flex-wrap gap-[5px]">
    <InspectorButton
      v-for="ms in BLENDS_MS"
      :key="ms"
      preset
      :data-testid="`${testid}-${ms}`"
      :reason="reasonFor(ms)"
      :pressed="currentMs === undefined ? undefined : currentMs === ms"
      @click="emit('blend', ms)"
    >
      Blend · {{ ms / 1000 }}s
    </InspectorButton>
  </div>
</template>
