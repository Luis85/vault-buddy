<script setup lang="ts">
/**
 * One track's row (Task 20; F-04; visual-parity Task 17, concept spec §6.4,
 * design D12): the label cell, then a content-space clip area `widthPx`
 * wide. `clips` arrives ALREADY filtered to this track and ALREADY
 * virtualized (`TimelineView`'s one `visibleClips` call) — this component
 * never re-derives either.
 *
 * **The label cell is pinned** (`position: sticky; left: 0`, D12) in the
 * one scroll container, above the clips and the grid (`z-[15]`: a clip's
 * own handles sit at `z-10`), below the sticky ruler row (`z-20`). A row
 * is `LANE_HEIGHT_PX` tall — the header's content plus the bottom rule —
 * so the drag's cross-track hit-test shares the number. The cell holds
 * `TrackHeader`; a right-click on it, or Shift+F10 / the Menu key while
 * focus is inside it, asks `TimelineView` for the track menu
 * (`track-context-menu`, ruling P4).
 *
 * **The lane**: a 1px grid line at every ruler tick (`rulerTicks.tickStep`,
 * the ruler's own step), "Drop video here · or add from Media" while it
 * holds no clip, a diagonal hatch while the track is locked
 * (`vb-lane-locked`, `style.css`, which keeps it in a Windows contrast
 * theme), its clips at 40 % while a video track is hidden, and the accent
 * wash while an accepted asset drag is over it.
 *
 * **Gap hints** (visual-parity Task 18, concept spec §6.5): every gap wider
 * than 70 px (`gaps`, which `TimelineView` works out with the lane menu's
 * own `trackEdits.gapsOnTrack` and never offers on a locked track) holds a
 * dashed "Close gap" button, faint (16 %) until the lane is hovered or
 * holds focus; a click asks `TimelineView` to send the lane menu's own
 * `closeGapCommand` for that gap.
 *
 * **A locked track's clip body is `pointer-events-none`** — Rust's own
 * `ensure_unlocked`-shaped refusal (`core::editor::commands::tracks`) means
 * a drag/trim/select that started here could only end in a rejected
 * command, so the gesture never starts. The reason is the body's `title`,
 * `actionMeta.ts`'s `lockedReason` — the same text a locked track's clip
 * actions show elsewhere.
 *
 * **Native drag-and-drop target (Task 26)**: dropping a `LibraryAssetCard`
 * onto this lane inserts a clip at the drop's own time — `insertClip`
 * through `editorProject.execute`, owned by the parent (`TimelineView.vue`,
 * which alone knows the scroll/label offset needed to turn a `clientX` into
 * a timeline `ms`); this component only decides WHETHER the drop lands
 * (`trackCompat.trackAccepts`) and shows why not. The listeners sit on the
 * ROOT element, not the (possibly `pointer-events-none`) body: a locked
 * lane must still show its own refusal reason during a native drag.
 */
import { computed, ref } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { laneGridStyle } from "../../../editor/rulerTicks";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { LANE_HEIGHT_PX, msToX, pxPerMs } from "../../../editor/timelineLayout";
import { clipOutputEnd } from "../../../editor/timeMap";
import { draggedAssetId, draggedAssetKind, dropRefusalReason, trackAccepts } from "../../../editor/trackCompat";
import type { Gap } from "../../../editor/trackEdits";
import type { Asset, AssetKind, Clip, ClipSpan, Track } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";
import ClipItem from "./ClipItem.vue";
import TrackHeader from "./TrackHeader.vue";

const props = defineProps<{
  track: Track;
  /** `V3`, `A1`… (`timelineLayout.trackBadges`). */
  badge: string;
  clips: Clip[];
  assets: Asset[];
  selectedClipIds: string[];
  zoom: number;
  widthPx: number;
  /** The label column at this window width (`editorWorkspace.trackLabelWidth`). */
  labelWidth: number;
  /** This lane's own index in the visual (top-to-bottom) track order, and
   * that same order's ids — Task 21's `useTimelineDrag` cross-track
   * drop-target hit-test, threaded straight through to each `ClipItem`
   * without this component needing to know why. */
  trackIndex: number;
  trackOrder: string[];
  /** The gaps wide enough for a "Close gap" hint (§6.5); none when absent. */
  gaps?: Gap[];
}>();
const emit = defineEmits<{
  (e: "context-menu", payload: { clip: Clip; clientX: number; clientY: number; atPlayhead?: boolean }): void;
  /** A right-click on the lane's empty body (visual-parity Task 5). */
  (e: "lane-context-menu", payload: { trackId: string; clientX: number; clientY: number }): void;
  /** The header asks for the track menu (visual-parity Task 17). */
  (e: "track-context-menu", payload: { trackId: string; clientX: number; clientY: number }): void;
  /** A `LibraryAssetCard` drop this lane accepted — `TimelineView.vue`
   * resolves `clientX` into a snapped `startMs` and sends the `insertClip`
   * (this component knows neither scroll position nor the label offset). */
  (e: "asset-drop", payload: { assetId: string; trackId: string; clientX: number }): void;
  /** A gap hint was pressed: close that gap on this track. */
  (e: "close-gap", payload: { trackId: string; gap: Gap }): void;
}>();

// ---- native drag-and-drop (Task 26) ----------------------------------------

const dragKind = ref<AssetKind | null>(null);
/** `null` while no drag is over this lane OR the drag would be accepted —
 * `dropRefusalReason` already returns `null` for both, so this doesn't
 * re-derive acceptance. */
const dropReason = computed(() => (dragKind.value === null ? null : dropRefusalReason(props.track, dragKind.value)));
/** The body's own `title` — the active drag's refusal reason when one is in
 * progress, else the STATIC locked-track title this lane always showed. */
const bodyTitle = computed(() => dropReason.value ?? (props.track.locked ? lockedReason(props.track.name) : undefined));

function onDragOver(event: DragEvent): void {
  const dt = event.dataTransfer;
  if (!dt) return;
  const kind = draggedAssetKind(dt);
  if (kind === null) return; // not one of ours -- leave the browser's own default (refused) behaviour
  dragKind.value = kind;
  if (trackAccepts(props.track, kind)) {
    event.preventDefault(); // only an ACCEPTED drag may actually drop
    dt.dropEffect = "copy";
  } else {
    dt.dropEffect = "none";
  }
}
function onDragLeave(): void {
  dragKind.value = null;
}
function onDrop(event: DragEvent): void {
  const dt = event.dataTransfer;
  const kind = dragKind.value;
  dragKind.value = null;
  if (!dt || kind === null || !trackAccepts(props.track, kind)) return;
  const assetId = draggedAssetId(dt, kind);
  if (assetId === null) return;
  event.preventDefault();
  emit("asset-drop", { assetId, trackId: props.track.id, clientX: event.clientX });
}

/** An accepted drag is over this lane: the accent wash (§6.4). */
const dropTarget = computed(() => dragKind.value !== null && dropReason.value === null);

/** Right-click on the header opens the track menu at the pointer. */
function onHeaderContextMenu(event: MouseEvent): void {
  emit("track-context-menu", { trackId: props.track.id, clientX: event.clientX, clientY: event.clientY });
}
/** Shift+F10 / the Menu key anywhere in the header: the menu opens under
 * the cell, and Escape gives focus back to the control that had it. */
function onHeaderKeydown(event: KeyboardEvent): void {
  if (!isContextMenuShortcut(event)) return;
  event.preventDefault();
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  emit("track-context-menu", { trackId: props.track.id, clientX: rect.left, clientY: rect.bottom });
}

/** One grid line per ruler tick, at the ruler's own step (§6.4). */
const laneStyle = computed(() => laneGridStyle(pxPerMs(props.zoom) * 1000, props.widthPx));
const laneClass = computed(() => [
  props.track.locked ? "vb-lane-locked pointer-events-none" : "",
  dropReason.value ? "cursor-not-allowed" : "",
  dropTarget.value ? "bg-accent-bg" : "",
  props.track.kind === "video" && !props.track.visible ? "*:opacity-40" : "",
]);

/** A clip answers its own right-click first (`@contextmenu.prevent`), so
 * an event that arrives here already handled was a clip's, not the lane's. */
function onLaneContextMenu(event: MouseEvent): void {
  if (event.defaultPrevented) return;
  event.preventDefault();
  emit("lane-context-menu", { trackId: props.track.id, clientX: event.clientX, clientY: event.clientY });
}

function spanOf(c: Clip): ClipSpan {
  return { start_ms: c.start_ms, in_ms: c.in_ms, out_ms: c.out_ms, speed: c.speed ?? 1 };
}

/** Stable left-to-right DOM order regardless of the (arbitrary) order
 * `visibleClips` returned them in. */
const sortedClips = computed(() => [...props.clips].sort((a, b) => a.start_ms - b.start_ms));

function kindOf(clip: Clip): "video" | "audio" {
  const a = props.assets.find((x) => x.id === clip.asset_id);
  return a?.kind === "audio" ? "audio" : "video";
}

function leftOf(clip: Clip): number {
  return msToX(clip.start_ms, props.zoom);
}

function widthOf(clip: Clip): number {
  return msToX(clipOutputEnd(spanOf(clip)), props.zoom) - leftOf(clip);
}

/** A gap hint sits 4 px in from each end of its gap (the concept's own). */
const GAP_INSET_PX = 4;
function gapStyle(gap: Gap) {
  const left = msToX(gap.start, props.zoom);
  const width = msToX(gap.end, props.zoom) - left;
  return { left: `${left + GAP_INSET_PX}px`, width: `${width - GAP_INSET_PX * 2}px` };
}
function gapLabel(gap: Gap): string {
  return `Close ${((gap.end - gap.start) / 1000).toFixed(1)} second gap on ${props.track.name}`;
}
</script>

<template>
  <div
    :data-testid="`track-lane-${track.id}`"
    class="flex min-h-[58px] w-max min-w-full border-b border-line"
    :style="{ height: `${LANE_HEIGHT_PX}px` }"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <div
      :data-testid="`track-lane-header-${track.id}`"
      class="sticky left-0 z-[15] flex shrink-0 items-center border-r border-line bg-panel px-2.5 py-2"
      :style="{ width: `${labelWidth}px` }"
      @contextmenu.prevent="onHeaderContextMenu"
      @keydown="onHeaderKeydown"
    >
      <TrackHeader
        :track="track"
        :badge="badge"
      />
    </div>

    <div
      :data-testid="`track-lane-body-${track.id}`"
      :title="bodyTitle"
      class="group/lane relative"
      :class="laneClass"
      :style="laneStyle"
      @contextmenu="onLaneContextMenu"
    >
      <span
        v-if="sortedClips.length === 0"
        :data-testid="`track-lane-empty-${track.id}`"
        class="pointer-events-none absolute top-[18px] left-4 text-[10px] text-fg-muted"
      >Drop {{ track.kind }} here · or add from Media</span>
      <button
        v-for="gap in gaps ?? []"
        :key="gap.start"
        type="button"
        :data-testid="`lane-gap-${track.id}-${gap.start}`"
        :aria-label="gapLabel(gap)"
        title="Close this gap on this track only"
        class="absolute top-4 z-[1] flex h-[25px] min-h-[25px] items-center gap-1 overflow-hidden rounded-[4px] border border-dashed border-line bg-panel p-[3px] text-[9px] whitespace-nowrap text-fg-muted opacity-16 transition-opacity duration-[120ms] group-hover/lane:opacity-100 group-focus-within/lane:opacity-100 hover:border-accent hover:bg-accent-bg hover:text-accent-ink focus-visible:opacity-100 motion-reduce:transition-none"
        :style="gapStyle(gap)"
        @click="emit('close-gap', { trackId: track.id, gap })"
      >
        <EditorIcon
          name="gap"
          :size="12"
        />Close gap
      </button>
      <ClipItem
        v-for="clip in sortedClips"
        :key="clip.id"
        :clip="clip"
        :asset-kind="kindOf(clip)"
        :selected="selectedClipIds.includes(clip.id)"
        :left-px="leftOf(clip)"
        :width-px="widthOf(clip)"
        :zoom="zoom"
        :track-index="trackIndex"
        :track-order="trackOrder"
        @context-menu="emit('context-menu', $event)"
      />
    </div>
  </div>
</template>
