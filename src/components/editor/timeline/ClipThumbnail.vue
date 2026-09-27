<script setup lang="ts">
/**
 * A video clip's poster (Task 28): one 160 px frame of the clip's asset at
 * the clip's own first source instant, derived by Rust
 * (`editor_media_thumbnail` — quantized to 250 ms, cached in the project's
 * `cache\`, LRU-bounded there) and served through the asset protocol. The
 * path comes from Rust; this component never builds one.
 *
 * Decorative: the clip's name and kind already say what it is, so a
 * thumbnail that cannot be made (no ffmpeg, no picture, a moved file)
 * simply is not drawn — the refusal is logged, except the expected
 * no-ffmpeg case, which the waveform lane already explains.
 *
 * The fetch/error/reconnect-refresh discipline itself lives in
 * `useAssetThumbnail` (visual-parity Task 9), shared with the media
 * library's own asset-row thumbnails.
 */
import { useAssetThumbnail } from "../../../composables/useAssetThumbnail";

const props = defineProps<{ assetId: string; atMs: number }>();

const { src } = useAssetThumbnail(
  () => props.assetId,
  () => props.atMs,
);
</script>

<template>
  <img
    v-if="src"
    :src="src"
    alt=""
    draggable="false"
    class="pointer-events-none h-full w-auto object-cover opacity-50"
  >
</template>
