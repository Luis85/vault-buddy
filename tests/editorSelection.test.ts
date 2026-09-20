import { describe, expect, it } from "vitest";
import { ref } from "vue";

import { useEditorSelection } from "../src/composables/useEditorSelection";
import type { TimelineDto } from "../src/types";

/**
 * The selection/playhead remap rule on its own, away from the window.
 *
 * `EditorRoot` drives this through real components in
 * `editorEditing.test.ts` — that is where the bug was seen and where the
 * proof that Delete removes the right footage belongs. This file states the
 * rule itself: **the selection follows the footage, and is dropped only when
 * that footage is gone**, and the playhead never claims a moment the output
 * no longer has. Both are properties of the composable, not of the strip.
 */
const three: TimelineDto = {
  segments: [
    { sourceStartMs: 0, sourceEndMs: 2000 },
    { sourceStartMs: 2000, sourceEndMs: 4000 },
    { sourceStartMs: 4000, sourceEndMs: 6000 },
  ],
};

describe("useEditorSelection", () => {
  it("follows its own footage when an operation re-indexes the timeline", () => {
    const timeline = ref<TimelineDto>(three);
    const s = useEditorSelection(timeline);
    s.selected.value = 2;
    // A split ahead of the selection: the same footage, one index later.
    s.editKeepingSelection(() => {
      timeline.value = {
        segments: [
          { sourceStartMs: 0, sourceEndMs: 1000 },
          { sourceStartMs: 1000, sourceEndMs: 2000 },
          ...three.segments.slice(1),
        ],
      };
    });
    expect(s.selected.value).toBe(3);
  });

  it("drops the selection when its footage is no longer there", () => {
    const timeline = ref<TimelineDto>(three);
    const s = useEditorSelection(timeline);
    s.selected.value = 1;
    // The selected block divided in two: neither half is the block that was
    // selected, so there is nothing to follow.
    s.editKeepingSelection(() => {
      timeline.value = {
        segments: [
          three.segments[0],
          { sourceStartMs: 2000, sourceEndMs: 3000 },
          { sourceStartMs: 3000, sourceEndMs: 4000 },
          three.segments[2],
        ],
      };
    });
    expect(s.selected.value).toBeNull();
  });

  it("leaves an unselected timeline unselected", () => {
    const timeline = ref<TimelineDto>(three);
    const s = useEditorSelection(timeline);
    s.editKeepingSelection(() => {
      timeline.value = { segments: three.segments.slice(1) };
    });
    expect(s.selected.value).toBeNull();
  });

  it("clamps the playhead into an output the edit shortened, and never past 0", () => {
    const timeline = ref<TimelineDto>(three);
    const s = useEditorSelection(timeline);
    s.playheadMs.value = 5000;
    // Still inside: a clamp must not drag the playhead to the end of every
    // edit, only inside the end.
    s.editKeepingSelection(() => {
      timeline.value = { segments: three.segments.slice(0, 3) };
    });
    expect(s.playheadMs.value).toBe(5000);

    s.editKeepingSelection(() => {
      timeline.value = { segments: three.segments.slice(0, 2) };
    });
    expect(s.playheadMs.value).toBe(4000);

    // An emptied timeline has no moments at all.
    s.editKeepingSelection(() => {
      timeline.value = { segments: [] };
    });
    expect(s.playheadMs.value).toBe(0);
  });

  it("clears on delete and reseeds on a new capture", () => {
    const timeline = ref<TimelineDto>(three);
    const s = useEditorSelection(timeline);
    s.selected.value = 0;
    s.playheadMs.value = 5000;
    timeline.value = { segments: three.segments.slice(1) };
    s.clearSelection();
    expect(s.selected.value).toBeNull();
    // clearSelection clamps too: a delete shortens the output like any edit.
    expect(s.playheadMs.value).toBe(4000);

    s.selected.value = 1;
    s.playheadMs.value = 3000;
    s.resetSelection();
    expect(s.selected.value).toBeNull();
    expect(s.playheadMs.value).toBe(0);
  });
});
