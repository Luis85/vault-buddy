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
 * `viewportWidth` is a TEST-ONLY prop override (the retired preview
 * toolbar's `overflowCount` precedent) — `undefined` in production, where a
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
 *
 * **Playhead, snap guide, gap hints, Fit (visual-parity Task 18, concept
 * spec §6.3, §6.5).** Everything below the toolbar sits in one content
 * layer as tall as the rows, so the playhead — a 1 px accent line from the
 * ruler's foot down (its pentagon head is the ruler's own) — and the dashed
 * gold snap guide a snapping drag shows (`snapGuide.ts`) run the full
 * height. Both are stacked ABOVE the clips and the locked hatch (z-12) and
 * BELOW the pinned label cells (z-15) and the sticky ruler (z-20), so
 * scrolled sideways they slide under the label column instead of painting
 * over the track names. Each unlocked lane gets its gaps wider than 70 px
 * (`trackEdits.gapsOnTrack`, the lane menu's own) for its "Close gap"
 * hints, which send the lane menu's own `closeGapCommand`. Fit fits the
 * edit into the lanes as the concept does: the timeline's width less the
 * label column and a 26 px margin, a short edit counted as 15 s.
 *
 * **Teaching layers and captions (visual-parity Task 19, concept spec
 * §6.4, design D11).** Right under the ruler, the Captions row while the
 * edit has captions, then the Teaching layers row above every track: each
 * project cue as a chip packed into rows 22 px apart. Both pin their label
 * cell like a track header, and both keep only the cues within a screen of
 * the viewport, as the lanes keep their clips (`visibleWindowMs`); the
 * lane count comes from EVERY cue, so scrolling never resizes the row. A
 * cue's right-click or Shift+F10 opens the cue menu in the one
 * `ContextMenu`, and with a cue selected the toolbar's Edit actions opens
 * that cue's menu rather than its clip's.
 *
 * **The footer (visual-parity Task 20, concept spec §7)** sits under the
 * lanes: the edit hint the dragged clip or cue reports (`dragHint.ts`,
 * provided here like the snap guide) and the Audio mixer.
 */
import { computed, onBeforeUnmount, onMounted, provide, ref } from "vue";

import type { TimelineViewOps } from "../../../composables/useEditorMenuContext";
import { SNAP_THRESHOLD_PX, snappedMs } from "../../../composables/useTimelineDrag";
import { baseActionContext } from "../../../editor/actionContext";
import type { PointerTarget } from "../../../editor/actions";
import { captionRows } from "../../../editor/captionRules";
import { laneCount, teachingCues } from "../../../editor/cueLanes";
import { DRAG_HINT_KEY } from "../../../editor/dragHint";
import { onReveal, revealedTimelineMs } from "../../../editor/revealBus";
import { SNAP_GUIDE_KEY } from "../../../editor/snapGuide";
import {
  fitTimelineZoom,
  LANE_HEIGHT_PX,
  msToX,
  pxPerMs,
  revealScrollLeft,
  snapTargets,
  trackBadges,
  visibleClips,
  visibleWindowMs,
  xToMs,
} from "../../../editor/timelineLayout";
import { draggedAssetId, draggedAssetKind } from "../../../editor/trackCompat";
import { addTrackThenInsert, closeGapCommand, type Gap, gapsOnTrack } from "../../../editor/trackEdits";
import type { Asset, Clip } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import ContextMenu from "../menus/ContextMenu.vue";
import CaptionsRow from "./CaptionsRow.vue";
import TeachingLayersRow from "./TeachingLayersRow.vue";
import TimelineFooter from "./TimelineFooter.vue";
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
    ? revealScrollLeft(revealedTimelineMs(), workspace.timelineZoom, el.scrollLeft, effectiveViewportWidth.value, labelPx.value)
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
/** V3/V2/V1 top to bottom, A1… (§6.4). */
const badges = computed(() => trackBadges(tracks.value));
/** The label column at this window width (§1.4): every row lays it out, and
 * every pointer/playhead conversion takes it back out. */
const labelPx = computed(() => workspace.trackLabelWidth);
/** Task 21: the lane order `ClipItem`'s cross-track drop hit-test needs —
 * see `TrackLane.vue`'s own `trackOrder` prop doc. */
const orderedTrackIds = computed(() => tracks.value.map((t) => t.id));

/** `visibleClips` operates on the CLIP body's own coordinate space
 * (`TrackLane`'s convention: `msToX(start_ms, zoom)` with no label offset),
 * while `scrollLeftPx` is measured against the whole scrolled row INCLUDING
 * the label column. The column is pinned over the lanes' left edge (D12),
 * so the lanes show body x `scrollLeft .. scrollLeft + viewport - label`;
 * starting the window a label's width earlier renders the few clips hidden
 * under the column too, never one that should show. */
const bodyScrollLeft = computed(() => Math.max(0, scrollLeftPx.value - labelPx.value));

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

// ---- the Teaching layers and Captions rows (§6.4) -----------------------------

const cues = computed(() => teachingCues(editorProject.project));
const cueLanes = computed(() => laneCount(cues.value));
const captions = computed(() => captionRows(editorProject.project));
/** The output window the lanes keep clips in, for the rows' cues too. */
const shownWindow = computed(() =>
  visibleWindowMs(bodyScrollLeft.value, effectiveViewportWidth.value, workspace.timelineZoom),
);
function shown(span: { startMs: number; endMs: number }): boolean {
  const [lo, hi] = shownWindow.value;
  return span.endMs > lo && span.startMs < hi;
}
const shownCues = computed(() => cues.value.filter(shown));
const shownCaptions = computed(() => captions.value.filter(shown));

// ---- the snap guide and the gap hints (§6.5) ---------------------------------

/** The snap target a dragging clip's edge sits on (`snapGuide.ts`). */
const snapGuideMs = ref<number | null>(null);
provide(SNAP_GUIDE_KEY, snapGuideMs);
/** The footer's line for the drag in hand (`dragHint.ts`). */
const dragHint = ref<string | null>(null);
provide(DRAG_HINT_KEY, dragHint);

/** A gap narrower than this shows no hint (the concept's own). */
const GAP_HINT_MIN_PX = 70;
const gapsByTrack = computed(() => {
  const p = editorProject.project;
  const map = new Map<string, Gap[]>();
  for (const t of tracks.value) {
    if (!p || t.locked) continue;
    const wide = gapsOnTrack(p, t.id).filter((g) => msToX(g.end - g.start, workspace.timelineZoom) > GAP_HINT_MIN_PX);
    map.set(t.id, wide);
  }
  return map;
});

/** The lane menu's own "Close this gap" command, for the pressed hint. */
async function onCloseGap(payload: { trackId: string; gap: Gap }) {
  const p = editorProject.project;
  if (p) await editorProject.execute(closeGapCommand(p, payload.trackId, payload.gap));
}

// ---- fit -------------------------------------------------------------------

function applyScrollLeft(left: number) {
  if (scrollRef.value) scrollRef.value.scrollLeft = left;
  scrollLeftPx.value = left;
}

function onFit() {
  const zoom = fitTimelineZoom(editorProject.durationMs, effectiveViewportWidth.value, labelPx.value);
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
  const contentX = clientX - rect.left + scrollRef.value.scrollLeft - labelPx.value;
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

/** A track header's right-click or Shift+F10 (visual-parity Task 17): the
 * track menu. */
function onTrackContextMenu(payload: { trackId: string; clientX: number; clientY: number }) {
  openMenu({ kind: "track", id: payload.trackId, timeMs: null }, payload.clientX, payload.clientY);
}

/** A teaching cue's right-click or Shift+F10 (visual-parity Task 19). */
function onCueContextMenu(payload: { effectId: string; clientX: number; clientY: number }) {
  openMenu({ kind: "effect", id: payload.effectId, timeMs: null }, payload.clientX, payload.clientY);
}

/** A right-click on an empty stretch of a lane (visual-parity Task 5). */
function onLaneContextMenu(payload: { trackId: string; clientX: number; clientY: number }) {
  openMenu({ kind: "gap", id: payload.trackId, timeMs: msFromClientX(payload.clientX) }, payload.clientX, payload.clientY);
}

/** The toolbar's Edit actions (Task 55): the same menu, for the selection
 * at the playhead, or the editor actions when nothing is selected. */
function onToolbarMore(at: { x: number; y: number }) {
  openMenu(selectionTarget(), at.x, at.y, true);
}

/** What Edit actions acts on: a selected cue (its clip is selected with
 * it, but the inspector shows the cue), else the first selected clip. */
function selectionTarget(): PointerTarget | null {
  const sel = workspace.selected;
  if (sel?.type === "effect") return { kind: "effect", id: sel.id, timeMs: null };
  const first = workspace.selectionClipIds[0];
  return first ? { kind: "clip", id: first, timeMs: workspace.playheadMs } : null;
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
    class="flex h-full min-h-0 flex-col"
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
      <div
        data-testid="timeline-content"
        class="relative w-max min-w-full"
      >
        <TimelineRuler
          :zoom="workspace.timelineZoom"
          :width-px="contentWidthPx"
          :label-width="labelPx"
        />
        <div
          data-testid="timeline-playhead"
          class="vb-playhead pointer-events-none absolute top-8 bottom-0 z-[12] w-px bg-accent"
          :style="{ left: `${labelPx + pxPerMs(workspace.timelineZoom) * workspace.playheadMs}px` }"
        />
        <div
          v-if="snapGuideMs !== null"
          data-testid="timeline-snap-guide"
          class="pointer-events-none absolute top-8 bottom-0 z-[12] border-l border-dashed border-gold"
          :style="{ left: `${labelPx + msToX(snapGuideMs, workspace.timelineZoom)}px` }"
        />
        <CaptionsRow
          :rows="shownCaptions"
          :count="captions.length"
          :zoom="workspace.timelineZoom"
          :width-px="contentWidthPx"
          :label-width="labelPx"
        />
        <TeachingLayersRow
          :cues="shownCues"
          :lanes="cueLanes"
          :zoom="workspace.timelineZoom"
          :width-px="contentWidthPx"
          :label-width="labelPx"
          @context-menu="onCueContextMenu"
        />
        <TrackLane
          v-for="(track, i) in tracks"
          :key="track.id"
          :track="track"
          :badge="badges[track.id]"
          :clips="clipsByTrack.get(track.id) ?? []"
          :assets="editorProject.project?.assets ?? []"
          :selected-clip-ids="workspace.selectionClipIds"
          :zoom="workspace.timelineZoom"
          :width-px="contentWidthPx"
          :label-width="labelPx"
          :track-index="i"
          :track-order="orderedTrackIds"
          :gaps="gapsByTrack.get(track.id) ?? []"
          @context-menu="onClipContextMenu"
          @lane-context-menu="onLaneContextMenu"
          @track-context-menu="onTrackContextMenu"
          @asset-drop="onAssetDrop"
          @close-gap="onCloseGap"
        />
        <!-- Multi-track placement (Task 26): dropping a library asset here
             mints a track of its own kind first, then inserts onto it -- the
             one drop target with no existing lane to accept/refuse against. -->
        <div
          data-testid="timeline-below-lanes"
          class="relative"
          :style="{ width: `${contentWidthPx + labelPx}px`, height: `${LANE_HEIGHT_PX}px` }"
          @dragover="onBelowLanesDragOver"
          @drop="onBelowLanesDrop"
        />
      </div>
    </div>

    <TimelineFooter :hint="dragHint" />

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
