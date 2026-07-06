<script setup lang="ts">
import { computed } from "vue";
import { useCaptureStore } from "../stores/capture";
import { useVaultsStore } from "../stores/vaults";

/**
 * Compact one-line summary on the panel's list view — the entry point into
 * the full Transcriptions view (Task 10). Mirrors that view's own priority:
 * the single active job (the worker queue runs one at a time) outranks a
 * failure, and with nothing active or failed there is nothing worth a line
 * for, so the component renders nothing at all.
 */
const capture = useCaptureStore();
const vaults = useVaultsStore();

const active = computed(() => capture.activeTranscription);

const failedCount = computed(
  () =>
    capture.finishedTranscriptions.filter((job) => job.phase === "failed")
      .length,
);

function percent(progress: number): number {
  return Math.round(Math.min(1, Math.max(0, progress)) * 100);
}

const summaryLabel = computed(() => {
  const job = active.value;
  if (job) {
    // "⟳" doubles as the spinner glyph while progress is unknown (preparing,
    // or a download with no total yet) — the percent segment is simply
    // dropped rather than showing a stale/misleading number.
    let text = `⟳ Transcribing "${job.name}"`;
    if (job.progress !== null) text += ` — ${percent(job.progress)}%`;
    const queued = capture.queuedTranscriptions.length;
    if (queued > 0) text += ` · +${queued} queued`;
    return text;
  }
  if (failedCount.value > 0) {
    const n = failedCount.value;
    return `⚠ ${n} transcription${n === 1 ? "" : "s"} failed`;
  }
  return null;
});

const hasSomethingToShow = computed(() => summaryLabel.value !== null);
</script>

<template>
  <div
    v-if="hasSomethingToShow"
    data-testid="transcription-summary"
    role="button"
    tabindex="0"
    :title="summaryLabel ?? undefined"
    class="cursor-pointer truncate rounded-lg border px-2 py-1 text-xs transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
    :class="
      active
        ? 'border-white/10 bg-violet-500/15 text-violet-100 hover:bg-violet-500/25'
        : 'border-white/10 bg-red-500/20 text-red-200 hover:bg-red-500/30'
    "
    @click="vaults.openTranscriptions()"
    @keydown.enter="vaults.openTranscriptions()"
    @keydown.space.prevent="vaults.openTranscriptions()"
  >{{ summaryLabel }}</div>
</template>
