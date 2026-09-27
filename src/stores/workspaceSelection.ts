/**
 * `editorWorkspace`'s selection mutators (visual-parity Task 13; concept
 * spec §5): the clips, the teaching cue, and — new here — a selected TRACK,
 * which the inspector's "Track properties" state shows (the track header's
 * badge selects one, Task 17).
 *
 * The track selection is VIEW state only: never an edit, and never
 * persisted (the saved workspace's `selected` is Rust's closed
 * `Selected` shape, and a track is not one of its kinds). It is exclusive
 * with the other two, as in the concept (`ui.selected` holds one object):
 * selecting a track clears the clips and the cue, and any clip or cue
 * selection — clearing included — clears the track.
 */
import type { Ref } from "vue";

import type { Selected } from "../editorTypes";

interface SelectionRefs {
  selectionClipIds: Ref<string[]>;
  selected: Ref<Selected | null>;
  selectedTrackId: Ref<string | null>;
}

export function createSelection(f: SelectionRefs, persist: () => void) {
  return {
    select(ids: string[]): void {
      f.selectionClipIds.value = [...new Set(ids)];
      f.selectedTrackId.value = null;
      persist();
    },
    setSelected(next: Selected | null): void {
      f.selected.value = next;
      f.selectedTrackId.value = null;
      persist();
    },
    /** A picture click (visual-parity Task 11, D15): these clips and no
     * selected cue, in one persist. */
    selectClipsOnly(ids: string[]): void {
      f.selectionClipIds.value = [...new Set(ids)];
      f.selected.value = null;
      f.selectedTrackId.value = null;
      persist();
    },
    /** One track, and nothing else selected. */
    selectTrack(trackId: string): void {
      f.selectionClipIds.value = [];
      f.selected.value = null;
      f.selectedTrackId.value = trackId;
      persist();
    },
  };
}

/** Drops a selected track the projection no longer has (removed, or undone
 * away). */
export function pruneSelectedTrack(selectedTrackId: Ref<string | null>, trackIds: readonly string[]): void {
  if (selectedTrackId.value !== null && !trackIds.includes(selectedTrackId.value)) {
    selectedTrackId.value = null;
  }
}
