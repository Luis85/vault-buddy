<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { TranscriptionAppConfig } from "../types";

// App-global GPU escape hatch (Buddy settings > Integrations, beside
// McpSettings/DocumentImportSettings) — the visible off-switch for the
// GPU-Vulkan increment (defaults on at the Rust layer; this card is what
// lets a user with a flaky graphics driver turn it back off). Optimistic
// with revert-on-failure (the autostart-toggle pattern in BuddySettings);
// busy disables the checkbox so two writes can't race. Always rendered, no
// v-if gating on load state (the DocumentImportSettings precedent) — a
// failed get_transcription_config must still show the error + a usable
// (if disabled-until-resolved) control, not hide the whole card.
const useGpu = ref<boolean | null>(null); // null = load pending/failed
const busy = ref(false);
const error = ref<string | null>(null);

onMounted(async () => {
  try {
    const cfg = await invoke<TranscriptionAppConfig>("get_transcription_config");
    useGpu.value = cfg.useGpu;
  } catch (e) {
    error.value = String(e);
    logWarning(`transcription app settings: get_transcription_config failed: ${String(e)}`);
  }
});

async function toggle(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked;
  const previous = useGpu.value;
  useGpu.value = enabled;
  busy.value = true;
  error.value = null;
  try {
    await invoke("set_transcription_config", { cfg: { useGpu: enabled } });
  } catch (e) {
    useGpu.value = previous;
    error.value = String(e);
    logWarning(`transcription app settings: set_transcription_config failed: ${String(e)}`);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Transcription — GPU
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <label
          for="transcription-use-gpu"
          class="text-sm text-slate-200"
        >
          Use GPU (Vulkan)
          <span class="block text-xs text-slate-500">
            Applies from the next transcription. Falls back to CPU when no
            compatible GPU is found — turn off if you hit graphics-driver
            crashes.
          </span>
        </label>
        <input
          id="transcription-use-gpu"
          data-testid="use-gpu-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500 disabled:cursor-default disabled:opacity-50"
          :checked="useGpu === true"
          :disabled="useGpu === null || busy"
          @change="toggle"
        >
      </div>
      <p
        v-if="error"
        data-testid="use-gpu-error"
        class="text-xs text-rose-400"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
