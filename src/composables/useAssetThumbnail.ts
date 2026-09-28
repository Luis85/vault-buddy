/**
 * Fetches a derived-media thumbnail path for one instant of an asset
 * (visual-parity Task 9), shared by the timeline's `ClipThumbnail.vue` and
 * the media library's asset rows so the fetch/error/reconnect-refresh
 * discipline lives in ONE place -- each caller owns only its own `<img>`
 * markup and sizing. Split out of `ClipThumbnail.vue` when the library rows
 * needed the identical fetch: a second copy of its `load()` would have been
 * a clone group (`check:quality`'s own gate; the `useRovingTablist`
 * precedent, visual-parity Task 33 fix round 1). Reuses `mediaDerived.ts`'s
 * in-flight sharing and per-asset version bump; this adds no cache of its
 * own.
 *
 * `assetId`/`atMs` are getters, not plain values, so a caller whose props
 * change (a clip virtualized back into view, a library row's filtered
 * position) re-fetches through the same `watch` this used to run inline.
 * `atMs` may answer `null` -- the media library's own asset rows never ask
 * for one behind an audio or a missing asset -- which this reads as
 * "nothing to show" rather than a real instant.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { onMounted, ref, watch } from "vue";

import { loadThumbnail, mediaVersion } from "../editor/mediaDerived";
import { EditorPortError } from "../editor/port";
import { logWarning } from "../logging";
import { useEditorProjectStore } from "../stores/editorProject";

export function useAssetThumbnail(assetId: () => string, atMs: () => number | null) {
  const editorProject = useEditorProjectStore();
  const src = ref<string | null>(null);

  async function load(): Promise<void> {
    const sessionId = editorProject.sessionId;
    const id = assetId();
    const ms = atMs();
    if (ms === null) {
      src.value = null;
      return;
    }
    const key = `${id}|${ms}`;
    if (!sessionId) return;
    try {
      const path = await loadThumbnail(editorProject.port, sessionId, id, ms);
      if (key !== `${assetId()}|${atMs()}`) return; // a newer frame owns the poster now
      src.value = convertFileSrc(path, "asset");
    } catch (e) {
      if (key !== `${assetId()}|${atMs()}`) return;
      src.value = null;
      if (e instanceof EditorPortError && e.error.code === "encoderUnavailable") return;
      const reason = e instanceof EditorPortError ? e.error.code : String(e);
      logWarning(`media: no thumbnail for asset ${id} (${reason})`);
    }
  }

  onMounted(load);
  // `mediaVersion`: a reconnect replaced the asset's file (Task 40).
  watch(() => [assetId(), atMs(), editorProject.sessionId, mediaVersion(assetId())], load);

  return { src };
}
