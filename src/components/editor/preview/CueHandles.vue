<script setup lang="ts">
/**
 * The teaching cues' selection and grab layer (Task 35; F-28, F-31;
 * SCREENS-AND-INTERACTIONS.md: "Arrows expose endpoints; … highlights and
 * spotlights expose bounds; zoom exposes focal point/factor/ramp … Preview
 * handles are never part of the encoded output").
 *
 * **Never part of the picture.** `PreviewSurface.vue` places this NEXT TO
 * its stage, after `LayoutHandles`, never inside the stage: nothing drawn
 * here is output (the `LayoutHandles` rule, pinned by a DOM `contains`
 * test). The root ignores the pointer; only the cue hit shapes and the
 * handles take it — which is how a cue drawn over a FULL-FRAME clip is
 * still reachable: this layer sits above the layout box, and the layout box
 * stands down while a cue is selected (Task 31's carried finding).
 *
 * **Pointer lifecycle** (Task 21's, as in `LayoutHandles`): primary button
 * only; a click on an unselected cue SELECTS it (and its clip, so the
 * inspector shows it); once selected, its body moves it and its handles
 * reshape it. Moving is a PREVIEW — `preview` hands `PreviewSurface` a
 * transient copy of the effect for `CueOverlay` to draw — and release sends
 * exactly ONE `updateEffect` (R14). Escape discards. A cue on a locked
 * track can be selected (to read it) but not dragged.
 *
 * Pointers map through `previewGeometry.clientToCanvas` against this
 * layer's own rect (the same rect as the stage), then through the zoom's
 * inverse, since a zoomed stage shows the cues zoomed too.
 */
import { computed, ref } from "vue";

import { selectedEffectOf } from "../../../editor/cueActions";
import type { CueHandle, CuePatch } from "../../../editor/cueDrag";
import { changedPatch, dragPatch, handleSpots } from "../../../editor/cueDrag";
import type { NormPoint, ZoomTransform } from "../../../editor/cueGeometry";
import { activeCues, svgZoomTransform, unzoomPoint } from "../../../editor/cueGeometry";
import { hitShape } from "../../../editor/cueShapes";
import type { Box, Size } from "../../../editor/previewGeometry";
import { boxStyle, clientToCanvas } from "../../../editor/previewGeometry";
import type { Effect } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const props = defineProps<{
  frame: Box;
  canvas: Size;
  /** Output time, ms. */
  timeMs: number;
  zoom: ZoomTransform;
}>();
const emit = defineEmits<{
  /** A transient copy of the cue being dragged, `null` to stop. */
  (e: "preview", effect: Effect | null): void;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const rootRef = ref<HTMLElement | null>(null);
/** The dragged cue as it would be now; `null` when not dragging. */
const draft = ref<Effect | null>(null);

interface Drag {
  effect: Effect;
  handle: CueHandle;
  start: NormPoint;
  patch: CuePatch;
}
let drag: Drag | null = null;

const active = computed(() => activeCues(editorProject.project, props.timeMs));

const selected = computed(() => {
  const effect = selectedEffectOf(editorProject.project, workspace.selected, workspace.selectionClipIds);
  const cue = effect ? active.value.find((c) => c.effect.id === effect.id) : undefined;
  return cue ? (draft.value?.id === effect?.id ? draft.value : effect) : null;
});

const selectedLocked = computed(() => {
  const effect = selected.value;
  const clip = effect ? editorProject.clipById(effect.clip_id) : null;
  return editorProject.project?.tracks.find((t) => t.id === clip?.track_id)?.locked ?? false;
});

/** Hit shapes for every cue on screen, drawn from the SHOWN copy. */
const hits = computed(() =>
  active.value.map(({ effect }) => {
    const shown = draft.value?.id === effect.id ? draft.value : effect;
    return { effect, shape: hitShape(shown, props.canvas) };
  }),
);

const spots = computed(() =>
  selected.value && !selectedLocked.value ? handleSpots(selected.value, props.canvas) : [],
);

const focals = computed(() =>
  active.value
    .filter((c) => c.effect.kind === "zoom")
    .map((c) => ({ id: c.effect.id, cx: c.effect.x * props.canvas.width, cy: c.effect.y * props.canvas.height })),
);

/** One screen pixel in canvas px, so handles keep their size at any zoom. */
const unit = computed(() =>
  props.frame.width > 0 ? props.canvas.width / props.frame.width / props.zoom.scale : 1,
);

const frameStyle = computed(() => boxStyle(props.frame));
const zoomAttr = computed(() => svgZoomTransform(props.zoom, props.canvas));

/** The selected cue's dashed outline. An arrow's hit shape is a fat line,
 * so it shows its endpoint handles instead of an outline. */
function outline(id: string, tag: string): string {
  return selected.value?.id === id && tag !== "line" ? "#8565c5" : "transparent";
}

/** A pointer as a canvas fraction: letterbox undone, then the zoom. */
function pointAt(event: PointerEvent): NormPoint {
  const rect = rootRef.value?.getBoundingClientRect() ?? { left: 0, top: 0, width: 0, height: 0 };
  const p = clientToCanvas(event, rect, props.canvas);
  return unzoomPoint(props.zoom, { x: p.x / props.canvas.width, y: p.y / props.canvas.height });
}

function stop(): void {
  drag = null;
  draft.value = null;
  emit("preview", null);
}

function begin(event: PointerEvent, effect: Effect, handle: CueHandle): void {
  if (event.button !== 0 || selectedLocked.value) return;
  (event.currentTarget as Element).setPointerCapture?.(event.pointerId);
  rootRef.value?.focus();
  drag = { effect, handle, start: pointAt(event), patch: {} };
}

function onHitDown(event: PointerEvent, effect: Effect): void {
  if (event.button !== 0) return;
  if (selected.value?.id !== effect.id) {
    workspace.select([effect.clip_id]);
    workspace.setSelected({ type: "effect", id: effect.id });
    return;
  }
  begin(event, effect, "move");
}

function update(event: PointerEvent): void {
  const d = drag;
  if (!d) return;
  const p = pointAt(event);
  d.patch = dragPatch(d.effect, d.handle, p.x - d.start.x, p.y - d.start.y);
  draft.value = { ...d.effect, ...d.patch };
  emit("preview", draft.value);
}

async function end(event: PointerEvent): Promise<void> {
  const el = event.currentTarget as Element;
  if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
  const d = drag;
  if (!d) return;
  drag = null;
  const patch = changedPatch(d.effect, d.patch);
  if (!patch) {
    stop();
    return;
  }
  await editorProject.execute({ kind: "updateEffect", effectId: d.effect.id, props: patch });
  // Only now drop the preview: the store holds the committed cue (or the
  // old one, if Rust refused — which is then what shows).
  if (!drag) stop();
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape" || !drag) return;
  event.preventDefault();
  event.stopPropagation();
  stop();
}
</script>

<template>
  <div
    ref="rootRef"
    data-testid="cue-handles"
    tabindex="-1"
    class="pointer-events-none absolute inset-0 overflow-hidden focus:outline-none"
    @keydown="onKeydown"
  >
    <svg
      class="absolute"
      :style="frameStyle"
      :viewBox="`0 0 ${canvas.width} ${canvas.height}`"
      preserveAspectRatio="none"
    >
      <g :transform="zoomAttr">
        <g
          v-for="f in focals"
          :key="`focal-${f.id}`"
          :data-testid="`cue-focal-${f.id}`"
          stroke="#8264bd"
          :stroke-width="1.5 * unit"
        >
          <circle
            :cx="f.cx"
            :cy="f.cy"
            :r="15 * unit"
            fill="#ffffff"
            fill-opacity="0.85"
          />
          <line
            :x1="f.cx - 7 * unit"
            :y1="f.cy"
            :x2="f.cx + 7 * unit"
            :y2="f.cy"
          />
          <line
            :x1="f.cx"
            :y1="f.cy - 7 * unit"
            :x2="f.cx"
            :y2="f.cy + 7 * unit"
          />
        </g>
        <component
          :is="hit.shape.tag"
          v-for="hit in hits"
          :key="hit.effect.id"
          :data-testid="`cue-hit-${hit.effect.id}`"
          v-bind="hit.shape.attrs"
          pointer-events="all"
          fill="transparent"
          :stroke="outline(hit.effect.id, hit.shape.tag)"
          :stroke-dasharray="`${5 * unit} ${3 * unit}`"
          class="cursor-pointer"
          @pointerdown="onHitDown($event, hit.effect)"
          @pointermove="update"
          @pointerup="end"
        />
        <circle
          v-for="spot in spots"
          :key="spot.handle"
          :data-testid="`cue-handle-${spot.handle}`"
          :cx="spot.cx"
          :cy="spot.cy"
          :r="7 * unit"
          pointer-events="all"
          fill="#ffffff"
          stroke="#8062b7"
          :stroke-width="1.5 * unit"
          class="cursor-grab"
          @pointerdown.stop="selected && begin($event, selected, spot.handle)"
          @pointermove.stop="update"
          @pointerup.stop="end"
        />
      </g>
    </svg>
  </div>
</template>
