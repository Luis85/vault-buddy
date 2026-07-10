<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useCaptureStore } from "../stores/capture";
import type { Phase, Recording } from "../types";

const props = defineProps<{ vaultId: string }>();

const capture = useCaptureStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const openError = ref<string | null>(null);
const recordings = ref<Recording[]>([]);
// mp3 awaiting a "replace the current transcript?" confirm (complete only).
const confirmMp3 = ref<string | null>(null);
// Per-view, not persisted: resets to grouped every time the view opens.
const grouped = ref(true);

const UNGROUPED = "Ungrouped";

// Phases that occupy the single-worker transcription queue: the store's own
// ACTIVE_PHASES plus "queued" (a queued job hasn't started running, but its
// row is still busy/cancellable, so this component's notion of "active" is
// intentionally broader). A row is "busy" only while ITS mp3 has a live
// entry in `capture.transcriptions` — backend-seeded on store init and kept
// live by capture:* events the store owns — never a component-local ref.
// The old local `transcribingMp3` Set started empty on every remount (this
// view is destroyed/recreated on each view navigation), so it forgot an
// in-flight job and wrongly showed the row as idle: the stale re-transcribe
// bug this task fixes.
const ACTIVE_PHASES: Phase[] = ["queued", "downloading", "preparing", "transcribing"];

function jobPhase(mp3: string): Phase | undefined {
  return capture.transcriptions[mp3]?.phase;
}

function isActive(mp3: string): boolean {
  const phase = jobPhase(mp3);
  return phase !== undefined && ACTIVE_PHASES.includes(phase);
}

/**
 * Display status for a row: the persisted `transcriptStatus` from the last
 * `list_recordings` fetch, overridden by a job that has already reached a
 * terminal phase THIS session (done/failed/cancelled) — so a completion
 * doesn't sit there mislabeled "Transcribing…" (with an enabled button) until
 * the view happens to remount and refetch.
 */
function effectiveStatus(r: Recording): Recording["transcriptStatus"] {
  const phase = jobPhase(r.mp3);
  if (phase === "done") return "complete";
  if (phase === "cancelled") return "cancelled";
  if (phase === "failed") return "failed";
  return r.transcriptStatus;
}

// One shape drives both modes: flat mode is a single header-less section;
// grouped mode is one section per type with Ungrouped forced last. Recordings
// arrive newest-first, so each section's rows stay newest-first.
const sections = computed<Array<{ type: string | null; items: Recording[] }>>(() => {
  if (!grouped.value) return [{ type: null, items: recordings.value }];
  const map = new Map<string, Recording[]>();
  for (const r of recordings.value) {
    const key = r.type ?? UNGROUPED;
    const list = map.get(key);
    if (list) list.push(r);
    else map.set(key, [r]);
  }
  return [...map.entries()]
    .sort(([a], [b]) => (a === UNGROUPED ? 1 : b === UNGROUPED ? -1 : 0))
    .map(([type, items]) => ({ type, items }));
});

function statusLabel(r: Recording): string {
  if (isActive(r.mp3)) return "Transcribing…";
  return { none: "", pending: "Transcribing…", failed: "Transcript failed", complete: "Transcribed ✓", cancelled: "Cancelled" }[effectiveStatus(r)];
}

/**
 * Hover text for the status indicator: a live job's failure reason
 * (`job.error`, set only on a same-session "failed" transition) when there
 * is one, else the generic label. A historical failure fetched from
 * `list_recordings` has no live job — the reason isn't persisted there — so
 * it always falls back to the generic "Transcript failed".
 */
function statusTitle(r: Recording): string {
  return capture.transcriptions[r.mp3]?.error ?? statusLabel(r);
}

async function runRetranscribe(mp3: string) {
  confirmMp3.value = null;
  await capture.retranscribe(mp3);
}

function onRetranscribeClick(r: Recording) {
  // A finished (or hand-edited) transcript needs a confirm before we clobber it.
  if (effectiveStatus(r) === "complete") confirmMp3.value = r.mp3;
  else void runRetranscribe(r.mp3);
}

onMounted(async () => {
  try {
    recordings.value = await invoke<Recording[]>("list_recordings", {
      id: props.vaultId,
    });
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

async function open(mp3: string) {
  openError.value = null;
  try {
    await invoke("open_recording", { path: mp3 });
    // Obsidian takes over — get the panel out of the way. Panel visibility is
    // owned by Rust in the split-window architecture (close_panel), not a store
    // flag; best-effort, mirroring the vault-open path in the vaults store.
    void invoke("close_panel").catch(() => {});
  } catch (e) {
    // A failed open (recording moved, launch error) is non-fatal — surface it
    // and keep the list so the user can pick another.
    openError.value = String(e);
    logWarning(`open recording rejected: ${String(e)}`);
  }
}
</script>

<template>
  <p
    v-if="loading"
    class="text-xs text-slate-400"
  >
    Loading…
  </p>
  <p
    v-else-if="loadError"
    class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
  >
    {{ loadError }}
  </p>
  <p
    v-else-if="recordings.length === 0"
    class="text-xs text-slate-400"
  >
    No recordings yet.
  </p>
  <div
    v-else
    class="flex flex-col gap-2"
  >
    <p
      v-if="openError"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ openError }}
    </p>
    <div
      v-if="confirmMp3"
      data-testid="retranscribe-confirm-row"
      class="flex items-center justify-between gap-2 rounded-lg border border-amber-400/30 bg-amber-500/10 px-2 py-1 text-xs text-amber-100"
    >
      <span>Replace the current transcript?</span>
      <span class="flex gap-1">
        <button
          type="button"
          data-testid="retranscribe-confirm"
          class="cursor-pointer rounded bg-amber-500/30 px-2 py-0.5 hover:bg-amber-500/40 focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-300"
          @click="runRetranscribe(confirmMp3)"
        >Replace</button>
        <button
          type="button"
          data-testid="retranscribe-cancel"
          class="cursor-pointer rounded bg-white/10 px-2 py-0.5 hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="confirmMp3 = null"
        >Cancel</button>
      </span>
    </div>
    <div class="flex items-center justify-between">
      <span class="text-xs text-slate-400">
        {{ recordings.length }} recording{{ recordings.length === 1 ? "" : "s" }}
      </span>
      <button
        type="button"
        data-testid="group-toggle"
        class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :aria-pressed="grouped"
        @click="grouped = !grouped"
      >
        {{ grouped ? "Grouped by type" : "Flat list" }}
      </button>
    </div>
    <section
      v-for="(section, i) in sections"
      :key="section.type ?? `flat-${i}`"
    >
      <h2
        v-if="section.type"
        class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-400"
      >
        {{ section.type }}
        <span class="text-slate-500">· {{ section.items.length }}</span>
      </h2>
      <div class="flex flex-col gap-1">
        <div
          v-for="r in section.items"
          :key="r.mp3"
          class="flex items-center gap-1"
        >
          <button
            type="button"
            data-testid="recording-row"
            class="flex min-w-0 flex-1 items-baseline justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
            @click="open(r.mp3)"
          >
            <span
              class="min-w-0 flex-1 truncate text-sm text-slate-100"
              :title="r.title"
            >
              {{ r.title }}
            </span>
            <span class="shrink-0 text-xs text-slate-400">{{ r.recordedAt }}</span>
            <span class="shrink-0 text-xs text-slate-500">{{ r.duration ?? "—" }}</span>
          </button>
          <span
            v-if="statusLabel(r)"
            class="shrink-0 text-[10px] text-slate-500"
            :title="statusTitle(r)"
          >
            <span
              v-if="isActive(r.mp3)"
              data-testid="recording-spinner"
              role="status"
              aria-label="Transcribing…"
              class="inline-block h-2.5 w-2.5 animate-spin rounded-full border-2 border-slate-500/40 border-t-slate-300 align-middle"
            />
            <span v-else>{{ effectiveStatus(r) === "failed" ? "⚠" : effectiveStatus(r) === "complete" ? "✓" : effectiveStatus(r) === "cancelled" ? "⦸" : "…" }}</span>
          </span>
          <button
            v-if="isActive(r.mp3)"
            type="button"
            data-testid="recording-cancel"
            :aria-label="`Cancel transcribing ${r.title}`"
            title="Cancel"
            class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
            @click="capture.cancelTranscription(r.mp3)"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
          <button
            type="button"
            data-testid="retranscribe"
            :disabled="isActive(r.mp3)"
            :aria-label="`Re-transcribe ${r.title}`"
            title="Re-transcribe"
            class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
            @click="onRetranscribeClick(r)"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M23 4v6h-6M1 20v-6h6" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
