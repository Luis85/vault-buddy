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
 */
import { computed } from "vue";

import { LANE_HEIGHT_PX, msToX, TRACK_LABEL_WIDTH_PX } from "../../../editor/timelineLayout";
import { clipOutputEnd } from "../../../editor/timeMap";
import type { Asset, Clip, ClipSpan, Track } from "../../../editorTypes";
import ClipItem from "./ClipItem.vue";

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
}>();

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
      <span class="truncate">{{ track.name }}</span>
    </div>

    <div
      :data-testid="`track-lane-body-${track.id}`"
      class="relative bg-stage"
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
