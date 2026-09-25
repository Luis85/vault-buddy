<script setup lang="ts">
/**
 * The preview's picture-in-picture handles (Task 31; F-21, F-23;
 * SCREENS-AND-INTERACTIONS.md: "Drag to move. Drag any corner to resize").
 * The selected visual clip's box, drawn over the letterboxed canvas, with
 * a move surface and eight resize handles.
 *
 * **A sibling overlay, never part of the stage.** `PreviewSurface.vue`
 * places this NEXT TO its `preview-stage` element, not inside it: the stage
 * is where the picture is composed, and nothing drawn here is ever part of
 * the picture (tests pin that with a DOM `contains` check).
 *
 * **Task 21's pointer lifecycle** (`ClipItem.vue`): primary button only,
 * pointer capture so a fast drag past the box keeps tracking, focus on the
 * box so Escape reaches it; the move is a PREVIEW only — emitted as a
 * transient project the surface shows in the picture itself, never
 * written to the store (R14) — and release sends exactly ONE `setLayout`.
 * Escape mid-drag discards the preview and sends nothing. The preview is
 * held until Rust answers, so a commit never flickers back to the old box.
 *
 * Pointers map through `previewGeometry.clientToCanvas` (Task 22), which
 * undoes the letterbox, against this overlay's own rect — the same rect as
 * the stage, since both fill the same wrapper. The keyboard alternative to
 * dragging is the Layout inspector's numeric fields.
 */
import { computed, ref } from "vue";

import { clipSpanOf } from "../../../editor/actionTargets";
import { selectedEffectOf } from "../../../editor/cueActions";
import type { ZoomTransform } from "../../../editor/cueGeometry";
import { IDENTITY_ZOOM, pointerToCanvas, zoomedFrame } from "../../../editor/cueGeometry";
import type { Handle, NormBox } from "../../../editor/layoutGeometry";
import { HANDLES, moveBox, resizeFromHandle, roundBox } from "../../../editor/layoutGeometry";
import type { Box, Size } from "../../../editor/previewGeometry";
import { boxStyle as styleOfBox, clipBox } from "../../../editor/previewGeometry";
import { clipIsActive } from "../../../editor/timeMap";
import type { Clip, Project } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const props = withDefaults(
  defineProps<{
    /** The canvas's letterboxed box inside the stage, in stage pixels. */
    frame: Box;
    /** The output canvas, in output pixels. */
    canvas: Size;
    /** An active zoom cue's stage transform (Task 35 fix round 1): the
     * picture is magnified, so the box is drawn against the zoomed frame
     * and pointers are mapped back through the inverse, or the box sits
     * over the wrong part of the picture and a drag moves the clip
     * factor-times faster than the pointer. */
    zoom?: ZoomTransform;
  }>(),
  { zoom: () => IDENTITY_ZOOM },
);
const emit = defineEmits<{
  /** A transient project to preview while dragging, `null` to stop. */
  (e: "preview", project: Project | null): void;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const rootRef = ref<HTMLElement | null>(null);
const boxRef = ref<HTMLElement | null>(null);
/** The box while a drag is in progress; `null` otherwise. */
const draft = ref<NormBox | null>(null);

interface Drag {
  mode: "move" | Handle;
  clipId: string;
  startX: number;
  startY: number;
  start: NormBox;
}
let drag: Drag | null = null;

/** One selected clip on a visible, unlocked video track — the only thing
 * that has a box to handle — and only while it is ON SCREEN and no teaching
 * cue on it is selected (Task 35, closing Task 31's carried finding: a
 * full-frame clip's box covers the whole canvas, so it swallowed every
 * stage pointerdown, even with the playhead nowhere near the clip). A
 * selected cue owns the pointer through `CueHandles` instead. */
const target = computed<Clip | null>(() => {
  const ids = workspace.selectionClipIds;
  const clip = ids.length === 1 ? editorProject.clipById(ids[0]) : undefined;
  if (!clip || !onEditableVideoTrack(clip)) return null;
  if (!clipIsActive(clipSpanOf(clip), workspace.playheadMs)) return null;
  return selectedEffectOf(editorProject.project, workspace.selected, ids) ? null : clip;
});

function onEditableVideoTrack(clip: Clip): boolean {
  const track = editorProject.project?.tracks.find((t) => t.id === clip.track_id);
  return track !== undefined && track.kind === "video" && track.visible && !track.locked;
}

const box = computed<NormBox | null>(() => {
  const clip = target.value;
  return draft.value ?? (clip ? { x: clip.x, y: clip.y, w: clip.w, h: clip.h } : null);
});

const boxStyle = computed(() => {
  if (!box.value) return {};
  return styleOfBox(clipBox(zoomedFrame(props.frame, props.zoom), box.value));
});

const isCircle = computed(() => target.value?.frame_shape === "circle");

const HANDLE_AT: Record<Handle, [number, number]> = {
  nw: [0, 0], n: [50, 0], ne: [100, 0], e: [100, 50], se: [100, 100], s: [50, 100], sw: [0, 100], w: [0, 50],
};
const CURSOR: Record<Handle, string> = {
  nw: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", sw: "nesw-resize",
  n: "ns-resize", s: "ns-resize", e: "ew-resize", w: "ew-resize",
};
function handleStyle(h: Handle) {
  const [x, y] = HANDLE_AT[h];
  return { left: `${x}%`, top: `${y}%`, cursor: CURSOR[h] };
}

/** A pointer as a fraction of the output canvas, letterbox undone. */
function pointAt(event: PointerEvent): { x: number; y: number } {
  return pointerToCanvas(event, rootRef.value?.getBoundingClientRect(), props.canvas, props.zoom);
}

function withBox(project: Project, clipId: string, b: NormBox): Project {
  return { ...project, clips: project.clips.map((c) => (c.id === clipId ? { ...c, ...b } : c)) };
}

function stop(): void {
  drag = null;
  draft.value = null;
  emit("preview", null);
}

function begin(event: PointerEvent, mode: "move" | Handle): void {
  const clip = target.value;
  if (event.button !== 0 || !clip) return;
  (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  boxRef.value?.focus();
  const p = pointAt(event);
  drag = { mode, clipId: clip.id, startX: p.x, startY: p.y, start: { x: clip.x, y: clip.y, w: clip.w, h: clip.h } };
}

/** A corner keeps the picture's proportions unless Shift frees it; a
 * circle keeps them from every handle, or it would stop being a circle. */
function keepsAspect(mode: Handle, shiftKey: boolean): boolean {
  return isCircle.value || (mode.length === 2 && !shiftKey);
}

function update(event: PointerEvent): void {
  const d = drag;
  const project = editorProject.project;
  if (!d || !project) return;
  const p = pointAt(event);
  const dx = p.x - d.startX;
  const dy = p.y - d.startY;
  draft.value =
    d.mode === "move"
      ? moveBox(d.start, dx, dy)
      : resizeFromHandle(d.start, d.mode, dx, dy, keepsAspect(d.mode, event.shiftKey));
  emit("preview", withBox(project, d.clipId, draft.value));
}

async function end(event: PointerEvent): Promise<void> {
  const el = event.currentTarget as HTMLElement;
  if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
  const d = drag;
  if (!d) return;
  drag = null;
  const next = draft.value ? roundBox(draft.value) : null;
  // Rounded before comparing: a pointer that wandered by less than the
  // committed precision changed nothing.
  if (!next || JSON.stringify(next) === JSON.stringify(roundBox(d.start))) {
    stop();
    return;
  }
  await editorProject.execute({ kind: "setLayout", clipIds: [d.clipId], ...next });
  // Only now drop the preview: the store already holds the committed box
  // (or the old one, if Rust refused — which is then what shows).
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
    data-testid="layout-handles"
    class="pointer-events-none absolute inset-0 overflow-hidden"
  >
    <div
      v-if="target && box"
      ref="boxRef"
      data-testid="layout-box"
      role="group"
      tabindex="0"
      :aria-label="`Position and size of ${target.name}. Drag to move, drag a handle to resize.`"
      class="pointer-events-auto absolute cursor-move border border-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="isCircle ? 'rounded-full' : ''"
      :style="boxStyle"
      @pointerdown="begin($event, 'move')"
      @pointermove="update"
      @pointerup="end"
      @keydown="onKeydown"
    >
      <span
        v-for="h in HANDLES"
        :key="h"
        :data-testid="`layout-handle-${h}`"
        class="vb-handle absolute h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-sm border border-accent bg-fg"
        :style="handleStyle(h)"
        @pointerdown.stop="begin($event, h)"
        @pointermove.stop="update"
        @pointerup.stop="end"
      />
    </div>
  </div>
</template>
