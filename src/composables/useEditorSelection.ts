import { type Ref, ref } from "vue";

import type { SegmentDto, TimelineDto } from "../types";
import { outputDurationMs } from "../utils/timelineGeometry";

/**
 * The two pieces of editor state that belong to the WINDOW rather than to the
 * timeline: which block is highlighted, and where the playhead is.
 *
 * `useEditorTimeline` knows nothing about either — it owns the segments and
 * the undo stack — but every operation it performs RE-INDEXES the array those
 * two are expressed against. That seam is where the phase's one real bug
 * lived: a split inserts a segment, so every index at or after the cut shifts
 * by one, and a selection left on its old NUMBER pointed at different footage.
 * Delete then removed a block the user had never selected, with no error and
 * no log line, and the sidecar write landed immediately (the phase review's
 * I-1). Undo and redo replace the array wholesale and had the same hole.
 *
 * So the rule lives here, in one place, stated once: **the selection follows
 * the footage, and is dropped only when that footage is gone.**
 */
export function useEditorSelection(timeline: Ref<TimelineDto>) {
  const selected = ref<number | null>(null);
  const playheadMs = ref(0);

  /** The playhead lives on the OUTPUT clock, and an edit SHORTENS that clock.
   * Left alone after a delete it sits past the end of the film: the strip
   * draws it pinned at 100%, the scrub control renders a `value` above its
   * own `max`, and the stored position claims a moment the timeline no longer
   * has. Clamped to the output duration — the scrub's own `max` — the
   * position is true again. The end itself is not a playable instant (the
   * spans are half-open), so a Split there is still the no-op spec 8.1 asks
   * for; the difference is that it is now a no-op AT A BOUNDARY the user can
   * see, rather than one caused by state that was lying. */
  function clampPlayhead() {
    playheadMs.value = Math.min(playheadMs.value, outputDurationMs(timeline.value));
  }

  /** Where in the CURRENT timeline the given span lives, or `null` if it is
   * no longer there.
   *
   * By VALUE, not by index arithmetic and not by object identity.
   * Re-deriving "the split inserted at index i" here would be a third
   * implementation of the segment algebra (there are already two, GAP-136)
   * and would have to mirror `splitAt`'s rounding and its boundary no-op
   * exactly; identity would rest silently on `useEditorTimeline` never
   * copying a segment it did not change. Looking the span back up says what
   * is actually meant. */
  function indexOf(held: SegmentDto): number | null {
    const found = timeline.value.segments.findIndex(
      (s) => s.sourceStartMs === held.sourceStartMs && s.sourceEndMs === held.sourceEndMs,
    );
    return found === -1 ? null : found;
  }

  /** Run an operation that re-indexes the timeline, keeping the selection on
   * its own footage and the playhead inside the output.
   *
   * A split that divides the SELECTED block drops the selection rather than
   * guessing at a half: neither half is the block that was selected. */
  function editKeepingSelection(op: () => void) {
    const held =
      selected.value === null ? undefined : timeline.value.segments[selected.value];
    op();
    selected.value = held === undefined ? null : indexOf(held);
    clampPlayhead();
  }

  /** Delete is the one operation that does NOT keep its selection: the
   * selected footage is exactly what was removed, so there is nothing left to
   * follow. */
  function clearSelection() {
    selected.value = null;
    clampPlayhead();
  }

  /** Back to a freshly-opened capture: nothing selected, playhead at the
   * start. */
  function resetSelection() {
    selected.value = null;
    playheadMs.value = 0;
  }

  return { selected, playheadMs, editKeepingSelection, clearSelection, resetSelection };
}
