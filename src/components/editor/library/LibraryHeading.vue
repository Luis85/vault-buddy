<script setup lang="ts">
/**
 * A library tab's heading row (visual-parity Tasks 9–10, concept spec
 * §3.2–3.6 `.library-heading`): the uppercase eyebrow on the left and a
 * §1.5 pill on the right — a count ("5 assets", "2"), a state ("Local") or,
 * with `accent`, the project's revision ("r4"). One component so the five
 * library surfaces cannot drift apart. `testid` names both halves
 * (`<testid>-heading`, `<testid>-pill`).
 */
defineProps<{
  label: string;
  pill: string;
  testid: string;
  /** The accent pill (§1.5 `.pill.accent`): accent-bg / accent-ink. */
  accent?: boolean;
}>();
</script>

<template>
  <div class="mb-[11px] flex items-center justify-between gap-2">
    <span
      :data-testid="`${testid}-heading`"
      class="text-[10px] font-semibold tracking-[0.65px] text-fg-muted"
    >{{ label }}</span>
    <span
      :data-testid="`${testid}-pill`"
      class="shrink-0 rounded border px-1.5 py-0.5 text-[9px] tracking-[0.3px]"
      :class="accent ? 'border-accent/30 bg-accent-bg text-accent-ink' : 'border-line bg-raised text-fg-secondary'"
    >{{ pill }}</span>
  </div>
</template>
