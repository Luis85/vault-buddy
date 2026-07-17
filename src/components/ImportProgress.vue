<script setup lang="ts">
import { computed } from "vue";

import { useNowTicker } from "../composables/useNowTicker";
import { useDocumentImportsStore } from "../stores/documentImports";
import { formatDuration } from "../utils/formatDuration";

/**
 * Working-state card for the single in-flight document conversion. Self-
 * contained (reads the store directly — the TranscriptionSummary pattern) so
 * every render site (RecordMode, ImportVaultPicker, the list view) shows the
 * same state, and renders nothing while idle. Pandoc reports no incremental
 * progress, so the visualization is honestly indeterminate: a spinner, the
 * elapsed time, and a sweeping activity bar — never a fake percentage.
 */
const imports = useDocumentImportsStore();

// Call sites v-if-gate mounting on `imports.active`, so the shared ticker
// only runs while a conversion is actually in flight.
const now = useNowTicker();

const elapsed = computed(() =>
  imports.active ? formatDuration(now.value - imports.active.startedAtMs) : "0:00",
);
</script>

<template>
  <!-- Sky accent: recording is red, transcription violet — a third color
       keeps concurrent background work distinguishable on the list view. -->
  <div
    v-if="imports.active"
    data-testid="import-progress"
    class="rounded-lg bg-sky-500/15 px-2 py-1.5"
  >
    <div class="flex items-center gap-2">
      <span
        class="h-3.5 w-3.5 shrink-0 animate-spin rounded-full border-2 border-sky-300/30 border-t-sky-300 motion-reduce:animate-none"
        aria-hidden="true"
      />
      <!-- role="status" is aria-live: only this stable label announces. The
           ticking timer stays OUTSIDE it, or a screen reader would chatter
           every second. -->
      <span
        role="status"
        class="min-w-0 flex-1 truncate text-sm font-medium text-sky-100"
      >
        Converting "{{ imports.active.fileName }}"
      </span>
      <span
        data-testid="import-elapsed"
        class="shrink-0 text-xs tabular-nums text-sky-200/80"
      >{{ elapsed }}</span>
    </div>
    <div
      data-testid="import-activity-bar"
      class="mt-1.5 h-1 overflow-hidden rounded-full bg-white/10"
      aria-hidden="true"
    >
      <div class="import-sweep h-full w-1/3 rounded-full bg-sky-400" />
    </div>
    <p class="mt-1 truncate text-xs text-sky-200/80">
      into {{ imports.active.vaultName || "your vault" }} — Pandoc is working,
      this can take a few seconds.
    </p>
  </div>
</template>

<style scoped>
/* Indeterminate sweep: a segment loops along the track to show live
   activity. Margins animate from fully off-track left to fully off-track
   right (the segment is w-1/3, so -35% clears it). */
.import-sweep {
  animation: import-sweep 1.4s ease-in-out infinite;
}
@keyframes import-sweep {
  from {
    margin-left: -35%;
  }
  to {
    margin-left: 100%;
  }
}
/* Reduced motion: a static partial bar still reads as "in progress" without
   the continuous sweep (the spinner opts out via motion-reduce:animate-none
   in the template). */
@media (prefers-reduced-motion: reduce) {
  .import-sweep {
    animation: none;
    margin-left: 33%;
  }
}
</style>
