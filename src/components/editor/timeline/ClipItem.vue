<script setup lang="ts">
/**
 * One clip's visual (Task 20; F-04, F-14, F-26;
 * SCREENS-AND-INTERACTIONS.md §03: "Clips show media type, selected state,
 * trims and gold fade controls"). Presentational and absolutely positioned
 * by its caller (`TrackLane.vue` supplies `leftPx`/`widthPx` in CONTENT
 * space, `timelineLayout.ts`'s own convention) — this component never reads
 * zoom or scroll itself.
 *
 * Selection is a direct store write (`editorWorkspace.select`, the
 * `PreviewToolbar`/`InspectorPanel` precedent of components calling a store
 * directly rather than emitting purely upward) — the only thing this
 * component bubbles UP is "open a context menu here", because the ONE
 * `ContextMenu` instance lives at `TimelineView.vue`, several components
 * away. Right-click and Shift+F10/Menu (SCREENS-AND-INTERACTIONS.md §03:
 * "The same actions are reachable from … Shift+F10") funnel into the SAME
 * `context-menu` emit shape so `TimelineView` has one handler, not two.
 *
 * Drag/trim (Task 21; F-07..F-11): the clip BODY drags to move it, and the
 * two handles trim it — each is a self-contained pointer-capture lifecycle
 * (`TimelineRuler.vue`'s own `setPointerCapture`/`releasePointerCapture`
 * pattern, mirrored here so a fast drag past this clip's own edge keeps
 * tracking instead of going silent), delegating every bit of the actual
 * interaction MATH to `useTimelineDrag` — this component only owns the DOM
 * plumbing (capture, `clientX`/`clientY`, focus) the composable's own doc
 * says stays here. Pointer-up sends exactly ONE `moveClips`/`trimClip`;
 * Escape mid-drag discards the preview with nothing sent. ←/→ (optionally
 * with Shift for the 1s step) nudge the FOCUSED clip by one `moveClips`
 * per keypress and restore focus to it after the projection re-renders —
 * `nextTick` plus a re-query by the clip's own STABLE `data-testid` key,
 * because the async round trip to Rust can, in principle, let Vue swap the
 * underlying DOM node out from under a captured element reference.
 *
 * `role="option"`, not `role="button"` (fix round 1, finding 3): `aria-
 * selected` is only a valid ARIA attribute on a handful of roles (option,
 * row, tab, gridcell, …) and `button` is not one of them —
 * `SearchHitRow.vue`'s own `role="option"` + `:aria-selected` is this
 * repo's existing precedent for exactly this "one of several selectable
 * items" shape. Because this element is a `<div>`, not a native `<button>`,
 * Enter/Space activation has to be wired by hand
 * (`TranscriptionSummary.vue`'s `@keydown.enter`/`@keydown.space.prevent`
 * precedent, folded into the same `onKeydown` this component already uses
 * for Shift+F10/Menu) — without it a keyboard user who tabs to a clip can
 * open its context menu but has no way to select it.
 */
import { computed, nextTick, ref } from "vue";

import { useTimelineDrag } from "../../../composables/useTimelineDrag";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { msToX, snapTargets as computeSnapTargets } from "../../../editor/timelineLayout";
import { clipOutputEnd } from "../../../editor/timeMap";
import type { Clip, ClipSpan } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";

const props = defineProps<{
  clip: Clip;
  assetKind: "video" | "audio";
  selected: boolean;
  leftPx: number;
  widthPx: number;
  zoom: number;
  trackIndex: number;
  trackOrder: string[];
}>();
const emit = defineEmits<{
  (e: "context-menu", payload: { clip: Clip; clientX: number; clientY: number }): void;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

function spanOf(c: Clip): ClipSpan {
  return { start_ms: c.start_ms, in_ms: c.in_ms, out_ms: c.out_ms, speed: c.speed ?? 1 };
}

function endMs(c: Clip): number {
  return clipOutputEnd(spanOf(c));
}

function label(c: Clip): string {
  return `Clip ${c.name}, ${formatDuration(c.start_ms)}–${formatDuration(endMs(c))}`;
}

/** Plain click selects only this clip; Ctrl/Cmd+click toggles it in or out
 * of the selection and Shift+click adds it (the brief's "Click selects
 * (Ctrl/Shift extends)"). Enter/Space call this with no event: plain select.
 * The click that ENDS a drag is swallowed (`suppressNextClick`) — otherwise
 * it would collapse the multi-clip selection the user just dragged as a
 * group down to this one clip. */
function onSelect(event?: MouseEvent) {
  if (suppressNextClick) {
    suppressNextClick = false;
    return;
  }
  const id = props.clip.id;
  const sel = workspace.selectionClipIds;
  if (event?.ctrlKey || event?.metaKey) {
    workspace.select(sel.includes(id) ? sel.filter((c) => c !== id) : [...sel, id]);
  } else {
    workspace.select(event?.shiftKey ? [...sel, id] : [id]);
  }
}

function onContextMenu(event: MouseEvent) {
  emit("context-menu", { clip: props.clip, clientX: event.clientX, clientY: event.clientY });
}

// ---- drag / trim / nudge (Task 21) -----------------------------------------

/** The clips a body drag or nudge moves together with THIS one — the whole
 * selection when this clip is part of a multi-selection, else just itself
 * (`actions.ts`'s `targetClipIds` rule, applied to a direct manipulation
 * gesture rather than a pointer/menu target). */
function moveTargetClipIds(): string[] {
  const sel = workspace.selectionClipIds;
  return sel.includes(props.clip.id) && sel.length > 1 ? sel : [props.clip.id];
}

/** Snap targets WITHOUT the clips being moved: their own edges travel with
 * the drag, so as targets they would pull every small drag (inside the
 * snap threshold) straight back onto where it started — a silent no-op. */
function snapTargetsExcludingMoved(): number[] {
  const p = editorProject.project;
  const moving = moveTargetClipIds();
  const others = p ? { ...p, clips: p.clips.filter((c) => !moving.includes(c.id)) } : null;
  return computeSnapTargets(others, workspace.playheadMs);
}

const drag = useTimelineDrag({
  clip: () => props.clip,
  zoom: () => props.zoom,
  snapEnabled: () => workspace.snap,
  snapTargets: snapTargetsExcludingMoved,
  moveTargetClipIds,
  trackOrder: () => props.trackOrder,
  trackIndex: () => props.trackIndex,
  trackAccepts: (trackId) => {
    const track = editorProject.trackById(trackId);
    return track !== undefined && !track.locked && track.kind === props.assetKind;
  },
  execute: (command) => editorProject.execute(command),
});

const root = ref<HTMLElement | null>(null);
/** Pointer travel (px) below which a press-and-release is still a click. */
const DRAG_SLOP_PX = 3;
let press: { x: number; y: number } | null = null;
let suppressNextClick = false;

/** Shared pointerdown plumbing for the body and both handles: primary
 * button only (a right-press is the context menu's, never a drag), pointer
 * capture so a fast drag past the clip's edge keeps tracking, and focus on
 * the clip so Escape mid-drag reaches `onKeydown`. */
function beginPress(event: PointerEvent): boolean {
  if (event.button !== 0) return false;
  (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  root.value?.focus({ preventScroll: true });
  press = { x: event.clientX, y: event.clientY };
  suppressNextClick = false;
  return true;
}
function endPress(event: PointerEvent): void {
  const el = event.currentTarget as HTMLElement;
  if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
  if (press && Math.hypot(event.clientX - press.x, event.clientY - press.y) > DRAG_SLOP_PX) {
    suppressNextClick = true;
  }
  press = null;
}

function onBodyPointerDown(event: PointerEvent) {
  if (beginPress(event)) drag.beginBodyDrag(event.clientX, event.clientY);
}
function onBodyPointerMove(event: PointerEvent) {
  drag.updateBodyDrag(event.clientX);
}
async function onBodyPointerUp(event: PointerEvent) {
  endPress(event);
  await drag.endBodyDrag(event.clientY);
}

function onTrimPointerDown(event: PointerEvent, edge: "start" | "end") {
  if (beginPress(event)) drag.beginTrim(edge, event.clientX);
}
function onTrimPointerMove(event: PointerEvent) {
  drag.updateTrim(event.clientX);
}
async function onTrimPointerUp(event: PointerEvent) {
  endPress(event);
  await drag.endTrim();
}

/** The rendered left position: the committed `leftPx` prop, shifted by
 * whichever preview is currently live (a trim overrides it outright, since
 * trimming can move the START edge itself; a body drag adds its own delta
 * on top). Neither preview mutates `editorProject.project` (R14) — this is
 * purely a local render-time override. */
const previewLeftPx = computed(() => {
  if (drag.trimPreview.value) return msToX(drag.trimPreview.value.startMs, props.zoom);
  if (drag.movePreview.value) return props.leftPx + msToX(drag.movePreview.value.deltaMs, props.zoom);
  return props.leftPx;
});
const previewWidthPx = computed(() => {
  const tp = drag.trimPreview.value;
  if (!tp) return props.widthPx;
  const start = msToX(tp.startMs, props.zoom);
  const end = msToX(
    clipOutputEnd({ start_ms: tp.startMs, in_ms: tp.inMs, out_ms: tp.outMs, speed: props.clip.speed ?? 1 }),
    props.zoom,
  );
  return end - start;
});

const NUDGE_FRAME_MS = 33;
const NUDGE_SECOND_MS = 1_000;

async function onNudge(event: KeyboardEvent) {
  event.preventDefault();
  const amount = event.shiftKey ? NUDGE_SECOND_MS : NUDGE_FRAME_MS;
  const deltaMs = event.key === "ArrowLeft" ? -amount : amount;
  const el = event.currentTarget as HTMLElement;
  await drag.nudge(deltaMs);
  // Stable key (`TrackLane.vue`'s `v-for` keys by `clip.id`, unchanged by a
  // move) usually keeps `el` itself the live element across the re-render —
  // the re-query below is the belt for the case it does not (e.g. the clip
  // briefly leaves `visibleClips`' virtualization window mid-round-trip).
  await nextTick();
  const restored =
    document.querySelector<HTMLElement>(`[data-testid="clip-${props.clip.id}"]`) ?? el;
  restored?.focus();
}

function onKeydown(event: KeyboardEvent) {
  if (isContextMenuShortcut(event)) {
    event.preventDefault();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    emit("context-menu", { clip: props.clip, clientX: rect.left, clientY: rect.bottom });
    return;
  }
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    onSelect();
    return;
  }
  if (event.key === "Escape" && (drag.movePreview.value || drag.trimPreview.value)) {
    drag.cancelBodyDrag();
    drag.cancelTrim();
    return;
  }
  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    void onNudge(event);
  }
}
</script>

<template>
  <div
    ref="root"
    :data-testid="`clip-${clip.id}`"
    role="option"
    tabindex="0"
    :aria-selected="selected"
    :aria-label="label(clip)"
    class="absolute top-1 bottom-1 flex items-center overflow-hidden rounded border px-1 text-micro cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    :class="[
      assetKind === 'audio' ? 'bg-audio-bg border-audio text-audio' : 'bg-video-bg border-video text-video',
      selected ? 'ring-2 ring-accent' : '',
    ]"
    :style="{ left: `${previewLeftPx}px`, width: `${Math.max(previewWidthPx, 2)}px` }"
    @click="onSelect"
    @contextmenu.prevent="onContextMenu"
    @keydown="onKeydown"
    @pointerdown="onBodyPointerDown"
    @pointermove="onBodyPointerMove"
    @pointerup="onBodyPointerUp"
  >
    <div
      v-if="clip.fade_in_ms > 0"
      :data-testid="`clip-${clip.id}-fade-in`"
      class="pointer-events-none absolute inset-y-0 left-0 w-3 bg-gold/40"
      style="clip-path: polygon(0 100%, 100% 100%, 0 0)"
    />
    <div
      v-if="clip.fade_out_ms > 0"
      :data-testid="`clip-${clip.id}-fade-out`"
      class="pointer-events-none absolute inset-y-0 right-0 w-3 bg-gold/40"
      style="clip-path: polygon(0 100%, 100% 100%, 100% 0)"
    />
    <!-- F-26: a reserved lane slot for a future waveform, not the waveform
         itself -- peaks rendering needs a Rust-side job this task does not
         add (the brief's own "(lane slot)" scoping). -->
    <div
      v-if="assetKind === 'audio'"
      :data-testid="`clip-${clip.id}-waveform-slot`"
      class="pointer-events-none absolute inset-x-1 bottom-0.5 h-3 rounded bg-audio-bg/60"
    />

    <span class="pointer-events-none relative z-10 truncate">{{ clip.name }}</span>

    <span
      :data-testid="`clip-${clip.id}-trim-start`"
      class="absolute inset-y-0 left-0 w-1 cursor-ew-resize bg-white/10"
      @pointerdown.stop="onTrimPointerDown($event, 'start')"
      @pointermove.stop="onTrimPointerMove"
      @pointerup.stop="onTrimPointerUp"
    />
    <span
      :data-testid="`clip-${clip.id}-trim-end`"
      class="absolute inset-y-0 right-0 w-1 cursor-ew-resize bg-white/10"
      @pointerdown.stop="onTrimPointerDown($event, 'end')"
      @pointermove.stop="onTrimPointerMove"
      @pointerup.stop="onTrimPointerUp"
    />
  </div>
</template>
