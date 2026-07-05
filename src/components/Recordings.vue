<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logWarning } from "../logging";
import type { Recording } from "../types";

const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const openError = ref<string | null>(null);
const recordings = ref<Recording[]>([]);
// mp3 currently being (re)transcribed → row shows a spinner. Seeded on click
// and by capture:transcribing; cleared by transcribed/transcribeFailed.
const transcribingMp3 = ref<Set<string>>(new Set());
// mp3 awaiting a "replace the current transcript?" confirm (complete only).
const confirmMp3 = ref<string | null>(null);
// Per-view, not persisted: resets to grouped every time the view opens.
const grouped = ref(true);

const UNGROUPED = "Ungrouped";

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
  if (transcribingMp3.value.has(r.mp3)) return "Transcribing…";
  return { none: "", pending: "Transcribing…", failed: "Transcript failed", complete: "Transcribed ✓" }[r.transcriptStatus];
}

async function runRetranscribe(mp3: string) {
  confirmMp3.value = null;
  transcribingMp3.value = new Set(transcribingMp3.value).add(mp3);
  try {
    await invoke("retranscribe", { path: mp3 });
  } catch (e) {
    clearTranscribing(mp3);
    openError.value = String(e);
    logWarning(`retranscribe rejected: ${String(e)}`);
  }
}

function onRetranscribeClick(r: Recording) {
  // A finished (or hand-edited) transcript needs a confirm before we clobber it.
  if (r.transcriptStatus === "complete") confirmMp3.value = r.mp3;
  else void runRetranscribe(r.mp3);
}

function clearTranscribing(mp3: string) {
  transcribingMp3.value = new Set([...transcribingMp3.value].filter((m) => m !== mp3));
}

// Recordings.vue is destroyed and recreated on every panel close and every
// view navigation (a v-else-if in ActionPanel.vue keyed by
// recordingsVaultId), so each remount's listeners must be torn down or they
// leak — collected here and released in onUnmounted below.
const unlisteners: Array<() => void> = [];

onMounted(async () => {
  unlisteners.push(
    await listen<{ mp3: string }>("capture:transcribing", (e) => {
      transcribingMp3.value = new Set(transcribingMp3.value).add(e.payload.mp3);
    }),
  );
  unlisteners.push(
    await listen<{ mp3: string }>("capture:transcribed", (e) => {
      clearTranscribing(e.payload.mp3);
      const row = recordings.value.find((r) => r.mp3 === e.payload.mp3);
      if (row) row.transcriptStatus = "complete";
    }),
  );
  unlisteners.push(
    await listen<{ mp3: string }>("capture:transcribeFailed", (e) => {
      clearTranscribing(e.payload.mp3);
      const row = recordings.value.find((r) => r.mp3 === e.payload.mp3);
      if (row) row.transcriptStatus = "failed";
    }),
  );
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

onUnmounted(() => {
  for (const u of unlisteners) u();
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
  <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
  <p
    v-else-if="loadError"
    class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
  >
    {{ loadError }}
  </p>
  <p v-else-if="recordings.length === 0" class="text-xs text-slate-400">
    No recordings yet.
  </p>
  <div v-else class="flex flex-col gap-2">
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
            <span class="min-w-0 flex-1 truncate text-sm text-slate-100" :title="r.title">
              {{ r.title }}
            </span>
            <span class="shrink-0 text-xs text-slate-400">{{ r.recordedAt }}</span>
            <span class="shrink-0 text-xs text-slate-500">{{ r.duration ?? "—" }}</span>
          </button>
          <span
            v-if="statusLabel(r)"
            class="shrink-0 text-[10px] text-slate-500"
            :title="statusLabel(r)"
          >{{ transcribingMp3.has(r.mp3) || r.transcriptStatus === "pending" ? "…" : r.transcriptStatus === "failed" ? "⚠" : "✓" }}</span>
          <button
            type="button"
            data-testid="retranscribe"
            :disabled="transcribingMp3.has(r.mp3)"
            :aria-label="`Re-transcribe ${r.title}`"
            title="Re-transcribe"
            class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
            @click="onRetranscribeClick(r)"
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M23 4v6h-6M1 20v-6h6" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
