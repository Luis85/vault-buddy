<script setup lang="ts">
/**
 * The reading-density and overlap notices (Task 36; F-34:
 * "overlap/density warnings are actionable"): one row per notice, each with
 * a "Select cue" that hands the cue back to `CaptionsLibrary` to reveal.
 * Editorial heuristics, stated as such -- never a pass/fail score.
 */
import type { CaptionNotice, CaptionRow } from "../../../editor/captionRules";

defineProps<{ notices: CaptionNotice[] }>();

const emit = defineEmits<{ (e: "select", row: CaptionRow): void }>();
</script>

<template>
  <ul
    data-testid="caption-notices"
    aria-label="Caption notices"
    class="flex flex-col gap-0.5"
  >
    <li
      v-for="notice in notices"
      :key="`${notice.kind}-${notice.row.cue.id}`"
      :data-testid="`caption-notice-${notice.kind}-${notice.row.cue.id}`"
      class="flex items-center justify-between gap-1 rounded bg-gold-bg px-1 text-gold"
    >
      <span>{{ notice.message }}</span>
      <button
        type="button"
        class="shrink-0 rounded px-1 underline hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
        @click="emit('select', notice.row)"
      >
        Select cue
      </button>
    </li>
  </ul>
</template>
