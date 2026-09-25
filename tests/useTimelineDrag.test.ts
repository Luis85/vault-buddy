/**
 * `useTimelineDrag` (Task 21; F-07..F-11) — called directly, no `mount()`
 * (the `useInspectorDraft.test.ts` precedent for a composable that is just
 * refs plus pure math): every test drives the returned functions with plain
 * numbers, the same shape `ClipItem.vue`'s own pointer handlers pass in.
 */
import { describe, expect, it, vi } from "vitest";

import {
  computeFadeDrag,
  computeMoveDelta,
  computeTrimEnd,
  computeTrimStart,
  MIN_CLIP_MS,
  useTimelineDrag,
} from "../src/composables/useTimelineDrag";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { BASE_PX_PER_MS } from "../src/editor/timelineLayout";
import type { Clip } from "../src/editorTypes";

// A clip whose start/in/out are all distinct so a swapped field or a sign
// error cannot pass by coincidence (the global constraints' "fixture flaw"
// rule).
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1",
    asset_id: "a1",
    track_id: "v1",
    name: "c1",
    start_ms: 3_000,
    in_ms: 500,
    out_ms: 1_500,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0,
    y: 0,
    w: 1,
    h: 1,
    ...overrides,
  };
}

const noSnap = { snapEnabled: false, targets: [] as number[], thresholdPx: 8, zoom: 1 };
const PPM = BASE_PX_PER_MS; // zoom 1

describe("computeMoveDelta", () => {
  it("a rightward drag produces a positive delta", () => {
    const delta = computeMoveDelta(clip(), 400, noSnap);
    expect(delta).toBe(400);
  });

  // The EditorRoot drag-seam lesson (AGENTS.md, `timelineLayout.ts`'s own
  // fixture rule): test BOTH directions. Computing a delta via `Math.abs()`
  // would make this assertion fail (it would read positive) -- the exact
  // mutation this task's brief names.
  it("computes a NEGATIVE delta for a leftward raw movement", () => {
    const delta = computeMoveDelta(clip(), -400, noSnap);
    expect(delta).toBe(-400);
  });

  it("clamps so the clip's new start never goes negative", () => {
    const delta = computeMoveDelta(clip({ start_ms: 200 }), -900, noSnap);
    expect(delta).toBe(-200); // 200 + delta === 0, not negative
  });

  it("snaps the dragged edge's NEW position, not the raw delta, against a target", () => {
    // clip.start_ms 3000, dragged to roughly 3040 -- a target at 3050 is
    // within the 8px threshold at zoom 1 (8 / (0.05) = 160ms), so it snaps.
    const delta = computeMoveDelta(clip(), 40, { ...noSnap, snapEnabled: true, targets: [3050] });
    expect(delta).toBe(50); // new start lands exactly on the target (3050)
  });
});

describe("computeTrimStart", () => {
  it("moves start_ms and derives in_ms from the resulting duration (unit speed)", () => {
    // clip start 3000, in 500, out 1500 -> 1000ms duration. Trimming the
    // start handle right by 200ms shortens the clip to 800ms.
    const preview = computeTrimStart(clip(), 200, noSnap);
    expect(preview.startMs).toBe(3_200);
    expect(preview.outMs).toBe(1_500); // unchanged
    expect(preview.inMs).toBe(700); // 1500 - 800
  });

  it("clamps so the resulting duration never drops below MIN_CLIP_MS (F14)", () => {
    // The clip is 1000ms; dragging the start handle right by 950ms would
    // leave only 50ms, below the 100ms minimum.
    const preview = computeTrimStart(clip(), 950, noSnap);
    const duration = preview.outMs - preview.inMs;
    expect(duration).toBeGreaterThanOrEqual(MIN_CLIP_MS);
    expect(preview.startMs).toBe(3_000 + (1_000 - MIN_CLIP_MS));
  });

  it("clamps so start_ms never goes below 0", () => {
    // in_ms 800 leaves room to extend 800ms left, but the clip starts at
    // 100 -- the timeline's own zero is the tighter bound here.
    const preview = computeTrimStart(clip({ start_ms: 100, in_ms: 800, out_ms: 1_500 }), -500, noSnap);
    expect(preview.startMs).toBe(0);
    expect(preview.inMs).toBe(700);
  });

  // A start-handle trim keeps the clip's END fixed; it can only extend left
  // as far as the source has footage (in_ms reaching 0). Clamping in_ms
  // alone while start_ms kept moving would detach the end: a 1000ms clip
  // at 3000..4000 dragged 800ms left would send start 2200 / in 0 / out
  // 1500 -- a 1500ms clip at 2200..3700, whose end moved 300ms although the
  // user only ever touched the start handle.
  it("stops extending left once in_ms reaches 0, keeping the end fixed", () => {
    const preview = computeTrimStart(clip(), -800, noSnap);
    expect(preview).toEqual({ startMs: 2_500, inMs: 0, outMs: 1_500 });
  });

  it("stops at in_ms 0 at a non-unit speed too (source ms, not output ms)", () => {
    // speed 2: in 500 is 250 OUTPUT ms of available head room.
    const preview = computeTrimStart(clip({ speed: 2, in_ms: 500, out_ms: 2_500 }), -800, noSnap);
    expect(preview).toEqual({ startMs: 2_750, inMs: 0, outMs: 2_500 });
  });
});

describe("computeTrimEnd", () => {
  it("moves the output end and derives out_ms from the resulting duration (unit speed)", () => {
    // 1000ms duration; extending the end handle right by 500ms grows it to
    // 1500ms output duration.
    const preview = computeTrimEnd(clip(), 500, noSnap);
    expect(preview.startMs).toBe(3_000); // unchanged
    expect(preview.inMs).toBe(500); // unchanged
    expect(preview.outMs).toBe(2_000); // 500 + 1500
  });

  it("clamps so the resulting duration never drops below MIN_CLIP_MS (F14)", () => {
    // Shrinking the end handle left by 950ms would leave only 50ms.
    const preview = computeTrimEnd(clip(), -950, noSnap);
    const duration = preview.outMs - preview.inMs;
    expect(duration).toBeGreaterThanOrEqual(MIN_CLIP_MS);
    expect(preview.outMs).toBe(clip().in_ms + MIN_CLIP_MS);
  });

  it("respects a non-unit speed when deriving out_ms", () => {
    // speed 2: a 1000ms output duration maps to a 2000ms source span
    // (in 500..2500). Extending the end handle right by 100ms output ms
    // grows the source span by 200ms.
    const c = clip({ speed: 2, in_ms: 500, out_ms: 2_500 });
    const preview = computeTrimEnd(c, 100, noSnap);
    expect(preview.outMs).toBe(2_700);
  });
});

describe("useTimelineDrag — body drag", () => {
  function deps(execute: (c: EditorCommand) => void) {
    return {
      clip: () => clip(),
      zoom: () => 1,
      snapEnabled: () => false,
      snapTargets: () => [] as number[],
      moveTargetClipIds: () => ["c1"],
      trackOrder: () => ["v1", "v2"] as const,
      trackIndex: () => 0,
      trackAccepts: () => true,
      execute,
    };
  }

  it("a drag submits exactly one moveClips on pointer-up", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(200 * PPM); // ~200ms rightward
    drag.updateBodyDrag(300 * PPM); // still dragging -- more movement
    await drag.endBodyDrag(0);

    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute).toHaveBeenCalledWith({
      kind: "moveClips",
      clipIds: ["c1"],
      deltaMs: 300,
      trackId: null,
    });
    expect(drag.movePreview.value).toBeNull();
  });

  it("Escape during a drag sends nothing and restores the clip", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(200 * PPM);
    expect(drag.movePreview.value?.deltaMs).toBeGreaterThan(0);

    drag.cancelBodyDrag();

    expect(drag.movePreview.value).toBeNull();
    expect(execute).not.toHaveBeenCalled();

    // The pointer is still down after Escape: further movement and the
    // eventual pointerup (here a lane lower, too) must not resurrect the
    // drag -- neither its preview nor a command.
    drag.updateBodyDrag(400 * PPM);
    expect(drag.movePreview.value).toBeNull();
    await drag.endBodyDrag(56);
    expect(execute).not.toHaveBeenCalled();
  });

  it("a drag with no net movement sends nothing", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginBodyDrag(100, 0);
    drag.updateBodyDrag(100); // back to the start
    await drag.endBodyDrag(0);

    expect(execute).not.toHaveBeenCalled();
  });

  it("dropping on a different lane sends trackId for a single clip", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginBodyDrag(0, 0);
    // One lane down (56px) -- targetIndex 0 + 1 -> "v2".
    await drag.endBodyDrag(56);

    expect(execute).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "moveClips", trackId: "v2" }),
    );
  });

  // Fix round 1 (review Important 1; brief: "trackId when dropped on another
  // COMPATIBLE lane"). Rust refuses the WHOLE moveClips for a destination
  // of the wrong kind or a locked one -- horizontal delta included -- so a
  // slightly diagonal drag across such a lane used to lose the entire move.
  // The drop now stays on the clip's own track and keeps the delta.
  it("a drop on a lane that does not accept the clip keeps its own track and the delta", async () => {
    const execute = vi.fn();
    const refused: string[] = [];
    const drag = useTimelineDrag({
      ...deps(execute),
      trackOrder: () => ["v1", "a1"],
      trackAccepts: (id: string) => {
        refused.push(id);
        return id !== "a1";
      },
    });

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(200 * PPM);
    await drag.endBodyDrag(56); // one lane down: a1, which refuses the clip

    expect(refused).toEqual(["a1"]);
    expect(execute).toHaveBeenCalledWith({ kind: "moveClips", clipIds: ["c1"], deltaMs: 200, trackId: null });
  });

  it("never sends trackId when the drag moves more than one clip", async () => {
    const execute = vi.fn();
    const d = deps(execute);
    const drag = useTimelineDrag({ ...d, moveTargetClipIds: () => ["c1", "c2"] });

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(10 * PPM);
    await drag.endBodyDrag(56); // would otherwise cross a lane

    expect(execute).toHaveBeenCalledWith(expect.objectContaining({ trackId: null }));
  });

  // The EditorRoot drag-seam lesson, applied to the WHOLE gesture rather
  // than the pure helper above: a delta computed with Math.abs() anywhere
  // between the pointer and the command (the brief's named mutation) turns
  // a leftward drag into a rightward move. Both the body drag and the
  // end-handle trim are driven leftward and asserted against what is SENT.
  it("leftward drags produce negative deltas", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginBodyDrag(500, 0);
    drag.updateBodyDrag(500 - 250 * PPM); // 250ms to the LEFT
    await drag.endBodyDrag(0);

    drag.beginTrim("end", 500);
    drag.updateTrim(500 - 300 * PPM); // end handle 300ms to the LEFT
    await drag.endTrim();

    expect(execute.mock.calls.map((c) => c[0])).toEqual([
      { kind: "moveClips", clipIds: ["c1"], deltaMs: -250, trackId: null },
      // 1000ms clip shortened to 700ms: out 1500 -> 1200, start/in untouched.
      { kind: "trimClip", clipId: "c1", startMs: 3_000, inMs: 500, outMs: 1_200 },
    ]);
  });

  // Every EditorCommand time is an INTEGER ms (Rust decodes u64/i64): at a
  // non-integer px-per-ms a pointer offset converts to a fractional ms
  // value, which Rust's own decoder refuses outright -- the drag would
  // silently never commit at most zoom levels.
  it("sends whole milliseconds at a fractional zoom", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag({ ...deps(execute), zoom: () => 1.5 });

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(10); // 10px / 0.075 px-per-ms = 133.33ms
    await drag.endBodyDrag(0);

    drag.beginTrim("start", 0);
    drag.updateTrim(7); // 7px / 0.075 = 93.33ms
    await drag.endTrim();

    const sent = execute.mock.calls.map((c) => c[0] as Record<string, unknown>);
    expect(sent[0]).toEqual({ kind: "moveClips", clipIds: ["c1"], deltaMs: 133, trackId: null });
    expect(sent[1]).toEqual({ kind: "trimClip", clipId: "c1", startMs: 3_093, inMs: 593, outMs: 1_500 });
  });
});

describe("useTimelineDrag — trim", () => {
  function deps(execute: (c: EditorCommand) => void) {
    return {
      clip: () => clip(),
      zoom: () => 1,
      snapEnabled: () => false,
      snapTargets: () => [] as number[],
      moveTargetClipIds: () => ["c1"],
      trackOrder: () => ["v1"] as const,
      trackIndex: () => 0,
      trackAccepts: () => true,
      execute,
    };
  }

  it("a trim submits exactly one trimClip on pointer-up", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginTrim("end", 0);
    drag.updateTrim(500 * PPM);
    await drag.endTrim();

    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute).toHaveBeenCalledWith({
      kind: "trimClip",
      clipId: "c1",
      startMs: 3_000,
      inMs: 500,
      outMs: 2_000,
    });
  });

  it("Escape during a trim sends nothing and restores the clip", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginTrim("start", 0);
    drag.updateTrim(200 * PPM);
    expect(drag.trimPreview.value).not.toBeNull();

    drag.cancelTrim();

    expect(drag.trimPreview.value).toBeNull();
    drag.updateTrim(300 * PPM); // still pressed after Escape
    expect(drag.trimPreview.value).toBeNull();
    await drag.endTrim();
    expect(execute).not.toHaveBeenCalled();
  });

  it("trim preview respects the 100 ms minimum", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginTrim("start", 0);
    drag.updateTrim(950 * PPM); // would leave 50ms
    await drag.endTrim();

    const sent = execute.mock.calls[0][0] as { kind: "trimClip"; inMs: number; outMs: number };
    expect(sent.outMs - sent.inMs).toBeGreaterThanOrEqual(MIN_CLIP_MS);
  });
});

describe("trim snapping (the dragged EDGE, not the delta)", () => {
  it("snaps a start-handle trim onto a nearby target and derives in_ms from it", () => {
    // start 3000 dragged +90 -> 3090; a target at 3120 is 30ms away
    // (inside 8px = 160ms at zoom 1), so the new start lands ON it.
    const preview = computeTrimStart(clip(), 90, { ...noSnap, snapEnabled: true, targets: [3_120] });
    expect(preview).toEqual({ startMs: 3_120, inMs: 620, outMs: 1_500 });
  });

  it("snaps an end-handle trim onto a nearby target and derives out_ms from it", () => {
    // end 4000 dragged -70 -> 3930; a target at 3900 is 30ms away.
    const preview = computeTrimEnd(clip(), -70, { ...noSnap, snapEnabled: true, targets: [3_900] });
    expect(preview).toEqual({ startMs: 3_000, inMs: 500, outMs: 1_400 });
  });
});

describe("computeFadeDrag", () => {
  // clip(): 1000ms output duration (in 500, out 1500, unit speed), so the
  // half-duration limit is 500ms and the default fade_in_ms/fade_out_ms are
  // both 0 -- distinct enough that a swapped edge or sign fails outright.
  it("dragging the fade-IN handle rightward lengthens it", () => {
    expect(computeFadeDrag(clip(), "in", 200)).toBe(200);
  });

  it("dragging the fade-OUT handle LEFTWARD lengthens it -- the opposite sign from fade-in", () => {
    expect(computeFadeDrag(clip(), "out", -150)).toBe(150);
  });

  it("clamps to half the clip's own output duration", () => {
    expect(computeFadeDrag(clip(), "in", 900)).toBe(500);
  });

  it("clamps to zero rather than going negative", () => {
    expect(computeFadeDrag(clip({ fade_in_ms: 50 }), "in", -900)).toBe(0);
  });
});

describe("useTimelineDrag — fade (Task 29; F-17, F-18)", () => {
  function deps(execute: (c: EditorCommand) => void) {
    return {
      clip: () => clip(),
      zoom: () => 1,
      snapEnabled: () => false,
      snapTargets: () => [] as number[],
      moveTargetClipIds: () => ["c1"],
      trackOrder: () => ["v1"] as const,
      trackIndex: () => 0,
      trackAccepts: () => true,
      execute,
    };
  }

  it("a fade-in drag submits exactly one setFades on pointer-up", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginFade("in", 0);
    drag.updateFade(200 * PPM);
    await drag.endFade();

    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute).toHaveBeenCalledWith({ kind: "setFades", clipId: "c1", fadeInMs: 200 });
  });

  it("a fade-out drag submits fadeOutMs, sign-flipped from fade-in", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginFade("out", 500);
    drag.updateFade(500 - 150 * PPM); // 150ms to the LEFT lengthens fade-out
    await drag.endFade();

    expect(execute).toHaveBeenCalledWith({ kind: "setFades", clipId: "c1", fadeOutMs: 150 });
  });

  it("Escape during a fade drag sends nothing and restores the preview", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginFade("in", 0);
    drag.updateFade(200 * PPM);
    expect(drag.fadePreview.value).not.toBeNull();

    drag.cancelFade();

    expect(drag.fadePreview.value).toBeNull();
    drag.updateFade(300 * PPM); // still pressed after Escape
    expect(drag.fadePreview.value).toBeNull();
    await drag.endFade();
    expect(execute).not.toHaveBeenCalled();
  });

  it("an unchanged fade sends nothing", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginFade("in", 0);
    drag.updateFade(0);
    await drag.endFade();

    expect(execute).not.toHaveBeenCalled();
  });
});

describe("useTimelineDrag — degenerate input", () => {
  function deps(execute: (c: EditorCommand) => void, zoom = 1) {
    return {
      clip: () => clip(),
      zoom: () => zoom,
      snapEnabled: () => false,
      snapTargets: () => [] as number[],
      moveTargetClipIds: () => ["c1"],
      trackOrder: () => [] as string[],
      trackIndex: () => 0,
      trackAccepts: () => true,
      execute,
    };
  }

  it("a zero zoom moves and trims nothing instead of dividing by zero", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute, 0));

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(500);
    expect(drag.movePreview.value).toEqual({ deltaMs: 0 });
    await drag.endBodyDrag(0);

    drag.beginTrim("end", 0);
    drag.updateTrim(500);
    await drag.endTrim(); // unchanged span -> nothing to send

    expect(execute).not.toHaveBeenCalled();
  });

  it("moves, updates and releases without a preceding press are no-ops", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.updateBodyDrag(400);
    drag.updateTrim(400);
    await drag.endBodyDrag(56);
    await drag.endTrim();

    expect(drag.movePreview.value).toBeNull();
    expect(drag.trimPreview.value).toBeNull();
    expect(execute).not.toHaveBeenCalled();
  });

  it("a vertical drop with no known lane order never invents a trackId", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag(deps(execute));

    drag.beginBodyDrag(0, 0);
    drag.updateBodyDrag(20); // 400ms right
    await drag.endBodyDrag(112); // two lanes down, but trackOrder() is empty

    expect(execute).toHaveBeenCalledWith({ kind: "moveClips", clipIds: ["c1"], deltaMs: 400, trackId: null });
  });
});

describe("useTimelineDrag — nudge", () => {
  it("sends exactly one moveClips with the given delta", async () => {
    const execute = vi.fn();
    const drag = useTimelineDrag({
      clip: () => clip(),
      zoom: () => 1,
      snapEnabled: () => false,
      snapTargets: () => [] as number[],
      moveTargetClipIds: () => ["c1"],
      trackOrder: () => ["v1"] as const,
      trackIndex: () => 0,
      trackAccepts: () => true,
      execute,
    });

    await drag.nudge(-33);

    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute).toHaveBeenCalledWith({ kind: "moveClips", clipIds: ["c1"], deltaMs: -33, trackId: null });
  });
});
