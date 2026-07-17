<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { TranscriptionModelStatus } from "../types";

// Cache visibility + guarded delete for the whisper model artifacts
// (Buddy settings > Integrations, beside TranscriptionAppSettings) — the
// user-facing remedy for a suspect cached model (docs/Gaps.md GAP-14).
// Delete is destructive (the model must re-download, ~seconds to minutes
// depending on tier) so it sits behind an in-panel confirm that names the
// re-download cost, mirroring the rest of the app's never-clobber posture
// even though this is app-owned cache, not vault content.

const LABELS: Record<string, string> = {
  base: "Base",
  small: "Small",
  medium: "Medium",
  turbo: "Turbo",
  vad: "VAD (silence filter)",
};

const models = ref<TranscriptionModelStatus[]>([]);
const confirmingId = ref<string | null>(null);
const busyId = ref<string | null>(null);
const error = ref<string | null>(null);

function formatSize(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  return `${Math.round(bytes / 1_048_576)} MB`;
}

async function refresh() {
  try {
    models.value = await invoke<TranscriptionModelStatus[]>("list_transcription_models");
  } catch (e) {
    error.value = String(e);
    logWarning(`list_transcription_models failed: ${String(e)}`);
  }
}

onMounted(refresh);

async function confirmDelete(id: string) {
  confirmingId.value = null;
  busyId.value = id;
  error.value = null;
  try {
    await invoke("delete_transcription_model", { id });
    await refresh();
  } catch (e) {
    error.value = String(e);
    logWarning(`delete_transcription_model(${id}) failed: ${String(e)}`);
  } finally {
    busyId.value = null;
  }
}
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Transcription models
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div
        v-for="m in models"
        :key="m.id"
        :data-testid="`model-row-${m.id}`"
        class="flex items-center justify-between gap-2"
      >
        <div class="text-sm text-slate-200">
          {{ LABELS[m.id] ?? m.id }}
          <span class="block text-xs text-slate-500">
            <template v-if="m.present">{{ formatSize(m.sizeBytes ?? 0) }}</template>
            <template v-else>not downloaded (~{{ formatSize(m.approxDownloadBytes) }})</template>
          </span>
        </div>
        <div
          v-if="confirmingId === m.id"
          class="flex items-center gap-1.5 text-right"
        >
          <span class="text-xs text-slate-400">
            Deleting frees the disk — downloading again costs
            ~{{ formatSize(m.approxDownloadBytes) }}.
          </span>
          <button
            :data-testid="`model-confirm-${m.id}`"
            type="button"
            class="rounded-md bg-rose-500/20 px-2 py-1 text-xs text-rose-300 hover:bg-rose-500/30 disabled:opacity-50"
            :disabled="busyId === m.id"
            @click="confirmDelete(m.id)"
          >
            Delete
          </button>
          <button
            :data-testid="`model-cancel-${m.id}`"
            type="button"
            class="rounded-md bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10"
            @click="confirmingId = null"
          >
            Cancel
          </button>
        </div>
        <button
          v-else-if="m.present"
          :data-testid="`model-delete-${m.id}`"
          type="button"
          class="rounded-md bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10 disabled:opacity-50"
          :disabled="busyId !== null"
          @click="confirmingId = m.id"
        >
          Delete
        </button>
      </div>
      <p
        v-if="error"
        data-testid="models-error"
        class="text-xs text-rose-400"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
