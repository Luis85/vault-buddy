<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useVaultsStore } from "../stores/vaults";
import { logWarning } from "../logging";
import type { Recording } from "../types";

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const openError = ref<string | null>(null);
const recordings = ref<Recording[]>([]);
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
    store.panelOpen = false; // Obsidian takes over — get out of the way
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
        <button
          v-for="r in section.items"
          :key="r.mp3"
          type="button"
          data-testid="recording-row"
          class="flex w-full items-baseline justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
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
      </div>
    </section>
  </div>
</template>
