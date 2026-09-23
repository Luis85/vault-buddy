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
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { onMounted, ref, watch } from "vue";

import { loadThumbnail } from "../../../editor/mediaDerived";
import { EditorPortError } from "../../../editor/port";
import { logWarning } from "../../../logging";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{ assetId: string; atMs: number }>();

const editorProject = useEditorProjectStore();
const src = ref<string | null>(null);

async function load(): Promise<void> {
  const sessionId = editorProject.sessionId;
  const key = `${props.assetId}|${props.atMs}`;
  if (!sessionId) return;
  try {
    const path = await loadThumbnail(editorProject.port, sessionId, props.assetId, props.atMs);
    if (key !== `${props.assetId}|${props.atMs}`) return;
    src.value = convertFileSrc(path, "asset");
  } catch (e) {
    if (key !== `${props.assetId}|${props.atMs}`) return; // a newer frame owns the poster now
    src.value = null;
    if (e instanceof EditorPortError && e.error.code === "encoderUnavailable") return;
    const reason = e instanceof EditorPortError ? e.error.code : String(e);
    logWarning(`timeline: no thumbnail for asset ${props.assetId} (${reason})`);
  }
}

onMounted(load);
watch(() => [props.assetId, props.atMs, editorProject.sessionId], load);
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
