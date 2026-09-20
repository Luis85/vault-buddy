<script setup lang="ts">
/**
 * Preview playback (spec 8.2).
 *
 * **This is a preview, and the UI says so.** Seeking a `<video>` element has
 * visible latency, so segment boundaries are not gapless: at each boundary
 * the element is seeked to the next segment's source start and there is a
 * hitch. Export is authoritative. Saying that plainly is the same honesty
 * posture as the transcription stats footer reporting the EFFECTIVE state
 * rather than the intended one — we do not imply frame-exact scrubbing we
 * are not building.
 */
import { ref, watch } from "vue";

import type { TimelineDto } from "../../types";
import {
  outputDurationMs,
  segmentAtOutputMs,
  toOutputMs,
  toSourceMs,
} from "../../utils/timelineGeometry";
import AppButton from "../ui/AppButton.vue";

const props = defineProps<{ src: string; timeline: TimelineDto; outputMs: number }>();
const emit = defineEmits<{ "update:outputMs": [ms: number] }>();

const video = ref<HTMLVideoElement | null>(null);
const playing = ref(false);

/** Drive the element from OUTPUT time: ask the timeline where that lands in
 * the source and seek there. A boundary crossing is a seek, not a gap. */
watch(
  () => props.outputMs,
  (ms) => {
    const src = toSourceMs(props.timeline, ms);
    const el = video.value;
    if (el === null || src === null) return;
    // A tolerance, not an equality: `timeupdate` feeds this watcher its own
    // reported position back, and seeking to a value the element is already
    // at would stall playback on every tick.
    if (Math.abs(el.currentTime * 1000 - src) > 250) el.currentTime = src / 1000;
  },
);

function onTimeUpdate() {
  // The element reports SOURCE time; every other surface speaks OUTPUT time,
  // so the raw value never leaves this component. A source moment that was
  // cut out maps to null — which is exactly the boundary case: playback has
  // run past the end of a segment into footage the edit removed, so seek to
  // wherever the NEXT segment starts instead of reporting a position the
  // strip cannot draw.
  const el = video.value;
  if (el === null) return;
  const out = toOutputMs(props.timeline, el.currentTime * 1000);
  if (out !== null) {
    emit("update:outputMs", out);
    return;
  }
  // Seek to the BOUNDARY's far side, never to `props.outputMs`. The playhead
  // is fed by this handler, so at a crossing it is always the LAST valid
  // output position — a tick behind the boundary, never at or past it.
  // `toSourceMs`-ing it therefore lands back inside the segment that just
  // ended: play, hit the boundary, seek backwards to the playhead, play a
  // quarter-second, seek backwards again. The preview could not reach a
  // second segment at all, and a trimmed tail looped instead of stopping.
  //
  // So ask which segment the playhead is in, take that segment's END on the
  // OUTPUT clock — the cumulative length through it — and map THAT. It is
  // the first output moment the ended segment does not contain, so it is the
  // next one's start, and `null` there means the output genuinely ended.
  // Deliberately not "the lowest `sourceStartMs` above the current source
  // position": that reads the SOURCE order, and a reordered timeline plays
  // its blocks in output order, so it would jump to whichever block happens
  // to sit later in the recording rather than to the one the user put next.
  // Both steps are `timelineGeometry`'s own functions — this mapping already
  // exists twice (docs/Gaps.md GAP-136) and must not gain a third copy.
  const i = segmentAtOutputMs(props.timeline, props.outputMs);
  const boundary =
    i === null ? null : outputDurationMs({ segments: props.timeline.segments.slice(0, i + 1) });
  const next = boundary === null ? null : toSourceMs(props.timeline, boundary);
  if (next === null) {
    el.pause();
    playing.value = false;
    return;
  }
  el.currentTime = next / 1000;
}

/** Scrub (spec 8.2). The range is OUTPUT milliseconds, so the control means
 * the same thing as the strip above it; the watcher does the seek. */
function onScrub(e: Event) {
  emit("update:outputMs", Number((e.target as HTMLInputElement).value));
}

function toggle() {
  const el = video.value;
  if (el === null) return;
  if (el.paused) {
    void el.play().catch(() => undefined);
    playing.value = true;
  } else {
    el.pause();
    playing.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <video
      ref="video"
      data-testid="preview-video"
      :src="src"
      class="w-full rounded-control bg-black"
      preload="metadata"
      @timeupdate="onTimeUpdate"
    />
    <div class="flex items-center gap-2">
      <AppButton
        data-testid="preview-toggle"
        variant="secondary"
        @click="toggle"
      >
        {{ playing ? "Pause" : "Play" }}
      </AppButton>
      <input
        type="range"
        data-testid="preview-scrub"
        class="grow accent-violet-500"
        min="0"
        :max="outputDurationMs(timeline)"
        step="100"
        :value="outputMs"
        aria-label="Scrub the preview"
        @input="onScrub"
      >
    </div>
    <p class="text-micro text-fg-subtle">
      Preview only — boundaries hitch while seeking. The exported file is exact.
    </p>
  </div>
</template>
