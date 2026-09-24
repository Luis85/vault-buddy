<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { CaptureWebcamInfo } from "../types";
import { webcamsFrom } from "../utils/captureWebcams";
import SectionHeader from "./ui/SectionHeader.vue";

// F-22: a webcam recorded BESIDE the screen, on the same clock, into its own
// file the editor places as the presenter. Optional: "No webcam" is the
// default and sends exactly the start every capture sent before this.
const props = defineProps<{ webcamId: string | null }>();
const emit = defineEmits<{ (e: "update:webcamId", value: string | null): void }>();

const webcams = ref<CaptureWebcamInfo[]>([]);

function onChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  emit("update:webcamId", value === "" ? null : value);
}

onMounted(async () => {
  try {
    webcams.value = webcamsFrom(await invoke("list_capture_webcams"));
  } catch (e) {
    // The capture works without a webcam, so a failed listing is "none",
    // never a banner over a picker that can still start.
    logWarning(`list_capture_webcams failed: ${String(e)}`);
  }
  // A choice the fresh list no longer offers is dropped rather than sent.
  if (props.webcamId !== null && !webcams.value.some((w) => w.id === props.webcamId)) {
    emit("update:webcamId", null);
  }
});
</script>

<template>
  <div class="flex flex-col gap-1">
    <SectionHeader>Webcam</SectionHeader>
    <p
      v-if="webcams.length === 0"
      data-testid="webcam-none-found"
      class="px-2 text-xs text-fg-subtle"
    >
      No webcam found.
    </p>
    <select
      v-else
      data-testid="webcam-select"
      aria-label="Webcam"
      class="mx-2 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg-secondary focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :value="webcamId ?? ''"
      @change="onChange"
    >
      <option value="">
        No webcam
      </option>
      <option
        v-for="w in webcams"
        :key="w.id"
        :value="w.id"
      >
        {{ w.label }}
      </option>
    </select>
  </div>
</template>
