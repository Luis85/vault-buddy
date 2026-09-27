<script setup lang="ts">
/**
 * The Render dialog's output profile (visual-parity Task 21; concept spec
 * §9.6 `.render-profile`): the concept's card, with its browser-only label
 * and limits replaced by the native truth (design D10) — the canvas size
 * and frame rate the render uses, an MP4 made by ffmpeg on this PC, the
 * render's length (the review range's, when one is ticked — fix round 1)
 * — and the native quality radios in the card, since
 * quality is the one output choice a native render offers.
 */
import { computed } from "vue";

import { rangeTimeLabel } from "../../../editor/renderRanges";
import type { RenderQuality } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{ canvas: { width: number; height: number; fps: number }; durationMs: number }>();
const quality = defineModel<RenderQuality>("quality", { required: true });

const QUALITIES: { id: RenderQuality; label: string; hint: string }[] = [
  { id: "low", label: "Low", hint: "Smallest file, fastest render." },
  { id: "balanced", label: "Balanced", hint: "Good quality at a moderate size." },
  { id: "high", label: "High", hint: "Best quality, largest file." },
];

const size = computed(() => `${props.canvas.width} × ${props.canvas.height} · ${props.canvas.fps} fps · MP4`);
</script>

<template>
  <section
    data-testid="render-dialog-profile"
    class="flex flex-col gap-2 rounded-[9px] border border-line p-4"
  >
    <span class="flex items-center gap-2 text-xs text-fg">
      <EditorIcon
        name="video"
        :size="16"
      />
      Output
    </span>
    <b class="text-[13px] font-semibold text-fg">{{ size }}</b>
    <small
      data-testid="render-dialog-profile-length"
      class="text-[11px] text-fg-muted"
    >{{ rangeTimeLabel(durationMs) }} · rendered with ffmpeg on this PC</small>
    <fieldset class="mt-1 grid grid-cols-3 gap-2 max-[560px]:grid-cols-1">
      <legend class="sr-only">
        Quality
      </legend>
      <label
        v-for="q in QUALITIES"
        :key="q.id"
        class="flex cursor-pointer items-start gap-2 rounded-[7px] border p-2 text-xs text-fg"
        :class="quality === q.id ? 'border-accent bg-accent-bg' : 'border-line'"
      >
        <input
          v-model="quality"
          type="radio"
          name="render-quality"
          :value="q.id"
          :data-testid="`render-dialog-quality-${q.id}`"
          class="mt-px shrink-0"
        >
        <span class="flex flex-col gap-0.5">
          {{ q.label }}
          <small class="text-[10px] text-fg-muted">{{ q.hint }}</small>
        </span>
      </label>
    </fieldset>
  </section>
</template>
