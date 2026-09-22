<script setup lang="ts">
/**
 * The timeline's time ruler (Task 20; F-14). Click/drag seeks the playhead
 * — and ONLY the playhead: it never touches `editorWorkspace.selection*`,
 * this task's own named test ("ruler click moves the playhead, not the
 * selection"). Ticks use `timelineLayout.tickIntervalMs` so their spacing
 * stays legible at every zoom rather than a fixed, arbitrary step.
 *
 * Drag (fix round 1, finding 2): `pointerdown` seeks once and arms
 * `dragging`; `pointermove` re-seeks only while `dragging` is true, so an
 * ordinary hover (no button ever pressed) never moves the playhead — the
 * `CompanionCharacter.vue`/`useTaskReorder.ts` precedent for this repo's
 * `setPointerCapture`/`?.()` idiom. Capturing the pointer on `pointerdown`
 * (rather than window-level listeners, `TimelineView`'s own resize-handle
 * pattern) keeps drag and click on the exact same `localX -> ms` path with
 * no second implementation to drift from: capture redirects `pointermove`/
 * `pointerup` to THIS element even once the cursor leaves the ruler's own
 * bounds, so a fast drag past either edge keeps seeking instead of going
 * silent.
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

/** The one `localX -> ms` path both a click and every drag step share. */
function msFromClientX(el: HTMLElement, clientX: number): number {
  const rect = el.getBoundingClientRect();
  const localX = clientX - rect.left;
  const ppm = pxPerMs(props.zoom);
  const ms = ppm > 0 ? localX / ppm : 0;
  return Math.max(0, ms);
}

/** Not a `ref` -- nothing in the template reads it, so making it reactive
 * would only cost an extra Vue dependency-tracking wrapper for no
 * observable effect (the same reasoning `CompanionCharacter.vue`'s
 * `pressedAt`/`dragged` module-scope variables already use). */
let dragging = false;

function onPointerDown(event: PointerEvent) {
  const el = event.currentTarget as HTMLElement;
  dragging = true;
  el.setPointerCapture?.(event.pointerId);
  workspace.setPlayhead(msFromClientX(el, event.clientX));
}

function onPointerMove(event: PointerEvent) {
  if (!dragging) return;
  const el = event.currentTarget as HTMLElement;
  workspace.setPlayhead(msFromClientX(el, event.clientX));
}

function onPointerUp(event: PointerEvent) {
  dragging = false;
  const el = event.currentTarget as HTMLElement;
  if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
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
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
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
