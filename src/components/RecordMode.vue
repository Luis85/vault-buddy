<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useVaultsStore } from "../stores/vaults";
import { useCaptureStore } from "../stores/capture";
import type { CaptureConfig } from "../types";

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const capture = useCaptureStore();

const OPTIONS = [
  { key: "meeting", title: "Meeting", hint: "Microphone + desktop audio", testId: "mode-meeting" },
  { key: "voice-note", title: "Voice Note", hint: "Microphone only", testId: "mode-voice-note" },
] as const;

const defaultMode = ref<"meeting" | "voice-note">("meeting");

onMounted(async () => {
  // The chooser needs the vault's DEFAULT mode; a config read failure must
  // never block recording — fall back to meeting.
  try {
    const cfg = await invoke<CaptureConfig>("get_capture_config", { id: props.vaultId });
    defaultMode.value = cfg.mode;
  } catch {
    // stale config never blocks recording — mirror the backend's rule
  }
});

function start(mode: "meeting" | "voice-note") {
  void capture.start(props.vaultId, mode);
  store.showList(); // recording bar shows on the list view
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <button
      v-for="option in OPTIONS"
      :key="option.key"
      type="button"
      :data-testid="option.testId"
      :aria-label="`Start a ${option.title.toLowerCase()} recording`"
      class="w-full cursor-pointer rounded-lg border px-3 py-2 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      :class="
        option.key === defaultMode
          ? 'border-violet-400 bg-violet-500/20'
          : 'border-white/10 bg-white/5 hover:bg-white/10'
      "
      @click="start(option.key)"
    >
      <span class="block text-sm font-medium text-slate-100">{{ option.title }}</span>
      <span class="block text-xs text-slate-400">{{ option.hint }}</span>
    </button>
    <button
      type="button"
      data-testid="mode-browse"
      aria-label="Browse past recordings"
      class="mt-1 w-full cursor-pointer border-t border-white/10 pt-2 text-left text-xs text-slate-400 transition-colors hover:text-slate-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      @click="store.openRecordings(props.vaultId)"
    >
      Browse recordings…
      <span class="block text-slate-500">See past recordings in this vault</span>
    </button>
  </div>
</template>
