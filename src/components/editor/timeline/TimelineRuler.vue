<script setup lang="ts">
/**
 * The timeline's time ruler (Task 20; F-14). Click/drag seeks the playhead
 * — and ONLY the playhead: it never touches `editorWorkspace.selection*`,
 * this task's own named test ("ruler click moves the playhead, not the
 * selection"). Ticks use `timelineLayout.tickIntervalMs` so their spacing
 * stays legible at every zoom rather than a fixed, arbitrary step.
 *
 * Row shape mirrors `TrackLane.vue`'s label-column-then-content-track split
 * (`TRACK_LABEL_WIDTH_PX`) so its ticks line up visually with the clips
 * below — see that component's own doc for why the label column is not
 * pinned via `position: sticky` this task.
 */
import { computed } from "vue";

import { msToX, pxPerMs, tickIntervalMs, TRACK_LABEL_WIDTH_PX, xToMs } from "../../../editor/timelineLayout";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";

const props = defineProps<{
  zoom: number;
  widthPx: number;
}>();

const workspace = useEditorWorkspaceStore();

/** A hard ceiling on how many ticks one ruler ever renders — a pathological
 * `zoom`/`widthPx` combination (e.g. a huge content width at a low zoom)
 * must degrade to sparse ticks, never a runaway loop. */
const MAX_TICKS = 2000;

const ticks = computed(() => {
  const interval = tickIntervalMs(props.zoom);
  const rangeMs = xToMs(props.widthPx, props.zoom);
  const count = Math.min(MAX_TICKS, Math.floor(rangeMs / interval) + 1);
  const out: { ms: number; x: number }[] = [];
  for (let i = 0; i < count; i += 1) {
    const ms = i * interval;
    out.push({ ms, x: msToX(ms, props.zoom) });
  }
  return out;
});

function onPointerDown(event: PointerEvent) {
  const el = event.currentTarget as HTMLElement;
  const rect = el.getBoundingClientRect();
  const localX = event.clientX - rect.left;
  const ppm = pxPerMs(props.zoom);
  const ms = ppm > 0 ? localX / ppm : 0;
  workspace.setPlayhead(Math.max(0, ms));
}
</script>

<template>
  <div
    data-testid="timeline-ruler"
    class="sticky top-0 z-20 flex bg-panel"
  >
    <div
      class="shrink-0 border-r border-line"
      :style="{ width: `${TRACK_LABEL_WIDTH_PX}px` }"
    />
    <div
      data-testid="timeline-ruler-ticks"
      class="relative h-6 cursor-pointer select-none"
      :style="{ width: `${widthPx}px` }"
      @pointerdown="onPointerDown"
    >
      <div
        v-for="t in ticks"
        :key="t.ms"
        class="pointer-events-none absolute top-0 h-full border-l border-line text-micro text-fg-subtle"
        :style="{ left: `${t.x}px` }"
      >
        <span class="ml-0.5">{{ formatDuration(t.ms) }}</span>
      </div>
    </div>
  </div>
</template>
