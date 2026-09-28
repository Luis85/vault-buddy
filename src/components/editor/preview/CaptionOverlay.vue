<script setup lang="ts">
/**
 * Captions in the preview (Task 36; F-35): the cues playing at the preview
 * time (`captionRules.captionRows`, output time), laid over the letterboxed
 * canvas at the project's position, size and background -- so the caption
 * settings' "position" and "size" are something the user can SEE before a
 * render, and "Show captions" off shows none.
 *
 * Sized in canvas terms like the reference compositor (`font_size` px at a
 * 720-line canvas, scaled by the canvas's shorter side, then by how large
 * the canvas is drawn in the stage), and inset 7 % / 6 % from the frame's
 * edges the same way. The burned-in render is Task 43's ASS generator; this
 * is the preview's approximation of it, never an input to it. The layer
 * ignores the pointer.
 */
import { computed } from "vue";

import { captionRows } from "../../../editor/captionRules";
import type { Box } from "../../../editor/previewGeometry";
import { boxStyle } from "../../../editor/previewGeometry";
import type { Project } from "../../../editorTypes";

const props = defineProps<{
  project: Project | null;
  /** Output time, ms. */
  timeMs: number;
  /** The canvas's letterboxed box inside the stage, stage px. */
  frame: Box;
}>();

const settings = computed(() => props.project?.captions ?? null);

const lines = computed(() => {
  if (!settings.value?.enabled) return [];
  return captionRows(props.project)
    .filter((row) => row.startMs <= props.timeMs && props.timeMs < row.endMs)
    .map((row) => ({ id: row.cue.id, text: row.cue.text }));
});

const textStyle = computed(() => {
  const canvas = props.project?.canvas ?? { width: 1280, height: 720 };
  const drawn = props.frame.width / canvas.width;
  const perLine = Math.min(canvas.width, canvas.height) / 720;
  const size = (settings.value?.font_size ?? 30) * perLine * drawn;
  return {
    fontSize: `${size}px`,
    padding: `${size * 0.35}px ${size * 0.6}px`,
    background: settings.value?.background ? "#11111cee" : "transparent",
    textShadow: settings.value?.background ? "none" : "0 0 5px #000",
  };
});
</script>

<template>
  <div
    v-if="lines.length > 0"
    data-testid="caption-overlay"
    :data-position="settings?.position"
    class="pointer-events-none absolute flex flex-col items-center"
    :class="settings?.position === 'top' ? 'justify-start' : 'justify-end'"
    :style="{ ...boxStyle(frame), paddingTop: '6%', paddingBottom: '6%' }"
  >
    <div
      class="max-w-[86%] rounded text-center font-semibold whitespace-pre-line text-white"
      :style="textStyle"
    >
      <p
        v-for="line in lines"
        :key="line.id"
      >
        {{ line.text }}
      </p>
    </div>
  </div>
</template>
