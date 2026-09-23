<script setup lang="ts">
/**
 * A clip's volume in the Audio inspector (Task 27, split out of
 * `AudioSection` for the template-complexity ratchet). Stored LINEAR
 * (`Clip.volume`, `[0,2]` — Rust refuses anything else), read out in dB.
 * The slider and the text field share ONE `useInspectorDraft` (Task 21's
 * mechanism): a drag only changes the draft (and the dB readout follows
 * it), and the release — or Enter/blur in the text field — sends exactly
 * one `setClipMix`. A value Rust refuses falls back to the committed one.
 */
import { computed } from "vue";

import { numberField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { formatDb } from "../../../editor/mixRules";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{
  clipId: string;
  /** Why the clip cannot be changed (its track is locked), or `null`. */
  lockReason: string | null;
}>();

/** Mirrors `core::editor::commands::mix::CLIP_VOLUME_MAX`. */
const VOLUME_MAX = 2;

const editorProject = useEditorProjectStore();

const volume = useInspectorDraft(
  numberField({
    value: () => editorProject.clipById(props.clipId)?.volume ?? 1,
    label: "Volume",
    min: 0,
    max: VOLUME_MAX,
    rangeLabel: "0 and 2",
  }),
  (v: number) => editorProject.execute({ kind: "setClipMix", clipIds: [props.clipId], volume: v }),
);
const { draft, error } = volume;

const dbReadout = computed(() => {
  const n = Number(draft.value);
  return draft.value.trim() !== "" && Number.isFinite(n) ? formatDb(n) : "—";
});

function onInput(event: Event): void {
  draft.value = (event.target as HTMLInputElement).value;
}
</script>

<template>
  <label class="flex flex-col gap-0.5">
    <span class="flex justify-between text-fg-subtle">
      Volume
      <span
        data-testid="audio-section-db"
        class="tabular-nums text-fg-muted"
      >{{ dbReadout }}</span>
    </span>
    <input
      data-testid="audio-section-volume-slider"
      type="range"
      min="0"
      :max="VOLUME_MAX"
      step="0.01"
      class="accent-violet-500"
      aria-label="Clip volume"
      :disabled="lockReason !== null"
      :title="lockReason ?? undefined"
      :value="draft"
      @input="onInput"
      @change="volume.submit()"
    >
    <input
      data-testid="audio-section-volume"
      type="text"
      class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
      aria-label="Clip volume (0 to 2, 1 is unchanged)"
      :disabled="lockReason !== null"
      :value="draft"
      @input="onInput"
      @keydown.enter="volume.submit()"
      @keydown.escape="volume.revert()"
      @blur="volume.submit()"
    >
    <span
      v-if="error"
      data-testid="audio-section-volume-error"
      class="text-danger"
    >{{ error }}</span>
  </label>
</template>
