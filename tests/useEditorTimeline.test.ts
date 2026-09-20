import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";

import { useEditorTimeline } from "../src/composables/useEditorTimeline";
import type { TimelineDto } from "../src/types";

const WHOLE: TimelineDto = { segments: [{ sourceStartMs: 0, sourceEndMs: 10_000 }] };

type Call = { cmd: string } & Record<string, unknown>;

/** Record every IPC call, optionally rejecting the ones `fail` selects.
 *
 * The recorder is deliberately not filtered at capture time: `logWarning`
 * reaches the log plugin through this same channel under `mockIPC`, and a
 * handler that only answered `save_capture_timeline` would turn the
 * save-failure path into an unrelated second failure. */
function calls(fail?: (cmd: string) => boolean): Call[] {
  const seen: Call[] = [];
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    if (fail?.(cmd)) throw new Error("the staging sidecar could not be written");
    return undefined;
  });
  return seen;
}

/** Just the sidecar writes, in order. */
function saves(seen: Call[]): { base: unknown; timeline: TimelineDto | null }[] {
  return seen
    .filter((c) => c.cmd === "save_capture_timeline")
    .map((c) => ({ base: c.base, timeline: c.timeline as TimelineDto | null }));
}

/** The write that would be on disk. Indexed rather than `.at(-1)`: this repo
 * targets ES2021, where `Array.prototype.at` is not in `lib`. */
function lastSave(seen: Call[]): { base: unknown; timeline: TimelineDto | null } {
  const written = saves(seen);
  return written[written.length - 1];
}

function starts(t: { timeline: { value: TimelineDto } }): number[] {
  return t.timeline.value.segments.map((s) => s.sourceStartMs);
}

afterEach(() => clearMocks());

describe("useEditorTimeline", () => {
  it("splits at the playhead and can undo back to the whole capture", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    expect(t.canUndo.value).toBe(false);
    t.splitAt(4000);
    expect(t.timeline.value.segments).toHaveLength(2);
    expect(t.timeline.value.segments[1].sourceStartMs).toBe(4000);
    expect(t.canUndo.value).toBe(true);
    t.undo();
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canRedo.value).toBe(true);
    t.redo();
    expect(t.timeline.value.segments).toHaveLength(2);
  });

  // Spec 8.1: a split exactly on a boundary is a no-op, NOT a zero-length
  // segment — a zero-length segment produces an unplayable file downstream.
  // A no-op must also not push an undo entry, or Ctrl+Z appears to do
  // nothing while actually consuming a step.
  it("refuses to split on a boundary and does not record an undo step", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(0);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canUndo.value).toBe(false);
    t.splitAt(10_000);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canUndo.value).toBe(false);
    // An interior boundary of an already-split timeline is the same rule,
    // and it is the one a user actually hits: splitting twice at the same
    // playhead position must not mint an empty segment.
    t.splitAt(4000);
    t.splitAt(4000);
    expect(t.timeline.value.segments).toHaveLength(2);
  });

  // Past the end there is no segment to split. `segmentAtOutputMs` answers
  // `null` there, and a composable that ignored that would index `undefined`
  // and throw out of a click.
  it("refuses to split outside the output", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(20_000);
    t.splitAt(-1);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.canUndo.value).toBe(false);
  });

  // A new edit after an undo discards the redo branch. Keeping it would let
  // Ctrl+Y jump to a timeline that never followed from what is on screen.
  it("drops the redo branch once a new edit lands", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(3000);
    t.undo();
    expect(t.canRedo.value).toBe(true);
    t.splitAt(7000);
    expect(t.canRedo.value).toBe(false);
  });

  it("deletes a segment and refuses to delete the last one", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.deleteSegment(0);
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(t.timeline.value.segments[0].sourceStartMs).toBe(5000);
    // Spec 8.1: an empty timeline is a capture Save refuses. Do not let the
    // editor reach that state by clicking Delete one more time.
    t.deleteSegment(0);
    expect(t.timeline.value.segments).toHaveLength(1);
  });

  // An index the strip can no longer be showing (a stale selection surviving
  // a reorder, say) must not delete the wrong segment or splice nothing and
  // still record a step.
  it("ignores a delete outside the segment range", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    const before = t.canUndo.value;
    t.deleteSegment(2);
    t.deleteSegment(-1);
    expect(starts(t)).toEqual([0, 5000]);
    expect(before).toBe(true);
    expect(t.canRedo.value).toBe(false);
  });

  // D-1. `to` is a DESTINATION INDEX — where the moved segment ends up —
  // matching `core::timeline::reorder`, NOT the insertion slot `dropIndex`
  // returns. The fixture is three segments and a forward move because that
  // is the only shape that tells the two apart: under slot semantics
  // `reorder(0, 2)` would land B, A, C.
  it("reorders by destination index rather than by an insertion slot", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(3000);
    t.splitAt(6000);
    expect(starts(t)).toEqual([0, 3000, 6000]);
    t.reorder(0, 2);
    expect(starts(t)).toEqual([3000, 6000, 0]);
    t.reorder(2, 0);
    expect(starts(t)).toEqual([0, 3000, 6000]);
  });

  // The plan's own two-segment case, kept because it is the one a reviewer
  // checks by eye: under slot semantics it is a no-op instead of a swap.
  it("swaps two segments when the destination index is the other one", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.reorder(0, 1);
    expect(starts(t)).toEqual([5000, 0]);
  });

  // D-1's hazard, made executable: `dropIndex` can return `widths.length`
  // ("dropped at the end"), which is NOT a valid destination index. Passing
  // a raw slot in must be refused, not silently reinterpreted — the caller
  // converts (`to = slot > from ? slot - 1 : slot`). An out-of-range `from`
  // is worse than a no-op if unguarded: `splice` would insert `undefined`
  // and corrupt the timeline.
  it("refuses a reorder whose indices are outside the segment range", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.reorder(0, 2);
    expect(starts(t)).toEqual([0, 5000]);
    t.reorder(2, 0);
    expect(starts(t)).toEqual([0, 5000]);
    t.reorder(-1, 1);
    t.reorder(0, -1);
    expect(starts(t)).toEqual([0, 5000]);
    // The refusals recorded nothing: ONE undo step exists, the split's, so
    // undoing once is already back at the whole capture. Asserting
    // `canUndo === false` here would assert nothing — the split made it true
    // before the reorders ran.
    t.undo();
    expect(starts(t)).toEqual([0]);
    expect(t.canUndo.value).toBe(false);
    await t.flushPending();
    expect(saves(seen)).toHaveLength(2);
  });

  it("treats a reorder onto a segment's own index as a no-op", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.reorder(1, 1);
    expect(starts(t)).toEqual([0, 5000]);
    t.undo();
    expect(starts(t)).toEqual([0]);
    expect(t.canUndo.value).toBe(false);
    await t.flushPending();
    expect(saves(seen)).toHaveLength(2);
  });

  // D-2. `isDirty` MEANS "what is on screen differs from the capture this
  // editor was opened with". It is derived, not a flag, which is the whole
  // point: a flag has to be maintained by undo, redo and revert, and the
  // plan's did not maintain it in any of the three.
  it("reports dirty only while the timeline differs from the opened capture", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    expect(t.isDirty.value).toBe(false);
    t.splitAt(4000);
    expect(t.isDirty.value).toBe(true);
    t.undo();
    // Undone all the way back: nothing differs, so nothing is dirty — while
    // `canUndo` stays true, because undo and dirty measure different things
    // (there IS a step to take back: the revert of the split).
    expect(t.isDirty.value).toBe(false);
    t.redo();
    expect(t.isDirty.value).toBe(true);
    t.revert();
    expect(t.isDirty.value).toBe(false);
    expect(t.canUndo.value).toBe(true);
  });

  it("reverts to the whole capture, and reverting an untouched one does nothing", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.revert();
    expect(t.canUndo.value).toBe(false);
    t.splitAt(4000);
    t.deleteSegment(0);
    t.revert();
    expect(starts(t)).toEqual([0]);
    expect(t.timeline.value.segments[0].sourceEndMs).toBe(10_000);
  });

  // Spec 10: saved on each edit, so a crash loses at most the last one.
  it("persists every landed edit and sends the base it was opened with", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap one", WHOLE);
    t.splitAt(4000);
    await t.flushPending();
    const written = saves(seen);
    expect(written).toHaveLength(1);
    expect(written[0].base).toBe("cap one");
    expect(written[0].timeline?.segments).toHaveLength(2);
  });

  // A no-op must not write. An fsync'd sidecar rewrite per rejected click is
  // wasted disk, and it makes "saved on each edit" a claim about clicks
  // rather than about edits.
  it("does not persist an operation that changed nothing", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(0);
    await t.flushPending();
    expect(saves(seen)).toHaveLength(0);
  });

  // Revert clears the sidecar's timeline rather than storing the whole
  // capture as a one-segment edit: absent means untouched, which is what
  // phase 5's fast-path remux keys on.
  it("reverts by clearing the stored timeline", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.revert();
    await t.flushPending();
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(lastSave(seen).timeline).toBeNull();
  });

  // D-4: the same rule, reached the other way. Undoing back to the capture
  // as opened leaves exactly the state revert leaves, so it must leave the
  // same thing on disk — otherwise "absent means untouched" holds only for
  // the button and not for Ctrl+Z.
  it("clears the stored timeline when undo returns to the opened capture", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.undo();
    await t.flushPending();
    expect(saves(seen).map((s) => s.timeline === null)).toEqual([false, true]);
  });

  it("re-persists the edit when redo puts it back", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.undo();
    t.redo();
    await t.flushPending();
    expect(lastSave(seen).timeline?.segments).toHaveLength(2);
  });

  it("does nothing when undo or redo have nothing left to do", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.undo();
    t.redo();
    await t.flushPending();
    expect(starts(t)).toEqual([0]);
    expect(saves(seen)).toHaveLength(0);
  });

  // A failed save must not throw out of the edit: the edit already landed on
  // screen and undo still works, so the cost is that this one operation is
  // not crash-safe — a log, not a lost interaction, and certainly not an
  // unhandled rejection that the window's own handler would re-log.
  it("keeps the editor usable when the sidecar write fails", async () => {
    calls((cmd) => cmd === "save_capture_timeline");
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    await expect(t.flushPending()).resolves.toBeUndefined();
    expect(starts(t)).toEqual([0, 4000]);
    t.splitAt(7000);
    await t.flushPending();
    expect(starts(t)).toEqual([0, 4000, 7000]);
  });

  // The caller owns the object it hands in — `EditorRoot` builds it from a
  // command reply. If the composable held that object rather than a copy, a
  // later mutation of the reply would silently redefine what "untouched"
  // means, and revert would restore something the user never recorded.
  it("keeps its own copy of the capture it was opened with", () => {
    calls();
    const seed: TimelineDto = { segments: [{ sourceStartMs: 0, sourceEndMs: 10_000 }] };
    const t = useEditorTimeline("cap", seed);
    seed.segments[0].sourceEndMs = 1;
    seed.segments.push({ sourceStartMs: 50, sourceEndMs: 60 });
    expect(starts(t)).toEqual([0]);
    expect(t.timeline.value.segments[0].sourceEndMs).toBe(10_000);
    expect(t.isDirty.value).toBe(false);
    t.splitAt(4000);
    t.revert();
    expect(t.timeline.value.segments).toEqual([{ sourceStartMs: 0, sourceEndMs: 10_000 }]);
  });

  it("reports the output duration of the timeline on screen", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    expect(t.outputMs.value).toBe(10_000);
    t.splitAt(4000);
    t.deleteSegment(0);
    expect(t.outputMs.value).toBe(6000);
  });
});
