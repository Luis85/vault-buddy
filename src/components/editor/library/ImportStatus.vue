<script setup lang="ts">
/**
 * The media library's import status (Task 25): the running import's
 * progress bar and Cancel, or — once it has its terminal — the last
 * import's summary and per-file errors (display names only; Rust never
 * sends a path). Reads `editorJobs` directly, the `TrackHeader.vue`
 * leaf-reads-its-store precedent; split out of `MediaLibrary.vue` so
 * neither template carries both halves' branching.
 */
import { computed } from "vue";

import { useEditorJobsStore } from "../../../stores/editorJobs";

const jobs = useEditorJobsStore();

const progressPercent = computed(() => Math.round((jobs.activeImport?.fraction ?? 0) * 100));

function cancelImport(): void {
  const active = jobs.activeImport;
  if (active) void jobs.cancel(active.jobId);
}

function plural(n: number): string {
  return n === 1 ? "1 file" : `${n} files`;
}

const summaryLine = computed<string | null>(() => {
  const last = jobs.lastImport;
  if (!last?.terminal) return null;
  const kept = plural(last.terminal.assetIds?.length ?? 0);
  if (last.phase === "failed") return `Import failed: ${last.terminal.error?.message ?? "unknown error"}`;
  if (last.phase === "cancelled") return `Import stopped. ${kept} imported.`;
  return `Imported ${kept}.`;
});

const perFile = computed(() => jobs.lastImport?.terminal?.perFile ?? []);
</script>

<template>
  <div
    v-if="jobs.activeImport"
    class="flex items-center gap-1"
  >
    <div
      data-testid="library-import-progress"
      role="progressbar"
      aria-label="Importing media"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="progressPercent"
      class="h-1.5 flex-1 overflow-hidden rounded bg-white/10"
    >
      <div
        class="h-full bg-accent"
        :style="{ width: `${progressPercent}%` }"
      />
    </div>
    <button
      type="button"
      data-testid="library-import-cancel"
      title="Stop after the file being imported now; finished files stay"
      class="shrink-0 rounded px-1 hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      @click="cancelImport"
    >
      Cancel
    </button>
  </div>

  <div
    v-else-if="summaryLine"
    data-testid="library-import-summary"
    role="status"
    class="flex flex-col gap-0.5"
  >
    <p>{{ summaryLine }}</p>
    <ul class="flex flex-col gap-0.5 text-danger-fg">
      <li
        v-for="row in perFile"
        :key="row.name"
      >
        <span class="font-semibold">{{ row.name }}</span>: {{ row.error }}
      </li>
    </ul>
  </div>
</template>
