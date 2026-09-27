<script setup lang="ts">
/**
 * One audio clip's waveform (Task 28; F-26; visual-parity Task 18, concept
 * spec §6.5): the peaks Rust derived for the clip's ASSET
 * (`editor_media_peaks`, via the `mediaDerived` memo, so every clip of one
 * recording shares one decode), drawn as the concept's rounded bars over
 * the clip's own source range (`waveform.waveformBars`) in a 27 px band
 * 18 px from the clip's top, at 65 %, in the clip's own colour.
 *
 * It exists only while its `ClipItem` does, and `TrackLane` renders only
 * the clips `timelineLayout.visibleClips` keeps — so a clip scrolled far
 * off screen never asks for a decode at all.
 *
 * A missing ffmpeg is not an empty band: `encoderUnavailable` says so in
 * the concept's words, "waveform unavailable · audio still plays" (the
 * preview plays the sound without ffmpeg). Any other refusal (a moved
 * file, an asset with no sound) draws nothing and is logged — the audio
 * would NOT play there, so the sentence would be untrue.
 */
import { computed, onMounted, ref, watch } from "vue";

import { loadPeaks, mediaVersion } from "../../../editor/mediaDerived";
import { EditorPortError } from "../../../editor/port";
import { peakBucketsFor, waveformBars } from "../../../editor/waveform";
import { logWarning } from "../../../logging";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{
  assetId: string;
  assetDurationMs: number;
  inMs: number;
  outMs: number;
  widthPx: number;
}>();

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
// `mediaVersion`: a reconnect replaced the asset's file (Task 40).
watch(() => [props.assetId, props.assetDurationMs, editorProject.sessionId, mediaVersion(props.assetId)], load);

const width = computed(() => Math.max(props.widthPx, 1));
const bars = computed(() =>
  peaks.value ? waveformBars(peaks.value, props.assetDurationMs, props.inMs, props.outMs, width.value) : "",
);
</script>

<template>
  <svg
    v-if="ffmpegMissing || bars"
    class="absolute top-[18px] left-0 h-[27px] w-full opacity-65"
    :viewBox="`0 0 ${width} 27`"
    preserveAspectRatio="none"
    aria-hidden="true"
  >
    <text
      v-if="ffmpegMissing"
      x="8"
      y="18"
      fill="currentColor"
      font-size="8"
    >waveform unavailable · audio still plays</text>
    <path
      v-else
      :d="bars"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
    />
  </svg>
</template>
