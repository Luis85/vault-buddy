<script setup lang="ts">
/**
 * Export the timeline's captions as SRT or WebVTT (Task 48; F-35): Rust
 * opens its OWN save dialog and writes every caption at the time it PLAYS
 * in the output (`editor_export_subtitles`), never touching the project.
 * The parent says why there is nothing to export (`reason`, R20: a
 * disabled control says why); one export at a time; the file name written
 * — or the refusal — is the status line. A dismissed dialog is not an
 * error and says nothing. Visual-parity Task 10 (concept spec §3.4, design
 * D10): the concept's browser "Download timeline SRT" slot, in native
 * words — "Export timeline SRT" and "Export timeline VTT".
 */
import { ref } from "vue";

import type { SubtitleFormat } from "../../../editorTypes";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import LibraryButton from "./LibraryButton.vue";

const props = defineProps<{ reason: string | null }>();

const project = useEditorProjectStore();
const exporting = ref(false);
const status = ref<{ text: string; alert: boolean } | null>(null);

const FORMATS: { format: SubtitleFormat; label: string }[] = [
  { format: "srt", label: "Export timeline SRT" },
  { format: "vtt", label: "Export timeline VTT" },
];

async function exportAs(format: SubtitleFormat): Promise<void> {
  const sessionId = project.sessionId;
  if (props.reason !== null || exporting.value || !sessionId) return;
  exporting.value = true;
  status.value = null;
  try {
    const name = await project.port.exportSubtitles(sessionId, format);
    if (name) status.value = { text: `Saved the captions to ${name}.`, alert: false };
  } catch (e) {
    status.value = { text: `The captions could not be exported. ${toEditorError(e).message}`, alert: true };
  } finally {
    exporting.value = false;
  }
}
</script>

<template>
  <div class="mt-3 flex flex-col gap-2">
    <LibraryButton
      v-for="item in FORMATS"
      :key="item.format"
      icon="download"
      :data-testid="`caption-export-${item.format}`"
      :disabled="reason !== null || exporting"
      :title="reason ?? `Export every caption as ${item.format.toUpperCase()}, in output time`"
      @click="exportAs(item.format)"
    >
      {{ item.label }}
    </LibraryButton>
    <p
      v-if="status"
      data-testid="caption-export-status"
      :role="status.alert ? 'alert' : 'status'"
      class="text-[11px]"
      :class="status.alert ? 'text-danger-fg' : 'text-fg-secondary'"
    >
      {{ status.text }}
    </p>
  </div>
</template>
