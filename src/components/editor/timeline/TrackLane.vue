<script setup lang="ts">
/**
 * One track's row: a fixed-width label column, then a content-space clip
 * area `widthPx` wide (Task 20; F-04). `clips` arrives ALREADY filtered to
 * this track and ALREADY virtualized (`TimelineView`'s own
 * `timelineLayout.visibleClips` call, once for the whole project rather than
 * once per lane) — this component never re-derives either.
 *
 * The label column deliberately scrolls WITH the lane body rather than
 * staying pinned via `position: sticky` — a real product would pin it, but
 * `TimelineView`'s own report names this as a scoped simplification: sticky
 * layout is untestable in this Vitest environment anyway (AGENTS.md's
 * Testing conventions: happy-dom has no layout engine) and the CSS
 * interaction with a per-row flex width was real added risk for a property
 * nothing here can verify. A follow-up gap, not a hidden shortcut.
 *
 * Task 23 (F-06): the label column's plain name span is now `TrackHeader`
 * (name/eye/lock/mute/solo/volume/menu). **A locked track's clip body is
 * `pointer-events-none`** — Rust's own `ensure_unlocked`-shaped refusal
 * (`core::editor::commands::tracks`) means a drag/trim/select gesture that
 * started here would only ever end in a rejected `moveClips`/`trimClip`
 * anyway, so this stops the gesture from starting at all rather than
 * letting the user watch a preview that can never commit. The reason is
 * carried as the body's own `title`, `actionMeta.ts`'s `lockedReason` — the
 * SAME text a locked track's clip actions already show elsewhere, so a
 * locked lane never explains itself two different ways.
 *
 * **Native drag-and-drop target (Task 26)**: dropping a `LibraryAssetCard`
 * onto this lane inserts a clip at the drop's own time — `insertClip`
 * through `editorProject.execute`, owned by the parent (`TimelineView.vue`,
 * which alone knows the scroll/label offset needed to turn a `clientX` into
 * a timeline `ms`); this component only decides WHETHER the drop lands
 * (`trackCompat.trackAccepts`, the same rule `useTimelineDrag`'s cross-lane
 * clip move already applies) and shows why not. The listeners sit on the
 * ROOT element, not the (possibly `pointer-events-none`) body: a locked
 * lane's body is deliberately unreachable by a POINTER gesture (see above),
 * but a locked lane must still show ITS OWN refusal reason during a native
 * drag, and `pointer-events: none` excludes an element from drag
 * hit-testing exactly the way it excludes it from pointer hit-testing.
 */
import { computed, ref } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { LANE_HEIGHT_PX, msToX, TRACK_LABEL_WIDTH_PX } from "../../../editor/timelineLayout";
import { clipOutputEnd } from "../../../editor/timeMap";
import { draggedAssetId, draggedAssetKind, dropRefusalReason, trackAccepts } from "../../../editor/trackCompat";
import type { Asset, AssetKind, Clip, ClipSpan, Track } from "../../../editorTypes";
import ClipItem from "./ClipItem.vue";
import TrackHeader from "./TrackHeader.vue";

const props = defineProps<{
  track: Track;
  clips: Clip[];
  assets: Asset[];
  selectedClipIds: string[];
  zoom: number;
  widthPx: number;
  /** This lane's own index in the visual (top-to-bottom) track order, and
   * that same order's ids — Task 21's `useTimelineDrag` cross-track
   * drop-target hit-test, threaded straight through to each `ClipItem`
   * without this component needing to know why. */
  trackIndex: number;
  trackOrder: string[];
}>();
const emit = defineEmits<{
  (e: "context-menu", payload: { clip: Clip; clientX: number; clientY: number }): void;
  /** A `LibraryAssetCard` drop this lane accepted — `TimelineView.vue`
   * resolves `clientX` into a snapped `startMs` and sends the `insertClip`
   * (this component knows neither scroll position nor the label offset). */
  (e: "asset-drop", payload: { assetId: string; trackId: string; clientX: number }): void;
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
</script>

<template>
  <div
    :data-testid="`track-lane-${track.id}`"
    class="flex border-b border-line"
    :style="{ height: `${LANE_HEIGHT_PX}px` }"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <div
      :data-testid="`track-lane-header-${track.id}`"
      class="flex shrink-0 items-center gap-1 truncate border-r border-line bg-raised px-2 text-micro text-fg-secondary"
      :style="{ width: `${TRACK_LABEL_WIDTH_PX}px` }"
    >
      <span
        class="h-2 w-2 shrink-0 rounded-full"
        :class="track.kind === 'audio' ? 'bg-audio' : 'bg-video'"
      />
      <TrackHeader
        :track="track"
        :track-index="trackIndex"
        :track-count="trackOrder.length"
      />
    </div>

    <div
      :data-testid="`track-lane-body-${track.id}`"
      :title="bodyTitle"
      class="relative bg-stage"
      :class="[track.locked ? 'pointer-events-none' : '', dropReason ? 'cursor-not-allowed' : '']"
      :style="{ width: `${widthPx}px` }"
    >
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
