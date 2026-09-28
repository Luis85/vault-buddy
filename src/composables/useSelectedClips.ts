/**
 * The inspector's selection scope (Task 31): the selected clips that still
 * resolve in the projection, and — when any of them sits on a locked track —
 * the reason, worded as Rust words its own refusal (`actionMeta.lockedReason`).
 * Shared by the Audio, Layout and Speed sections so the three can never
 * disagree about what the selection is or why it cannot be edited.
 */
import type { ComputedRef } from "vue";
import { computed } from "vue";

import { lockedReason } from "../editor/actionMeta";
import type { Clip } from "../editorTypes";
import { useEditorProjectStore } from "../stores/editorProject";

export interface SelectedClips {
  clips: ComputedRef<Clip[]>;
  /** The first locked track among the selection's, or `null`. */
  lockReason: ComputedRef<string | null>;
}

export function useSelectedClips(clipIds: () => string[]): SelectedClips {
  const editorProject = useEditorProjectStore();
  const clips = computed(() =>
    clipIds()
      .map((id) => editorProject.clipById(id))
      .filter((c): c is Clip => c !== undefined),
  );
  const lockReason = computed(() => {
    const tracks = editorProject.project?.tracks ?? [];
    const locked = clips.value.map((c) => tracks.find((t) => t.id === c.track_id)).find((t) => t?.locked);
    return locked ? lockedReason(locked.name) : null;
  });
  return { clips, lockReason };
}
