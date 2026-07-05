<script setup lang="ts">
import { computed } from "vue";
import { useCaptureStore } from "../stores/capture";

const capture = useCaptureStore();

const downloadPct = computed(() => {
  const d = capture.modelDownload;
  if (!d || !d.total) return null;
  return Math.min(100, Math.round((d.received / d.total) * 100));
});
</script>

<template>
  <div v-if="capture.transcribing || capture.transcriptError || capture.lastTranscribed">
    <div
      v-if="capture.transcribing"
      class="rounded-lg bg-violet-500/15 px-2 py-1.5 text-xs text-violet-100"
      role="status"
    >
      <span v-if="capture.modelDownload">
        Downloading {{ capture.modelDownload.model }} model<span v-if="downloadPct !== null">
          — {{ downloadPct }}%</span
        >…
      </span>
      <span v-else>Transcribing…</span>
    </div>
    <div
      v-else-if="capture.transcriptError"
      class="flex items-center justify-between gap-2 rounded-lg bg-red-500/20 px-2 py-1.5 text-xs text-red-200"
    >
      <span>Transcription failed: {{ capture.transcriptError }}</span>
      <button
        v-if="capture.transcriptFailedMp3"
        type="button"
        class="cursor-pointer rounded bg-red-500/80 px-2 py-0.5 font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300"
        @click="capture.retryTranscription()"
      >
        Retry
      </button>
    </div>
    <div
      v-else
      class="flex items-center justify-between gap-2 rounded-lg bg-emerald-500/15 px-2 py-1.5 text-xs text-emerald-100"
      role="status"
    >
      <span>✓ Transcribed</span>
      <button
        type="button"
        data-testid="open-transcript"
        class="cursor-pointer rounded bg-emerald-500/80 px-2 py-0.5 font-semibold text-white hover:bg-emerald-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
        @click="capture.openTranscript()"
      >
        Open in Obsidian
      </button>
    </div>
  </div>
</template>
