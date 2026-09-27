<script setup lang="ts">
/**
 * One clip's visual (Task 20; F-04, F-14, F-26). Presentational and
 * absolutely positioned by its caller (`TrackLane.vue` supplies
 * `leftPx`/`widthPx` in CONTENT space); it never reads zoom or scroll.
 *
 * Selection is a direct store write (`editorWorkspace.select`); the only
 * thing this component bubbles UP is "open a context menu here", because
 * the ONE `ContextMenu` lives at `TimelineView.vue`. Right-click and
 * Shift+F10/Menu funnel into the SAME `context-menu` emit.
 *
 * Drag/trim (Task 21; F-07..F-11): the BODY drags to move the clip, the
 * trim handles trim it and the gold fade handles (Task 29; F-17, F-18) set
 * its fades — each a self-contained pointer-capture lifecycle (so a fast
 * drag past the clip's edge keeps tracking) whose MATH is
 * `useTimelineDrag`'s; this component owns only the DOM plumbing. A
 * release sends exactly ONE `moveClips`/`trimClip`/`setFades`; Escape
 * discards the preview with nothing sent. ←/→ (Shift: 1 s) nudge the
 * FOCUSED clip by one `moveClips` per key and give focus back after the
 * re-render — `nextTick` plus a re-query by the clip's STABLE
 * `data-testid`, since Vue may swap the node during the round trip.
 *
 * **Anatomy (visual-parity Task 18, concept spec §6.5).** The body is 47 px
 * tall at top 5 of the 68 px row, radius 5, its kind's fill and edge, and
 * never narrower than 5 px; a selected clip has a 2 px accent OUTLINE (an
 * outline, not a box-shadow ring, so a Windows contrast theme keeps it).
 * Inside a clipped content layer: the filmstrip or the bar waveform, the
 * name (bottom-left with a 10 px glyph; an audio clip's at the top), the
 * chips (`ClipBadges`) and the fade shape (`ClipFadeShape`, a nonzero fade
 * only). Outside it, so they can ride the edges: the fade handles — 13 px
 * gold circles on the TOP edge at the knees, always there because a fade
 * is CREATED from zero — and the trim handles — 9 × 33 grips with a 2 px
 * ink bar, which a clip narrower than two grips moves outside its own
 * edges so both stay grabbable. Both show on hover or while selected and
 * keep `vb-handle`. A move or trim that snaps reports its target to
 * `TimelineView`'s dashed guide (`snapGuide.ts`).
 *
 * `role="option"` (fix round 1, finding 3): `aria-selected` is valid on an
 * option, not on a button (`SearchHitRow.vue`'s precedent). A `<div>` has
 * no native activation, so Enter/Space select it in `onKeydown`, beside
 * Shift+F10/Menu.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useTimelineDrag } from "../../../composables/useTimelineDrag";
import { hasPreviewSource } from "../../../editor/previewLayers";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { useSnapGuideReport } from "../../../editor/snapGuide";
import { MIN_CLIP_WIDTH_PX, msToX, snapTargets as computeSnapTargets } from "../../../editor/timelineLayout";
import { clipOutputDuration, clipOutputEnd } from "../../../editor/timeMap";
import { trackAccepts } from "../../../editor/trackCompat";
import type { Clip, ClipSpan } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";
import EditorIcon from "../icons/EditorIcon.vue";
import ClipBadges from "./ClipBadges.vue";
import ClipFadeShape from "./ClipFadeShape.vue";
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
  (e: "context-menu", payload: { clip: Clip; clientX: number; clientY: number; atPlayhead?: boolean }): void;
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

/** The dashed guide `TimelineView` draws while this clip's drag snaps. */
useSnapGuideReport(drag.snapGuideMs);

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

// ---- fade handles (Task 29; F-17, F-18): the trim handles' lifecycle -------

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

// ---- fades (Task 29; §6.5) -----------------------------------------------
// The knees as percentages of the body, from the LIVE drag preview (falling
// back to the committed clip), so dragging a handle from zero grows the
// gold shape under it as the user watches. Each knee stays on its own half.

const previewFadeInMs = computed(() =>
  drag.fadePreview.value?.edge === "in" ? drag.fadePreview.value.ms : props.clip.fade_in_ms,
);
const previewFadeOutMs = computed(() =>
  drag.fadePreview.value?.edge === "out" ? drag.fadePreview.value.ms : props.clip.fade_out_ms,
);
/** The body's output length now (a live trim previews its own range). */
const shownDurationMs = computed(() => {
  const tp = drag.trimPreview.value;
  const c = props.clip;
  return clipOutputDuration(tp?.inMs ?? c.in_ms, tp?.outMs ?? c.out_ms, c.speed ?? 1);
});
function pct(ms: number): number {
  return Math.round(Math.min(50, (ms * 100) / Math.max(1, shownDurationMs.value)) * 100) / 100;
}
const fadeInPct = computed(() => pct(previewFadeInMs.value));
const fadeOutPct = computed(() => 100 - pct(previewFadeOutMs.value));
/** Both fade handles: where each sits, and what its title says. */
const fadeHandles = computed(() =>
  (["in", "out"] as const).map((edge) => {
    const ms = edge === "in" ? previewFadeInMs.value : previewFadeOutMs.value;
    return {
      edge,
      left: `${edge === "in" ? fadeInPct.value : fadeOutPct.value}%`,
      title: `Fade ${edge}: ${(ms / 1000).toFixed(1)}s. Drag, or use Fades properties.`,
    };
  }),
);

/** The drawn width: never under the concept's 5 px floor. */
const bodyWidthPx = computed(() => Math.max(previewWidthPx.value, MIN_CLIP_WIDTH_PX));
/** Narrower than two 9 px grips, the grips step outside the body's edges. */
const TRIM_GRIP_PX = 9;
const narrow = computed(() => bodyWidthPx.value < TRIM_GRIP_PX * 2);
/** Hover or selection shows the handles (§11). */
const handleShown = computed(() => (props.selected ? "opacity-100" : "opacity-0 group-hover:opacity-100"));
/** Both trim grips: inside the body's edges, or just outside a narrow one. */
const trimHandles = computed(() => [
  { edge: "start" as const, place: narrow.value ? "right-full" : "left-0", bar: "left-[2px]" },
  { edge: "end" as const, place: narrow.value ? "left-full" : "right-0", bar: "right-[2px]" },
]);
/** The body's colours by kind, and the selection outline over them. */
const bodyClass = computed(() => [
  props.assetKind === "audio" ? "border-clip-audio-edge bg-audio-bg text-audio" : "border-clip-video-edge bg-video-bg text-video",
  props.selected ? "z-[4] outline-2 outline-offset-0 outline-accent" : "z-[2]",
]);
/** A video clip's name sits bottom-left over the picture; an audio clip's
 * at the top, above its bars, in its own colour. */
const nameClass = computed(() =>
  props.assetKind === "audio" ? "top-[3px] text-audio" : "bottom-[5px] text-fg [text-shadow:0_1px_var(--color-panel)]",
);
const glyph = computed(() => (props.assetKind === "audio" ? "music" : "video"));

// ---- derived media (Task 28) -----------------------------------------------

/** The asset this clip plays — `null` for a dangling reference, which then
 * draws neither a waveform nor a thumbnail. */
const asset = computed(() => editorProject.project?.assets.find((a) => a.id === props.clip.asset_id) ?? null);
/** The source range on screen: a live trim previews its own in/out so the
 * waveform follows the handle instead of stretching the committed range. */
const shownInMs = computed(() => drag.trimPreview.value?.inMs ?? props.clip.in_ms);
const shownOutMs = computed(() => drag.trimPreview.value?.outMs ?? props.clip.out_ms);
/** Narrower than this, a filmstrip is noise over the clip's name. */
const THUMBNAIL_MIN_WIDTH_PX = 48;
/** An audio clip's asset, for its waveform lane (`null`: no lane) — only
 * when it has a real file to decode: a synthesized builtin would be refused
 * by Rust on every mount (fix round 1, review Minor 7). */
const waveformAsset = computed(() => {
  const a = asset.value;
  return a && props.assetKind === "audio" && hasPreviewSource(a) ? a : null;
});
/** A video clip's asset when it has a real file to cut a frame from and
 * the clip is wide enough to show one (`null`: no filmstrip). */
const posterAsset = computed(() => {
  const a = asset.value;
  const fits = previewWidthPx.value >= THUMBNAIL_MIN_WIDTH_PX;
  return a && props.assetKind === "video" && fits && hasPreviewSource(a) ? a : null;
});

const NUDGE_FRAME_MS = 33;
const NUDGE_SECOND_MS = 1_000;

async function onNudge(event: KeyboardEvent) {
  event.preventDefault();
  const amount = event.shiftKey ? NUDGE_SECOND_MS : NUDGE_FRAME_MS;
  const deltaMs = event.key === "ArrowLeft" ? -amount : amount;
  const el = event.currentTarget as HTMLElement;
  await drag.nudge(deltaMs);
  // The stable key usually keeps `el` live; the re-query covers a clip that
  // briefly left the virtualization window mid-round-trip.
  await nextTick();
  const restored =
    document.querySelector<HTMLElement>(`[data-testid="clip-${props.clip.id}"]`) ?? el;
  restored?.focus();
}

/** Cancels any body/trim/fade preview; whether one was active (kept out of
 * `onKeydown` so its branches stay flat, for the complexity ratchet). */
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
    emit("context-menu", { clip: props.clip, clientX: rect.left, clientY: rect.bottom, atPlayhead: true });
    return;
  }
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    onSelect();
    return;
  }
  if (event.key === "Escape" && cancelActiveDrag()) {
    // Claimed: without this, the Escape kept bubbling with
    // `defaultPrevented: false` all the way to `EditorShell`'s dispatcher,
    // whose `isGuideDismissKey` reads exactly that flag -- so cancelling a
    // drag preview also dismissed an open guide coach in the same keypress
    // (review finding F-M2).
    event.preventDefault();
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
    class="group absolute top-[5px] h-[47px] cursor-grab touch-none rounded-[5px] border select-none focus-visible:z-[5] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
    :class="bodyClass"
    :style="{ left: `${previewLeftPx}px`, width: `${bodyWidthPx}px` }"
    @click="onSelect"
    @contextmenu.prevent="onContextMenu"
    @keydown="onKeydown"
    @pointerdown="onBodyPointerDown"
    @pointermove="onBodyPointerMove"
    @pointerup="onBodyPointerUp"
  >
    <div
      :data-testid="`clip-${clip.id}-content`"
      class="pointer-events-none absolute inset-0 overflow-hidden rounded-[4px]"
    >
      <!-- F-26 (Task 28): the bars of an audio clip, and the filmstrip of a
           video clip whose asset has a real file. Both mount only with this
           ClipItem, i.e. only for a clip the timeline keeps visible. -->
      <div
        v-if="waveformAsset"
        :data-testid="`clip-${clip.id}-waveform`"
        class="absolute inset-0"
      >
        <ClipWaveform
          :asset-id="waveformAsset.id"
          :asset-duration-ms="waveformAsset.duration_ms"
          :in-ms="shownInMs"
          :out-ms="shownOutMs"
          :width-px="bodyWidthPx"
        />
      </div>
      <div
        v-else-if="posterAsset"
        :data-testid="`clip-${clip.id}-thumbnail`"
        class="absolute inset-0"
      >
        <ClipThumbnail
          :clip-id="clip.id"
          :asset-id="posterAsset.id"
          :at-ms="clip.in_ms"
        />
      </div>

      <span
        :data-testid="`clip-${clip.id}-name`"
        class="absolute right-[6px] left-[9px] z-[3] truncate text-[10px] font-[550]"
        :class="nameClass"
      ><EditorIcon
        :name="glyph"
        :size="10"
        class="mr-1 inline align-[-1px]"
      />{{ clip.name }}</span>

      <ClipBadges
        :clip="clip"
        :asset="asset"
        :kind="assetKind"
        :width-px="bodyWidthPx"
        :duration-ms="shownDurationMs"
      />
      <ClipFadeShape
        :clip-id="clip.id"
        :fade-in-pct="fadeInPct"
        :fade-out-pct="fadeOutPct"
      />
    </div>

    <!-- Always there (a fade has to be CREATED from zero, so the grab target
         exists before there is anything to see); shown on hover/selection. -->
    <span
      v-for="h in fadeHandles"
      :key="h.edge"
      :data-testid="`clip-${clip.id}-fade-${h.edge}-handle`"
      :title="h.title"
      class="vb-handle absolute -top-1 z-[8] h-[13px] w-[13px] -translate-x-1/2 cursor-ew-resize rounded-full border-2 border-panel bg-gold"
      :class="handleShown"
      :style="{ left: h.left }"
      @pointerdown.stop="onFadePointerDown($event, h.edge)"
      @pointermove.stop="onFadePointerMove"
      @pointerup.stop="onFadePointerUp"
    />

    <span
      v-for="t in trimHandles"
      :key="t.edge"
      :data-testid="`clip-${clip.id}-trim-${t.edge}`"
      :title="`Trim ${t.edge}; the exact range is in Clip properties`"
      class="absolute top-[10px] z-[7] h-[33px] w-[9px] cursor-ew-resize"
      :class="[handleShown, t.place]"
      @pointerdown.stop="onTrimPointerDown($event, t.edge)"
      @pointermove.stop="onTrimPointerMove"
      @pointerup.stop="onTrimPointerUp"
    >
      <span
        :data-testid="`clip-${clip.id}-trim-${t.edge}-bar`"
        class="vb-handle absolute top-1.5 bottom-1.5 w-[2px] rounded-[2px] bg-fg"
        :class="t.bar"
      />
    </span>
  </div>
</template>
