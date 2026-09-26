<script setup lang="ts">
/**
 * The tutorial editor's virtualized multi-track timeline (Task 20; F-04,
 * F-14, F-26; SCREENS-AND-INTERACTIONS.md §03). Fills `EditorShell.vue`'s
 * `timeline` slot (`EditorRoot.vue` wires it, the `InspectorPanel`/Task 19
 * precedent). Owns everything the leaf components can't own themselves
 * because it is the one thing that measures the viewport and scrolls: the
 * ONE `ContextMenu` instance (clip right-click AND Shift+F10 both funnel
 * into it — SCREENS-AND-INTERACTIONS.md §03: "The same actions are reachable
 * from … Shift+F10"), virtualization (`timelineLayout.visibleClips`), the
 * REAL "fit" computation `editorWorkspace.fit()`'s own doc explicitly defers
 * to "a later task that DOES know the timeline's rendered width". Its
 * height is the frame's (visual-parity Task 4): it fills the grid row the
 * shell sizes from `editorWorkspace.timelineHeightPx`, and the resize
 * handle is the shell's own splitter row above it (`TimelineSplitter`).
 *
 * `viewportWidth` is a TEST-ONLY prop override (the `PreviewToolbar.
 * overflowCount` precedent) — `undefined` in production, where a
 * `ResizeObserver` measures the real scroll container; happy-dom implements
 * neither a layout engine nor (reliably) `ResizeObserver`, so a test drives
 * the width directly. The un-overridden default (1000px) is deliberately a
 * plausible viewport width, not 0 — a real measurement failing silently
 * must not blank every clip on the timeline.
 *
 * **Multi-track placement and stills (Task 26)**: a `LibraryAssetCard`
 * dropped onto an existing `TrackLane` inserts a clip at the drop's own
 * snapped time (`TrackLane` decides ACCEPTANCE via `trackCompat.
 * trackAccepts`/`dropRefusalReason` and emits `asset-drop`; this component
 * alone knows the scroll offset and label width needed to turn the drop's
 * `clientX` into a timeline `ms`, so it owns the actual `insertClip`).
 * Dropping onto the strip BELOW the last lane mints a track of the dropped
 * asset's own kind first (`addTrack`) and inserts onto it second
 * (`insertClip`) — two separate `editorProject.execute` calls, so Undo
 * sees two labelled steps, never one merged "add track and clip" edit
 * Rust has no single command for.
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import type { TimelineViewOps } from "../../../composables/useEditorMenuContext";
import { SNAP_THRESHOLD_PX, snappedMs } from "../../../composables/useTimelineDrag";
import { baseActionContext } from "../../../editor/actionContext";
import type { PointerTarget } from "../../../editor/actions";
import { onReveal, revealedTimelineMs } from "../../../editor/revealBus";
import {
  fitZoom,
  LANE_HEIGHT_PX,
  pxPerMs,
  revealScrollLeft,
  snapTargets,
  TRACK_LABEL_WIDTH_PX,
  visibleClips,
  xToMs,
} from "../../../editor/timelineLayout";
import { draggedAssetId, draggedAssetKind } from "../../../editor/trackCompat";
import { addTrackThenInsert } from "../../../editor/trackEdits";
import type { Asset, Clip } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import ContextMenu from "../menus/ContextMenu.vue";
import TimelineRuler from "./TimelineRuler.vue";
import TimelineToolbar from "./TimelineToolbar.vue";
import TrackLane from "./TrackLane.vue";

const props = withDefaults(defineProps<{ viewportWidth?: number }>(), { viewportWidth: undefined });

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

// ---- viewport width measurement --------------------------------------------

const measuredWidth = ref(1000);
const effectiveViewportWidth = computed(() => props.viewportWidth ?? measuredWidth.value);

const scrollRef = ref<HTMLElement | null>(null);
let observer: ResizeObserver | null = null;

onMounted(() => {
  if (scrollRef.value) {
    scrollRef.value.scrollLeft = workspace.timelineScrollLeft;
    scrollRef.value.scrollTop = workspace.timelineScrollTop;
    scrollLeftPx.value = workspace.timelineScrollLeft;
  }
  if (props.viewportWidth === undefined && typeof ResizeObserver === "function" && scrollRef.value) {
    observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) measuredWidth.value = entry.contentRect.width;
    });
    observer.observe(scrollRef.value);
  }
});
onBeforeUnmount(() => observer?.disconnect());

// ---- scroll ------------------------------------------------------------

const scrollLeftPx = ref(0);
/** `event.currentTarget`, not `scrollRef.value` -- the listener is bound
 * directly on the same element the ref names, so a defensive
 * `if (!scrollRef.value)` guard here would be an untestable branch for a
 * state that can't occur (the event can only fire on an attached element),
 * the `actions.ts` module doc's own "trusting the invariant" precedent. */
function onScroll(event: Event) {
  const el = event.currentTarget as HTMLElement;
  scrollLeftPx.value = el.scrollLeft;
  workspace.setTimelineScroll(el.scrollLeft, el.scrollTop);
}

/** Task 54: a before-you-share finding's "Show it" scrolls its object in. */
onReveal("timeline", () => {
  const el = scrollRef.value;
  const left = el
    ? revealScrollLeft(revealedTimelineMs(), workspace.timelineZoom, el.scrollLeft, effectiveViewportWidth.value)
    : null;
  if (!el || left === null) return;
  el.scrollLeft = left;
  scrollLeftPx.value = left;
  workspace.setTimelineScroll(left, el.scrollTop);
});

// ---- content geometry ----------------------------------------------------

/** Editing headroom past the edit's own end, so there is somewhere to drop
 * a clip after the last one without the content area's own width fighting
 * the drop target. */
const TRAILING_PADDING_MS = 5_000;

const contentDurationMs = computed(() => Math.max(editorProject.durationMs, 0) + TRAILING_PADDING_MS);
const contentWidthPx = computed(() => pxPerMs(workspace.timelineZoom) * contentDurationMs.value);

const tracks = computed(() => editorProject.project?.tracks ?? []);
/** Task 21: the lane order `ClipItem`'s cross-track drop hit-test needs —
 * see `TrackLane.vue`'s own `trackOrder` prop doc. */
const orderedTrackIds = computed(() => tracks.value.map((t) => t.id));

/** `visibleClips` operates on the CLIP body's own coordinate space
 * (`TrackLane`'s convention: `msToX(start_ms, zoom)` with no label offset),
 * while `scrollLeftPx` is measured against the whole scrolled row INCLUDING
 * the label column (`TrackLane`/`TimelineRuler` both scroll the label along
 * with the content, their own doc explains why) -- so the label width has to
 * come back out before the viewport window is converted to ms. */
const bodyScrollLeft = computed(() => Math.max(0, scrollLeftPx.value - TRACK_LABEL_WIDTH_PX));

const visible = computed(() =>
  visibleClips(
    editorProject.project?.clips ?? [],
    bodyScrollLeft.value,
    effectiveViewportWidth.value,
    workspace.timelineZoom,
  ),
);
const clipsByTrack = computed(() => {
  const map = new Map<string, Clip[]>();
  for (const c of visible.value) {
    const list = map.get(c.track_id);
    if (list) list.push(c);
    else map.set(c.track_id, [c]);
  }
  return map;
});

// ---- fit -------------------------------------------------------------------

function applyScrollLeft(left: number) {
  if (scrollRef.value) scrollRef.value.scrollLeft = left;
  scrollLeftPx.value = left;
}

function onFit() {
  const zoom = fitZoom(editorProject.durationMs, effectiveViewportWidth.value);
  workspace.setZoom(zoom);
  workspace.setTimelineScroll(0, workspace.timelineScrollTop);
  applyScrollLeft(0);
}

/** The context menu's view changes (visual-parity Task 5): only this
 * component knows the viewport width and owns the scroller. */
const menuView: TimelineViewOps = {
  fitRange: (startMs, endMs) => applyScrollLeft(workspace.zoomToRange(startMs, endMs, effectiveViewportWidth.value)),
  fitTimeline: onFit,
};

// ---- the one context menu ----------------------------------------------

const menuOpen = ref(false);
/** Opened by the toolbar's Edit actions, for its `aria-expanded`. */
const menuFromToolbar = ref(false);
const menuX = ref(0);
const menuY = ref(0);
const menuTarget = ref<PointerTarget | null>(null);
const menuContext = computed(() =>
  baseActionContext(
    editorProject.project,
    editorProject.snapshot,
    workspace.playheadMs,
    workspace.selectionClipIds,
    menuTarget.value,
  ),
);

/** Whole milliseconds: every time Rust takes is a `u64`, and at a fitted
 * zoom a pixel is a fraction of a millisecond (Task 5 fix round 1). */
function msFromClientX(clientX: number): number {
  if (!scrollRef.value) return workspace.playheadMs;
  const rect = scrollRef.value.getBoundingClientRect();
  const contentX = clientX - rect.left + scrollRef.value.scrollLeft - TRACK_LABEL_WIDTH_PX;
  return Math.round(xToMs(Math.max(0, contentX), workspace.timelineZoom));
}

function openMenu(target: PointerTarget | null, x: number, y: number, fromToolbar = false) {
  menuTarget.value = target;
  menuX.value = x;
  menuY.value = y;
  menuFromToolbar.value = fromToolbar;
  menuOpen.value = true;
}

/** A right-click acts at the pointer's time; Shift+F10 at the playhead's. */
function onClipContextMenu(payload: { clip: Clip; clientX: number; clientY: number; atPlayhead?: boolean }) {
  const timeMs = payload.atPlayhead ? workspace.playheadMs : msFromClientX(payload.clientX);
  openMenu({ kind: "clip", id: payload.clip.id, timeMs }, payload.clientX, payload.clientY);
}

/** A right-click on an empty stretch of a lane (visual-parity Task 5). */
function onLaneContextMenu(payload: { trackId: string; clientX: number; clientY: number }) {
  openMenu({ kind: "gap", id: payload.trackId, timeMs: msFromClientX(payload.clientX) }, payload.clientX, payload.clientY);
}

/** The toolbar's Edit actions (Task 55): the same menu, for the selection
 * at the playhead, or the editor actions when nothing is selected. */
function onToolbarMore(at: { x: number; y: number }) {
  const first = workspace.selectionClipIds[0];
  const target: PointerTarget | null = first ? { kind: "clip", id: first, timeMs: workspace.playheadMs } : null;
  openMenu(target, at.x, at.y, true);
}

// ---- native drag-and-drop: place a library asset (Task 26) ----------------

/** `clientX` -> a snapped, non-negative integer `ms` -- the SAME snap
 * targets/threshold a clip drag/trim already uses (`useTimelineDrag`'s
 * exported `SNAP_THRESHOLD_PX`/`snappedMs`), so a dropped asset settles
 * against the identical playhead/clip-edge magnets a dragged clip would. */
function snappedMsFromClientX(clientX: number): number {
  const raw = msFromClientX(clientX);
  const snapped = snappedMs(raw, {
    snapEnabled: workspace.snap,
    targets: snapTargets(editorProject.project, workspace.playheadMs),
    thresholdPx: SNAP_THRESHOLD_PX,
    zoom: workspace.timelineZoom,
  });
  return Math.max(0, Math.round(snapped));
}

/** `TrackLane`'s own `asset-drop`: it already confirmed the lane accepts
 * this asset's kind (`trackCompat.trackAccepts`) before emitting, so this
 * only resolves the asset (for its default `outMs`) and the drop's time. */
async function onAssetDrop(payload: { assetId: string; trackId: string; clientX: number }) {
  const asset = editorProject.project?.assets.find((a) => a.id === payload.assetId);
  if (!asset) return;
  await editorProject.execute({
    kind: "insertClip",
    assetId: asset.id,
    trackId: payload.trackId,
    startMs: snappedMsFromClientX(payload.clientX),
    inMs: 0,
    outMs: asset.duration_ms,
  });
}

function onBelowLanesDragOver(event: DragEvent) {
  const dt = event.dataTransfer;
  if (!dt || draggedAssetKind(dt) === null) return; // not one of ours
  // Below the last lane always accepts a valid kind -- there is no existing
  // track to refuse against, only one about to be minted.
  event.preventDefault();
  dt.dropEffect = "copy";
}

/** A native drag's asset, resolved -- `null` for a drag that is not one of
 * ours, or whose id does not resolve (a payload from a since-closed
 * project). The fresh track takes the asset's own kind. */
function resolveDraggedAsset(dt: DataTransfer): Asset | null {
  const kind = draggedAssetKind(dt);
  if (kind === null) return null;
  const assetId = draggedAssetId(dt, kind);
  return (assetId && editorProject.project?.assets.find((a) => a.id === assetId)) || null;
}

/** `addTrack` then `insertClip` (`trackEdits.ts`): two undo steps, and
 * nothing is inserted when Rust refuses the track. */
async function onBelowLanesDrop(event: DragEvent) {
  const dt = event.dataTransfer;
  const asset = dt ? resolveDraggedAsset(dt) : null;
  if (!asset) return;
  event.preventDefault();
  await addTrackThenInsert(
    (command) => editorProject.execute(command),
    () => editorProject.project,
    asset,
    snappedMsFromClientX(event.clientX),
  );
}
</script>

<template>
  <div
    data-testid="timeline-view"
    class="flex h-full min-h-0 flex-col gap-1"
  >
    <TimelineToolbar
      :more-open="menuOpen && menuFromToolbar"
      @fit="onFit"
      @more="onToolbarMore"
    />

    <div
      ref="scrollRef"
      data-testid="timeline-scroll"
      class="vb-thin-scroll relative min-h-0 flex-1 overflow-auto"
      @scroll="onScroll"
    >
      <TimelineRuler
        :zoom="workspace.timelineZoom"
        :width-px="contentWidthPx"
      />
      <div
        data-testid="timeline-playhead"
        class="vb-playhead pointer-events-none absolute top-0 bottom-0 z-30 w-px bg-accent"
        :style="{ left: `${TRACK_LABEL_WIDTH_PX + pxPerMs(workspace.timelineZoom) * workspace.playheadMs}px` }"
      />
      <TrackLane
        v-for="(track, i) in tracks"
        :key="track.id"
        :track="track"
        :clips="clipsByTrack.get(track.id) ?? []"
        :assets="editorProject.project?.assets ?? []"
        :selected-clip-ids="workspace.selectionClipIds"
        :zoom="workspace.timelineZoom"
        :width-px="contentWidthPx"
        :track-index="i"
        :track-order="orderedTrackIds"
        @context-menu="onClipContextMenu"
        @lane-context-menu="onLaneContextMenu"
        @asset-drop="onAssetDrop"
      />
      <!-- Multi-track placement (Task 26): dropping a library asset here
           mints a track of its own kind first, then inserts onto it -- the
           one drop target with no existing lane to accept/refuse against. -->
      <div
        data-testid="timeline-below-lanes"
        class="relative"
        :style="{ width: `${contentWidthPx + TRACK_LABEL_WIDTH_PX}px`, height: `${LANE_HEIGHT_PX}px` }"
        @dragover="onBelowLanesDragOver"
        @drop="onBelowLanesDrop"
      />
    </div>

    <ContextMenu
      :open="menuOpen"
      :context="menuContext"
      :x="menuX"
      :y="menuY"
      :view="menuView"
      @close="menuOpen = false"
    />
  </div>
</template>
