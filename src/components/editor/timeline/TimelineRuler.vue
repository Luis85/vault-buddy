<script setup lang="ts">
/**
 * The timeline's header row (Task 20; visual-parity Task 16, concept spec
 * §6.3): 32px, sticky at the top of the one scroll container. Its label
 * cell is sticky at the left too (design D12) and holds **Add track** (the
 * registry's add-track menu) and the "Layers ↓" hint; the ruler beside it
 * carries the ticks and the chapter markers.
 *
 * **Ticks** are `rulerTicks.ts`' (the first step whose spacing reaches
 * 70px, `MM:SS` or `MM:SS.d` labels), 9px mono with a 7px line below.
 *
 * **Seeking.** The ruler is the playhead's slider (`role="slider"`): a
 * press seeks and a drag keeps seeking — and ONLY the playhead: it never
 * touches the selection. Capturing the pointer on `pointerdown` keeps a
 * drag past either edge on the same `localX -> ms` path as the click. From
 * the keyboard, ←/↓ and →/↑ step one frame of the project's own frame
 * rate (whole ms) and Page Up/Down one second; Home/End reach the editor's
 * own Go to start / Go to end.
 *
 * **Chapter markers** are gold ◆ buttons with a 24px hit area, centred on
 * their OUTPUT time (`captionRules.chapterRows`: a marker is stored in its
 * clip's source time), named "Go to <title>"; a click seeks there. They
 * are the slider's SIBLINGS, laid over it in one positioned layer — never
 * its children, which ARIA makes presentational (a marker inside would
 * lose its role and name). Their `pointerdown` still stops at them, so a
 * press can never reach the ruler and seek to the pointer instead.
 */
import { computed, ref } from "vue";

import { chapterRows } from "../../../editor/captionRules";
import { rulerTicks } from "../../../editor/rulerTicks";
import { msToX, pxPerMs } from "../../../editor/timelineLayout";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatMenuTime } from "../menus/menuModel";
import AddTrackButton from "./AddTrackButton.vue";

const props = defineProps<{
  zoom: number;
  widthPx: number;
  /** The label column at this window width (`editorWorkspace.trackLabelWidth`). */
  labelWidth: number;
}>();

const workspace = useEditorWorkspaceStore();
const editorProject = useEditorProjectStore();
const addTrackOpen = ref(false);

const ticks = computed(() => rulerTicks(pxPerMs(props.zoom) * 1000, props.widthPx));
const markers = computed(() =>
  chapterRows(editorProject.project).map((row) => ({
    id: row.marker.id,
    title: row.marker.title,
    ms: row.outputMs,
    x: msToX(row.outputMs, props.zoom),
  })),
);

/** Whole milliseconds: the playhead becomes a command's time (Split at the
 * playhead), and Rust takes every time as a `u64`. */
function msFromClientX(el: HTMLElement, clientX: number): number {
  const ppm = pxPerMs(props.zoom);
  const localX = clientX - el.getBoundingClientRect().left;
  return Math.max(0, Math.round(ppm > 0 ? localX / ppm : 0));
}

/** Not a `ref`: nothing in the template reads it. */
let dragging = false;

function onPointerDown(event: PointerEvent) {
  const el = event.currentTarget as HTMLElement;
  dragging = true;
  el.setPointerCapture?.(event.pointerId);
  workspace.setPlayhead(msFromClientX(el, event.clientX));
}

function onPointerMove(event: PointerEvent) {
  if (!dragging) return;
  workspace.setPlayhead(msFromClientX(event.currentTarget as HTMLElement, event.clientX));
}

function onPointerUp(event: PointerEvent) {
  dragging = false;
  const el = event.currentTarget as HTMLElement;
  if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
}

/** One frame at the project's own rate, in whole ms (30 fps: 33; 60: 17). */
const frameMs = computed(() => Math.round(1_000 / (editorProject.project?.canvas.fps || 30)));
/** Each key's step, in frames (±1) or whole seconds (±1 s). */
const KEY_STEPS: Record<string, { frames?: number; ms?: number }> = {
  ArrowLeft: { frames: -1 },
  ArrowDown: { frames: -1 },
  ArrowRight: { frames: 1 },
  ArrowUp: { frames: 1 },
  PageDown: { ms: -1_000 },
  PageUp: { ms: 1_000 },
};

function onKeydown(event: KeyboardEvent) {
  // Only the slider's own keys: never one that bubbled up to it.
  if (event.target !== event.currentTarget) return;
  const key = KEY_STEPS[event.key];
  const step = key && (key.ms ?? (key.frames as number) * frameMs.value);
  if (step === undefined || event.ctrlKey || event.altKey || event.metaKey) return;
  event.preventDefault();
  workspace.setPlayhead(Math.max(0, workspace.playheadMs + step));
}
</script>

<template>
  <div
    data-testid="timeline-ruler"
    class="sticky top-0 flex h-8 w-max min-w-full border-b border-line bg-panel"
    :class="addTrackOpen ? 'z-40' : 'z-20'"
  >
    <div
      data-testid="timeline-ruler-label"
      class="sticky left-0 z-[3] flex shrink-0 items-center justify-between border-r border-line bg-panel px-3"
      :style="{ width: `${labelWidth}px` }"
    >
      <AddTrackButton v-model:open="addTrackOpen" />
      <span
        class="text-[10px] text-fg-muted"
        title="Tracks higher up sit in front of the ones below"
      >Layers ↓</span>
    </div>
    <div
      class="relative h-full shrink-0"
      :style="{ width: `${widthPx}px` }"
    >
      <div
        data-testid="timeline-ruler-ticks"
        role="slider"
        tabindex="0"
        aria-label="Timeline playhead"
        aria-valuemin="0"
        :aria-valuemax="editorProject.durationMs / 1000"
        :aria-valuenow="(workspace.playheadMs / 1000).toFixed(2)"
        :aria-valuetext="formatMenuTime(workspace.playheadMs)"
        class="relative h-full cursor-ew-resize touch-none select-none focus:outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @keydown="onKeydown"
      >
        <span
          v-for="t in ticks"
          :key="t.ms"
          data-testid="timeline-ruler-tick"
          class="pointer-events-none absolute top-[3px] font-mono text-[9px] text-fg-muted after:mt-[5px] after:block after:h-[7px] after:border-l after:border-line after:content-['']"
          :style="{ left: `${t.x}px` }"
        >{{ t.label }}</span>
      </div>
      <button
        v-for="m in markers"
        :key="m.id"
        type="button"
        :data-testid="`timeline-marker-${m.id}`"
        :title="m.title"
        :aria-label="`Go to ${m.title}`"
        class="absolute top-3 z-[2] flex h-6 min-h-6 w-6 min-w-6 -translate-x-1/2 items-center justify-center p-0 text-[11px] leading-6 text-gold focus-visible:bg-panel"
        :style="{ left: `${m.x}px` }"
        @pointerdown.stop
        @click="workspace.setPlayhead(m.ms)"
      >
        ◆
      </button>
    </div>
  </div>
</template>
