<script setup lang="ts">
/**
 * One audio clip's waveform (Task 28; F-26): the peaks Rust derived for
 * the clip's ASSET (`editor_media_peaks`, via the `mediaDerived` memo, so
 * every clip of one recording shares one decode), drawn as ONE SVG
 * polyline over the clip's own source range (`waveform.waveformPoints`).
 *
 * It exists only while its `ClipItem` does, and `TrackLane` renders only
 * the clips `timelineLayout.visibleClips` keeps — so a clip scrolled far
 * off screen never asks for a decode at all.
 *
 * A missing ffmpeg is not an empty lane: `encoderUnavailable` shows the
 * install hint (R20 — nothing is faked, and nothing silently absent). Any
 * other refusal (a moved file, an asset with no sound) draws nothing and is
 * logged, rather than wrongly telling the user to install ffmpeg.
 */
import { computed, onMounted, ref, watch } from "vue";

import { loadPeaks } from "../../../editor/mediaDerived";
import { EditorPortError } from "../../../editor/port";
import { peakBucketsFor, waveformPoints } from "../../../editor/waveform";
import { logWarning } from "../../../logging";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{
  assetId: string;
  assetDurationMs: number;
  inMs: number;
  outMs: number;
  widthPx: number;
}>();

/** The lane's drawing height — `ClipItem`'s `h-3` slot. */
const HEIGHT_PX = 12;

const editorProject = useEditorProjectStore();
const peaks = ref<number[] | null>(null);
const ffmpegMissing = ref(false);

async function load(): Promise<void> {
  const sessionId = editorProject.sessionId;
  const assetId = props.assetId;
  if (!sessionId) return;
  try {
    const buckets = peakBucketsFor(props.assetDurationMs);
    const result = await loadPeaks(editorProject.port, sessionId, assetId, buckets);
    if (assetId !== props.assetId) return; // a newer asset's load owns the lane now
    peaks.value = result;
    ffmpegMissing.value = false;
  } catch (e) {
    if (assetId !== props.assetId) return;
    peaks.value = null;
    if (e instanceof EditorPortError && e.error.code === "encoderUnavailable") {
      ffmpegMissing.value = true;
      return;
    }
    const reason = e instanceof EditorPortError ? e.error.code : String(e);
    logWarning(`timeline: no waveform for asset ${assetId} in session ${sessionId} (${reason})`);
  }
}

onMounted(load);
watch(() => [props.assetId, props.assetDurationMs, editorProject.sessionId], load);

const points = computed(() =>
  peaks.value
    ? waveformPoints(peaks.value, props.assetDurationMs, props.inMs, props.outMs, props.widthPx, HEIGHT_PX)
    : "",
);
</script>

<template>
  <span
    v-if="ffmpegMissing"
    class="block truncate text-micro leading-3 text-fg-subtle"
  >Install ffmpeg to see waveforms</span>
  <svg
    v-else-if="points"
    class="block h-full w-full"
    :viewBox="`0 0 ${Math.max(widthPx, 1)} ${HEIGHT_PX}`"
    preserveAspectRatio="none"
    aria-hidden="true"
  >
    <polyline
      :points="points"
      class="fill-audio/40 stroke-audio"
      stroke-width="1"
      vector-effect="non-scaling-stroke"
    />
  </svg>
</template>
