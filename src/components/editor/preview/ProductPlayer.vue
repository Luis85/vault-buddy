<script setup lang="ts">
/**
 * Plays a RENDERED file (Task 47; F-42; SCREENS 09: "Watch rendered file
 * plays actual encoded media"): a Rendered Product (`{ productId }`) or a
 * Review render (`{ reviewJobId }`), in a plain `<video>` with the
 * browser's own controls.
 *
 * Deliberately NOT the editable preview: `PreviewSurface` composes the
 * project's layers live in the webview, an approximation of the render
 * (ADR GAP-N3). This plays the file ffmpeg actually wrote, located only
 * through `editor_media_url` — the frontend never builds a path — and
 * served by the pinned asset scope (R7: `products\` and `cache\`).
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { ref, watch } from "vue";

import type { MediaRef } from "../../../editorTypes";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{ media: MediaRef; label: string }>();

const editorProject = useEditorProjectStore();
const src = ref<string | null>(null);
const problem = ref<string | null>(null);

/** Which file is asked for — a string, so a parent re-rendering the same
 * reference as a fresh object literal does not re-resolve it. */
const key = (media: MediaRef) => JSON.stringify(media);

async function resolve(media: MediaRef): Promise<void> {
  src.value = null;
  problem.value = null;
  const sessionId = editorProject.sessionId;
  if (!sessionId) return;
  try {
    const path = await editorProject.port.mediaUrl(sessionId, media);
    if (key(media) === key(props.media)) src.value = convertFileSrc(path, "asset");
  } catch (e) {
    const error = toEditorError(e);
    if (key(media) !== key(props.media)) return;
    problem.value =
      error.code === "sourceMissing" ? "The rendered file is no longer on disk." : error.message;
  }
}

watch(
  () => key(props.media),
  () => void resolve(props.media),
  { immediate: true },
);
</script>

<template>
  <div
    data-testid="product-player"
    class="flex flex-col gap-1"
  >
    <video
      v-if="src"
      data-testid="product-player-video"
      :src="src"
      :aria-label="label"
      controls
      preload="metadata"
      class="max-h-[50vh] w-full rounded-control bg-black"
    />
    <p
      v-else-if="problem"
      role="alert"
      data-testid="product-player-problem"
      class="text-xs text-danger-fg"
    >
      {{ problem }}
    </p>
    <p
      v-else
      class="text-xs text-fg-subtle"
    >
      Finding the rendered file…
    </p>
  </div>
</template>
