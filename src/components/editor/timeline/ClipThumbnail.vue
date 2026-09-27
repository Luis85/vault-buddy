<script setup lang="ts">
/**
 * A video clip's filmstrip (Task 28; visual-parity Task 18, concept spec
 * §6.5 `.clip-film`): ONE 160 px frame of the clip's asset at the clip's
 * own first source instant, derived by Rust (`editor_media_thumbnail` —
 * quantized to 250 ms, cached in the project's `cache\`, LRU-bounded
 * there), served through the asset protocol and repeated across the whole
 * body by CSS at the clip's height (47 px), 32 % and desaturated. One
 * request per clip, however wide: the repeat is the browser's, never a
 * second frame. The path comes from Rust; this component never builds one.
 *
 * Decorative: the clip's name and kind already say what it is, so a
 * thumbnail that cannot be made (no ffmpeg, no picture, a moved file)
 * simply is not drawn — the refusal is logged, except the expected
 * no-ffmpeg case, which an audio clip's band already explains.
 *
 * The fetch/error/reconnect-refresh discipline itself lives in
 * `useAssetThumbnail` (visual-parity Task 9), shared with the media
 * library's own asset-row thumbnails.
 */
import { computed } from "vue";

import { useAssetThumbnail } from "../../../composables/useAssetThumbnail";

const props = defineProps<{ clipId: string; assetId: string; atMs: number }>();

const { src } = useAssetThumbnail(
  () => props.assetId,
  () => props.atMs,
);
/** The frame, repeated sideways at the clip's own height. */
const filmStyle = computed(() => ({
  backgroundImage: `url(${JSON.stringify(src.value)})`,
  backgroundSize: "auto 47px",
  backgroundRepeat: "repeat-x",
}));
</script>

<template>
  <div
    v-if="src"
    :data-testid="`clip-${clipId}-film`"
    class="absolute inset-0 opacity-32 saturate-70"
    :style="filmStyle"
  />
</template>
