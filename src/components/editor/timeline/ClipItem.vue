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
 * The two GOLD FADE HANDLES (Task 29; F-17, F-18) follow the exact same
 * pointer-capture lifecycle as the trim handles above, never a second drag
 * model: preview during the drag (`drag.fadePreview`), exactly ONE
 * `setFades` on release, Escape discards. They are always rendered (a fade
 * has to be CREATED from zero, so the grab target must exist before there
 * is anything to see) while the gold WEDGE beside each one — the visual
 * indicator of how much fade there is — stays gated on a nonzero value, the
 * behaviour `editorTimelineView.test.ts`'s own "renders a fade wedge only
 * for a nonzero fade" test already pins.
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
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useTimelineDrag } from "../../../composables/useTimelineDrag";
import { hasPreviewSource } from "../../../editor/previewLayers";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { msToX, snapTargets as computeSnapTargets } from "../../../editor/timelineLayout";
import { clipOutputEnd } from "../../../editor/timeMap";
import { trackAccepts } from "../../../editor/trackCompat";
import type { Clip, ClipSpan } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";
import ClipThumbnail from "./ClipThumbnail.vue";
import ClipWaveform from "./ClipWaveform.vue";

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
  trackAccepts: (trackId) => trackAccepts(editorProject.trackById(trackId), props.assetKind),
  execute: (command) => editorProject.execute(command),
});

const root = ref<HTMLElement | null>(null);
/** The guide's `clip.selected` (Task 55): this clip, while it is selected. */
const selectedTarget = useGuideTarget("clip.selected", { active: () => props.selected });
function bindRoot(el: Element | ComponentPublicInstance | null): void {
  root.value = el as HTMLElement | null;
  selectedTarget(el);
}
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

// ---- fade handles (Task 29; F-17, F-18) ------------------------------------
// The trim handles' own pointer-capture lifecycle, reused rather than a
// second drag model (the brief's own instruction): `beginPress` captures
// the pointer and focuses the clip so Escape mid-drag reaches `onKeydown`,
// exactly as the body/trim handlers above already do.

function onFadePointerDown(event: PointerEvent, edge: "in" | "out") {
  if (beginPress(event)) drag.beginFade(edge, event.clientX);
}
function onFadePointerMove(event: PointerEvent) {
  drag.updateFade(event.clientX);
}
async function onFadePointerUp(event: PointerEvent) {
  endPress(event);
  await drag.endFade();
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

// ---- fade wedges (Task 29) --------------------------------------------------
// The gold wedge's WIDTH is proportional to the fade duration it shows
// (`msToX`, the same output-time-to-px scale `previewWidthPx` above uses),
// clamped to at most half the clip's own current width so two wedges can
// never overlap, and floored at a small minimum so even a very short fade
// stays grabbable/visible. Driven by the LIVE drag preview (falling back to
// the committed clip value) so dragging a fade handle from zero grows the
// wedge as the user watches, exactly like `previewLeftPx`/`previewWidthPx`
// do for a body/trim drag.
const MIN_FADE_WEDGE_PX = 6;

const previewFadeInMs = computed(() =>
  drag.fadePreview.value?.edge === "in" ? drag.fadePreview.value.ms : props.clip.fade_in_ms,
);
const previewFadeOutMs = computed(() =>
  drag.fadePreview.value?.edge === "out" ? drag.fadePreview.value.ms : props.clip.fade_out_ms,
);
function fadeWedgeWidthPx(ms: number): number {
  return Math.min(previewWidthPx.value / 2, Math.max(msToX(ms, props.zoom), MIN_FADE_WEDGE_PX));
}

// ---- derived media (Task 28) -----------------------------------------------

/** The asset this clip plays — `null` for a dangling reference, which then
 * draws neither a waveform nor a thumbnail. */
const asset = computed(() => editorProject.project?.assets.find((a) => a.id === props.clip.asset_id) ?? null);
/** The source range on screen: a live trim previews its own in/out so the
 * waveform follows the handle instead of stretching the committed range. */
const shownInMs = computed(() => drag.trimPreview.value?.inMs ?? props.clip.in_ms);
const shownOutMs = computed(() => drag.trimPreview.value?.outMs ?? props.clip.out_ms);
/** Narrower than this, a poster frame is noise over the clip's name. */
const THUMBNAIL_MIN_WIDTH_PX = 48;
/** An audio clip's asset, for its waveform lane (`null`: no lane) — only
 * when it has a real file to decode: a synthesized builtin would be refused
 * by Rust on every mount (fix round 1, review Minor 7). */
const waveformAsset = computed(() => {
  const a = asset.value;
  return a && props.assetKind === "audio" && hasPreviewSource(a) ? a : null;
});
/** A video clip's asset when it has a real file to cut a poster frame
 * from and the clip is wide enough to show one (`null`: no poster). */
const posterAsset = computed(() => {
  const a = asset.value;
  const fits = previewWidthPx.value >= THUMBNAIL_MIN_WIDTH_PX;
  return a && props.assetKind === "video" && fits && hasPreviewSource(a) ? a : null;
});
/** The waveform's drawing width: the lane is inset 4 px on each side. */
const waveformWidthPx = computed(() => Math.max(previewWidthPx.value - 8, 1));

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

/** Cancels any in-progress body/trim/fade preview and reports whether one
 * was actually active -- extracted out of `onKeydown`'s own Escape branch
 * (fallow complexity: three previews `||`'d together there pushed that
 * function's cyclomatic count over the ratchet) so the keydown dispatcher
 * stays a flat table of single-condition branches. */
function cancelActiveDrag(): boolean {
  const active = drag.movePreview.value !== null || drag.trimPreview.value !== null || drag.fadePreview.value !== null;
  drag.cancelBodyDrag();
  drag.cancelTrim();
  drag.cancelFade();
  return active;
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
  if (event.key === "Escape" && cancelActiveDrag()) {
    return;
  }
  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    void onNudge(event);
  }
}
</script>

<template>
  <div
    :ref="bindRoot"
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
      v-if="previewFadeInMs > 0"
      :data-testid="`clip-${clip.id}-fade-in`"
      class="pointer-events-none absolute inset-y-0 left-0 bg-gold/40"
      :style="{ width: `${fadeWedgeWidthPx(previewFadeInMs)}px`, clipPath: 'polygon(0 100%, 100% 100%, 0 0)' }"
    />
    <div
      v-if="previewFadeOutMs > 0"
      :data-testid="`clip-${clip.id}-fade-out`"
      class="pointer-events-none absolute inset-y-0 right-0 bg-gold/40"
      :style="{ width: `${fadeWedgeWidthPx(previewFadeOutMs)}px`, clipPath: 'polygon(0 100%, 100% 100%, 100% 0)' }"
    />
    <!-- Always-visible drag handles (never gated on a nonzero fade, unlike
         the wedge above): a fade has to be CREATED from zero somehow, so
         the grab target exists even when there is nothing to see yet. -->
    <span
      :data-testid="`clip-${clip.id}-fade-in-handle`"
      class="absolute -top-0.5 left-0 z-10 h-2 w-2 -translate-x-0.5 cursor-ew-resize rounded-full bg-gold"
      @pointerdown.stop="onFadePointerDown($event, 'in')"
      @pointermove.stop="onFadePointerMove"
      @pointerup.stop="onFadePointerUp"
    />
    <span
      :data-testid="`clip-${clip.id}-fade-out-handle`"
      class="absolute -top-0.5 right-0 z-10 h-2 w-2 translate-x-0.5 cursor-ew-resize rounded-full bg-gold"
      @pointerdown.stop="onFadePointerDown($event, 'out')"
      @pointermove.stop="onFadePointerMove"
      @pointerup.stop="onFadePointerUp"
    />
    <!-- F-26 (Task 28): the waveform of an audio clip, and a poster frame
         for a video clip whose asset has a real file. Both mount only with
         this ClipItem, i.e. only for a clip the timeline keeps visible. -->
    <div
      v-if="waveformAsset"
      :data-testid="`clip-${clip.id}-waveform`"
      class="pointer-events-none absolute inset-x-1 bottom-0.5 h-3 rounded bg-audio-bg/60"
    >
      <ClipWaveform
        :asset-id="waveformAsset.id"
        :asset-duration-ms="waveformAsset.duration_ms"
        :in-ms="shownInMs"
        :out-ms="shownOutMs"
        :width-px="waveformWidthPx"
      />
    </div>
    <div
      v-else-if="posterAsset"
      :data-testid="`clip-${clip.id}-thumbnail`"
      class="pointer-events-none absolute inset-y-0 left-1"
    >
      <ClipThumbnail
        :asset-id="posterAsset.id"
        :at-ms="clip.in_ms"
      />
    </div>

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
