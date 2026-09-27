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
 * With no peaks, the band says why only when a sentence is TRUE (Task 18
 * fix round 1, rulings T18-1/T18-2; `noPeaksNote`): a missing ffmpeg names
 * the fix and says the sound still plays (the preview needs no ffmpeg);
 * any other failure that says nothing about the sound itself says only
 * that the picture is unavailable. A missing file (the missing-media state
 * and Reconnect already say so) and an asset with no sound write nothing:
 * "audio still plays" would be false there. Every failure but the expected
 * no-ffmpeg one is logged, by code. The note is a full-colour text span
 * OUTSIDE the faded, aria-hidden bar picture, so it reads at 4.5:1 and a
 * screen reader reaches it.
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

const FFMPEG_NOTE = "Install ffmpeg to see waveforms · audio still plays";
const UNAVAILABLE_NOTE = "waveform unavailable · audio still plays";
/** Failures that say the SOUND is gone, not just its picture. */
const SILENT_CODES = new Set(["sourceMissing", "unsupportedMedia"]);

/** What the band says for a failed peaks request (`null`: nothing). */
function noPeaksNote(e: unknown): string | null {
  const code = e instanceof EditorPortError ? e.error.code : null;
  if (code === "encoderUnavailable") return FFMPEG_NOTE;
  return code !== null && SILENT_CODES.has(code) ? null : UNAVAILABLE_NOTE;
}

const editorProject = useEditorProjectStore();
const peaks = ref<number[] | null>(null);
const note = ref<string | null>(null);

async function load(): Promise<void> {
  const sessionId = editorProject.sessionId;
  const assetId = props.assetId;
  if (!sessionId) return;
  try {
    const buckets = peakBucketsFor(props.assetDurationMs);
    const result = await loadPeaks(editorProject.port, sessionId, assetId, buckets);
    if (assetId !== props.assetId) return; // a newer asset's load owns the lane now
    peaks.value = result;
    note.value = null;
  } catch (e) {
    if (assetId !== props.assetId) return;
    peaks.value = null;
    note.value = noPeaksNote(e);
    if (note.value === FFMPEG_NOTE) return;
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
  <span
    v-if="note"
    data-testid="waveform-note"
    class="absolute top-[22px] right-2 left-2 truncate text-[8px] leading-[10px] text-audio"
  >{{ note }}</span>
  <svg
    v-else-if="bars"
    class="absolute top-[18px] left-0 h-[27px] w-full opacity-65"
    :viewBox="`0 0 ${width} 27`"
    preserveAspectRatio="none"
    aria-hidden="true"
  >
    <path
      :d="bars"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
    />
  </svg>
</template>
