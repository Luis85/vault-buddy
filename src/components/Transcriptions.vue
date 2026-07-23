<script setup lang="ts">
import { computed, watch } from "vue";

import { useNowTicker } from "../composables/useNowTicker";
import { useCaptureStore } from "../stores/capture";
import { useVaultsStore } from "../stores/vaults";
import type { TranscriptionJob } from "../types";
import { formatDuration } from "../utils/formatDuration";
import AppIcon from "./AppIcon.vue";
import EmptyState from "./ui/EmptyState.vue";
import IconButton from "./ui/IconButton.vue";

const capture = useCaptureStore();
const vaults = useVaultsStore();

const active = computed(() => capture.activeTranscription);
const queued = computed(() => capture.queuedTranscriptions);
const finished = computed(() => capture.finishedTranscriptions);
const isEmpty = computed(
  () =>
    !active.value &&
    !capture.waitingForRecording &&
    queued.value.length === 0 &&
    finished.value.length === 0,
);

// Ticks once a second so `elapsed` and the stuck-hint check stay live
// without a per-frame render loop — the shared RecordingBar/ImportProgress
// ticker.
const now = useNowTicker();

function percent(job: TranscriptionJob): number {
  return Math.round(Math.min(1, Math.max(0, job.progress ?? 0)) * 100);
}

/**
 * This is a cross-vault view — a job's vault may not be in this window's
 * vault list (not yet discovered, or discovered by a different window), so
 * fall back to the raw id rather than showing nothing.
 */
function vaultName(id: string): string {
  return vaults.vaults.find((v) => v.id === id)?.name ?? id;
}

function phaseLabel(job: TranscriptionJob): string {
  switch (job.phase) {
    case "downloading":
      // An unknown total (no percent yet) must read as "still working", not
      // a misleading "0%" — omit the number entirely rather than guess.
      return job.progress != null
        ? `Downloading ${job.model ?? "model"}… ${percent(job)}%`
        : `Downloading ${job.model ?? "model"}…`;
    case "preparing":
      return "Preparing…";
    case "transcribing":
      return job.progress != null
        ? `Transcribing… ${percent(job)}%`
        : "Transcribing…";
    default:
      return job.phase;
  }
}

function elapsed(startedAtMs: number | null): string {
  if (startedAtMs === null) return "0:00";
  return formatDuration(now.value - startedAtMs);
}

const STATUS_META: Record<string, { glyph: string; label: string }> = {
  done: { glyph: "✓", label: "Transcribed" },
  failed: { glyph: "⚠", label: "Failed" },
  cancelled: { glyph: "⦸", label: "Cancelled" },
};
function statusGlyph(job: TranscriptionJob): string {
  return STATUS_META[job.phase]?.glyph ?? "";
}

// --- "taking longer than expected" hint -------------------------------
// A transcribing job's own inference progress can legitimately sit at the
// same percent for a while (whisper reports coarse steps), so this only
// flags a stall once the CURRENT job's percent hasn't moved for STUCK_MS.
// The "since" clock lives in the capture store (`noteActiveProgress`), not
// a local ref: this view is destroyed and recreated every time the panel
// navigates away and back, and a component-local ref would restart the
// clock on every remount. The store only resets it on a REAL change
// (different job, or an actual progress delta) — a re-upsert with the
// identical percent must not, or a slow-but-alive job would never trip the
// hint, and a fresh mount re-observing the same job/progress is a no-op.
const STUCK_MS = 2 * 60 * 1000;

watch(
  () => capture.activeTranscription,
  (job) => capture.noteActiveProgress(job),
  { immediate: true },
);

const isStuck = computed(() => {
  const job = active.value;
  if (!job || job.phase !== "transcribing") return false;
  if (job.mp3 !== capture.activeStuckMp3 || capture.activeStuckSinceMs === null) {
    return false;
  }
  return now.value - capture.activeStuckSinceMs >= STUCK_MS;
});
</script>

<template>
  <div class="flex flex-col gap-3">
    <EmptyState
      v-if="isEmpty"
      title="No transcriptions yet."
    >
      <template #icon>
        <AppIcon :size="28">
          <path d="M4 4h16v16H4Z" />
          <path d="M8 9h8M8 13h5" />
        </AppIcon>
      </template>
    </EmptyState>

    <section
      v-if="active || capture.waitingForRecording"
      data-testid="transcription-active"
    >
      <h2 class="mb-1 text-xs font-semibold uppercase tracking-wide text-fg-muted">
        Active
      </h2>
      <div
        v-if="active"
        class="flex flex-col gap-1.5 rounded-control border border-white/10 bg-white/5 px-2 py-1.5"
      >
        <div class="flex items-baseline justify-between gap-2">
          <span
            class="min-w-0 flex-1 truncate text-sm text-fg"
            :title="active.name"
          >{{ active.name }}</span>
          <span class="shrink-0 text-xs text-fg-subtle">{{ vaultName(active.vaultId) }}</span>
        </div>
        <div class="flex items-center justify-between gap-2 text-xs text-fg-secondary">
          <span role="status">{{ phaseLabel(active) }}</span>
          <span class="shrink-0 text-fg-subtle">{{ elapsed(active.startedAtMs) }}</span>
        </div>
        <div
          v-if="active.progress !== null"
          data-testid="transcription-progress"
          role="progressbar"
          :aria-valuenow="percent(active)"
          aria-valuemin="0"
          aria-valuemax="100"
          class="h-1.5 overflow-hidden rounded-full bg-white/10"
        >
          <div
            class="h-full rounded-full bg-violet-400 transition-[width] duration-200"
            :style="{ width: percent(active) + '%' }"
          />
        </div>
        <div
          v-else
          data-testid="transcription-progress"
          class="h-3 w-3 animate-spin rounded-full border-2 border-white/30 border-t-white"
          role="status"
          :aria-label="phaseLabel(active)"
        />
        <p
          v-if="isStuck"
          data-testid="transcription-stuck-hint"
          class="text-xs text-amber-300"
        >
          Taking longer than expected…
        </p>
        <button
          type="button"
          data-testid="transcription-cancel"
          :aria-label="`Cancel ${active.name}`"
          class="cursor-pointer self-end rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="capture.cancelTranscription(active.mp3)"
        >
          Cancel
        </button>
      </div>
      <p
        v-else
        class="text-xs text-fg-muted"
      >
        Waiting for the recording to finish…
      </p>
    </section>

    <section
      v-if="queued.length > 0"
      data-testid="transcription-queued"
    >
      <h2 class="mb-1 text-xs font-semibold uppercase tracking-wide text-fg-muted">
        Queued <span class="text-fg-subtle">· {{ queued.length }}</span>
      </h2>
      <div class="flex flex-col gap-1">
        <div
          v-for="j in queued"
          :key="j.mp3"
          class="flex items-center gap-2 rounded-control border border-white/10 bg-white/5 px-2 py-1"
        >
          <span
            class="min-w-0 flex-1 truncate text-sm text-fg"
            :title="j.name"
          >
            {{ j.name }}
          </span>
          <span class="shrink-0 text-xs text-fg-subtle">{{ vaultName(j.vaultId) }} · Waiting</span>
          <button
            type="button"
            data-testid="transcription-cancel"
            :aria-label="`Cancel ${j.name}`"
            class="shrink-0 cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
            @click="capture.cancelTranscription(j.mp3)"
          >
            Cancel
          </button>
        </div>
      </div>
    </section>

    <section
      v-if="finished.length > 0"
      data-testid="transcription-finished"
    >
      <h2 class="mb-1 text-xs font-semibold uppercase tracking-wide text-fg-muted">
        Finished this session
      </h2>
      <div class="flex flex-col gap-1">
        <div
          v-for="j in finished"
          :key="j.mp3"
          class="flex flex-col gap-1 rounded-control border border-white/10 bg-white/5 px-2 py-1"
        >
          <div class="flex items-center gap-2">
            <span
              aria-hidden="true"
              class="shrink-0 text-fg-subtle"
            >{{ statusGlyph(j) }}</span>
            <span
              class="min-w-0 flex-1 truncate text-sm text-fg"
              :title="j.name"
            >
              {{ j.name }}
            </span>
            <button
              v-if="j.phase === 'done'"
              type="button"
              data-testid="transcription-open"
              :aria-label="`Open ${j.name} in Obsidian`"
              class="shrink-0 cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
              @click="capture.openTranscript(j.mp3)"
            >
              Open in Obsidian
            </button>
            <button
              v-else
              type="button"
              data-testid="transcription-retranscribe"
              :aria-label="`Re-transcribe ${j.name}`"
              class="shrink-0 cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
              @click="capture.retranscribe(j.mp3)"
            >
              Re-transcribe
            </button>
            <!-- Clear a finished row (and, for a failure, its error) from the
                 list. The row used to be undismissable — it just lingered. -->
            <IconButton
              data-testid="transcription-dismiss"
              size="sm"
              :label="`Dismiss ${j.name}`"
              :title="`Dismiss ${j.name}`"
              @click="capture.dismissTranscription(j.mp3)"
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="3"
                stroke-linecap="round"
                aria-hidden="true"
              >
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </IconButton>
          </div>
          <p
            v-if="j.phase === 'failed' && j.error"
            class="text-xs text-danger-fg"
          >
            {{ j.error }}
          </p>
        </div>
      </div>
    </section>
  </div>
</template>
