import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";

import { useEditorTimeline } from "../src/composables/useEditorTimeline";
import type { TimelineDto } from "../src/types";

const WHOLE: TimelineDto = { segments: [{ sourceStartMs: 0, sourceEndMs: 10_000 }] };

/** A capture RESUMED from a sidecar a previous session wrote: spec 10's
 * Resume, which `EditorRoot` reaches through `loaded.timeline ?? whole`. The
 * first 4 s were cut last time. This is the shape in which "the timeline I
 * was handed" and "the whole recording" are DIFFERENT things, and it is the
 * only shape that can tell the two apart. */
const RESUMED: TimelineDto = { segments: [{ sourceStartMs: 4000, sourceEndMs: 10_000 }] };

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
  //
  // I-6: this sampled `canUndo` BEFORE the deletes and asserted the sample,
  // which is an assertion about the SETUP's split — true whether or not the
  // deletes recorded a step of their own. The step count is only observable
  // by spending the steps: one undo must land back at the whole capture,
  // which it cannot do if either refusal pushed an entry.
  it("ignores a delete outside the segment range", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(5000);
    t.deleteSegment(2);
    t.deleteSegment(-1);
    expect(starts(t)).toEqual([0, 5000]);
    expect(t.canRedo.value).toBe(false);
    t.undo();
    expect(starts(t)).toEqual([0]);
    expect(t.canUndo.value).toBe(false);
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

  // C-1/m-3. Split out of a test that bundled this with the untouched no-op
  // below, and RENAMED: revert goes back to the capture as this editor
  // OPENED it. On a capture opened unedited that is the whole recording —
  // which is this fixture — and the resumed case, where the two differ, is
  // the separate fixture further down.
  it("reverts to the capture it was opened with", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.deleteSegment(0);
    t.revert();
    expect(starts(t)).toEqual([0]);
    expect(t.timeline.value.segments[0].sourceEndMs).toBe(10_000);
  });

  it("does nothing when reverting a capture nothing has changed", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.revert();
    expect(t.canUndo.value).toBe(false);
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

  // C-1. Revert persists the timeline it restored, like every other
  // operation. It used to write `null` instead, on the rule "absent means
  // untouched" — a rule this side cannot evaluate, since it is never told
  // the source duration. Phase 5 asks `is_untouched(source_duration_ms)`,
  // which answers the same for a stored whole-capture timeline as for an
  // absent one, so the fast path is unaffected either way.
  it("reverts by storing the timeline it restored, never by clearing it", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.revert();
    await t.flushPending();
    expect(t.timeline.value.segments).toHaveLength(1);
    expect(lastSave(seen).timeline).toEqual(WHOLE);
  });

  // D-4: the same rule, reached the other way. Undoing back to the capture as
  // opened leaves exactly the state revert leaves, so it must leave the same
  // thing on disk.
  it("stores the restored timeline when undo returns to the opened capture", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.undo();
    await t.flushPending();
    expect(saves(seen).map((w) => w.timeline)).toEqual([
      { segments: [{ sourceStartMs: 0, sourceEndMs: 4000 }, { sourceStartMs: 4000, sourceEndMs: 10_000 }] },
      WHOLE,
    ]);
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

  // C-1, the whole point of this wave. Session 1 trimmed the first 4 s and
  // closed; session 2 resumes, edits, and takes the edit back. The sidecar
  // must still describe the trim. It used to be handed `null` here — "this
  // capture was never touched" — and phase 5's fast path would have remuxed
  // the whole recording, putting back the 4 s the user cut, with nothing on
  // screen changing and nothing in the log.
  //
  // The fixture asserts the WRITE, not the screen: the screen was always
  // right, and that is exactly what made the bug invisible.
  it("keeps a resumed edit on disk when undo returns to it", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", RESUMED);
    t.splitAt(1000);
    t.undo();
    await t.flushPending();
    expect(starts(t)).toEqual([4000]);
    expect(lastSave(seen).timeline).toEqual(RESUMED);
  });

  // The same erasure by the other route (`revert`), and the fixture that
  // makes this verb's NAME true: it goes back to the capture as opened —
  // here a previous session's trim — not back to the whole recording.
  it("reverts a resumed capture to the saved edit, not to the whole recording", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", RESUMED);
    t.splitAt(1000);
    t.revert();
    await t.flushPending();
    expect(starts(t)).toEqual([4000]);
    expect(lastSave(seen).timeline).toEqual(RESUMED);
  });

  // I-2. `splitAt` is the sole producer of the persisted timeline, and
  // `Segment`'s fields are `u64` in the Rust twin. The preview derives the
  // playhead from `el.currentTime * 1000`, a double in seconds, so a
  // fractional millisecond is the NORMAL case rather than an edge one; an
  // unrounded cut writes a boundary `serde_json` cannot read back as `u64`.
  it("cuts on whole milliseconds, whatever the playhead carries", async () => {
    const seen = calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4.0333333 * 1000);
    expect(t.timeline.value.segments).toEqual([
      { sourceStartMs: 0, sourceEndMs: 4033 },
      { sourceStartMs: 4033, sourceEndMs: 10_000 },
    ]);
    await t.flushPending();
    expect(lastSave(seen).timeline).toEqual(t.timeline.value);
  });

  // I-2's second half, and the reason rounding belongs at the ENTRY rather
  // than on the cut: a playhead a fraction below an existing boundary must
  // reach the boundary no-op, not sneak past it. Unrounded, this produced
  // `[{0, 2999.9999}, {2999.9999, 3000}, {3000, 10000}]` — a 0.0001 ms
  // segment, i.e. the unplayable zero-length segment spec 8.1's no-op
  // exists to prevent, rounded into existence downstream.
  it("treats a playhead a fraction off a boundary as being on it", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(3000);
    t.splitAt(2999.9999);
    expect(t.timeline.value.segments).toEqual([
      { sourceStartMs: 0, sourceEndMs: 3000 },
      { sourceStartMs: 3000, sourceEndMs: 10_000 },
    ]);
  });

  // I-3. Undo is a STACK. With one entry on it, popping the newest and
  // popping the oldest are the same operation, and every other fixture in
  // this file has exactly one — so the module's first documented property
  // was unpinned. Two edits, two undos, newest first.
  it("takes back two edits one at a time, newest first", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(3000);
    t.splitAt(6000);
    expect(starts(t)).toEqual([0, 3000, 6000]);
    t.undo();
    expect(starts(t)).toEqual([0, 3000]);
    t.undo();
    expect(starts(t)).toEqual([0]);
  });

  // I-3, the redo half. Same reasoning, same shape: the redo stack is only
  // distinguishable from a queue once it holds two entries.
  it("puts two undone edits back one at a time, oldest first", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(3000);
    t.splitAt(6000);
    t.undo();
    t.undo();
    t.redo();
    expect(starts(t)).toEqual([0, 3000]);
    t.redo();
    expect(starts(t)).toEqual([0, 3000, 6000]);
  });

  // I-4. The saves are CHAINED, and every other fixture here answers IPC
  // synchronously — under which unchained writes still land in issue order,
  // so the suite could not tell a chain from no chain at all. The target is
  // one sidecar rewritten through a replacing rename, so a write that lands
  // out of order is permanent.
  //
  // Two SPLITS, not an edit and its undo: the two timelines then differ in
  // segment count and neither equals the capture as opened, so this fixture
  // measures the ordering and nothing else. (An undo here would also be
  // reading the "what does a return to the opened capture write" rule, and a
  // fixture that trips two guards proves nothing about the one it names.)
  it("lands the writes in the order they were issued, not in the order they finish", async () => {
    const landed: number[] = [];
    let first = true;
    mockIPC((cmd, args) => {
      if (cmd !== "save_capture_timeline") return undefined;
      const { timeline } = args as { timeline: TimelineDto };
      const delay = first ? 40 : 0;
      first = false;
      return new Promise((resolve) => {
        setTimeout(() => {
          landed.push(timeline.segments.length);
          resolve(undefined);
        }, delay);
      });
    });
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.splitAt(7000);
    await t.flushPending();
    // Wait out the slow write as well. `flushPending` already covers it when
    // the chain is real; without the chain it is still in flight, and this
    // makes the failure read as the INVERSION it is rather than as a missing
    // entry.
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(landed).toEqual([2, 3]);
  });

  // I-5. `sameTimeline` compares BOTH ends of every segment, and the
  // `sourceEndMs` half was unpinned because no fixture ever deleted a
  // segment at an index other than 0 — so no fixture ever produced two
  // timelines whose segments start alike and end differently. Trimming dead
  // time off the END is the likeliest screen-capture edit of all, and it is
  // exactly that shape: `[{0,4000}]` against the opened `[{0,10000}]`.
  it("sees a tail trim as a change, not as the capture it was opened with", () => {
    calls();
    const t = useEditorTimeline("cap", WHOLE);
    t.splitAt(4000);
    t.deleteSegment(1);
    expect(starts(t)).toEqual([0]);
    expect(t.isDirty.value).toBe(true);
  });

  // I-7. A failed sidecar write means spec 10's "a crash loses at most the
  // last operation" no longer holds, and only the user can do anything
  // about it (a full disk). The log alone left it invisible — nothing in
  // the returned surface said so, so no window could render it.
  it("reports a failed save, and stops reporting it once one succeeds", async () => {
    let failing = true;
    mockIPC((cmd) => {
      if (cmd === "save_capture_timeline" && failing) throw new Error("no space left on device");
      return undefined;
    });
    const t = useEditorTimeline("cap", WHOLE);
    expect(t.saveFailed.value).toBe(false);
    t.splitAt(4000);
    await t.flushPending();
    expect(t.saveFailed.value).toBe(true);
    failing = false;
    t.splitAt(7000);
    await t.flushPending();
    expect(t.saveFailed.value).toBe(false);
  });

});
