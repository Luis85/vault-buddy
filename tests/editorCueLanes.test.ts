/**
 * The Teaching layers row's pure half (visual-parity Task 19; concept spec
 * §6.4, design D11): packing cues into rows 22 px apart, the row's height,
 * where each cue paints on the OUTPUT timeline and what it reads, and the
 * move/trim/nudge arithmetic a cue drag sends as one `updateEffect`.
 */
import { describe, expect, it } from "vitest";

import {
  cueMoveRange,
  cueNudgeRange,
  cueTrimRange,
} from "../src/composables/useCueDrag";
import { SNAP_THRESHOLD_PX } from "../src/composables/useTimelineDrag";
import { cueLabel, cueTop, laneCount, packCues, teachingCues, teachingRowHeight } from "../src/editor/cueLanes";
import type { Clip, Effect } from "../src/editorTypes";
import { clip, project } from "./helpers/inspectorProject";

function effect(id: string, clipId: string, start: number, end: number, extra: Partial<Effect> = {}): Effect {
  return { id, clip_id: clipId, kind: "text", start_ms: start, end_ms: end, x: 0.5, y: 0.5, color: "#fff", ...extra };
}

const NO_SNAP = { snapEnabled: false, targets: [], thresholdPx: SNAP_THRESHOLD_PX, zoom: 1 };

describe("packCues (§6.4: greedy by start, then id)", () => {
  it("two overlapping cues get lanes 0 and 1; a third that starts after the first ends gets lane 0", () => {
    const packed = packCues([
      { id: "a", startMs: 0, endMs: 5_000 },
      { id: "b", startMs: 2_000, endMs: 8_000 },
      { id: "c", startMs: 5_000, endMs: 6_000 },
    ]);
    expect(packed).toEqual([
      { id: "a", lane: 0 },
      { id: "b", lane: 1 },
      { id: "c", lane: 0 },
    ]);
  });

  it("orders by start, then by id, whatever order the cues arrive in", () => {
    const packed = packCues([
      { id: "z", startMs: 1_000, endMs: 2_000 },
      { id: "b", startMs: 0, endMs: 3_000 },
      { id: "a", startMs: 0, endMs: 3_000 },
    ]);
    expect(packed).toEqual([
      { id: "a", lane: 0 },
      { id: "b", lane: 1 },
      { id: "z", lane: 2 },
    ]);
  });

  it("a cue goes to the FIRST lane that is free, not the last one used", () => {
    const packed = packCues([
      { id: "a", startMs: 0, endMs: 1_000 },
      { id: "b", startMs: 0, endMs: 9_000 },
      { id: "c", startMs: 0, endMs: 2_000 },
      { id: "d", startMs: 2_500, endMs: 3_000 },
    ]);
    expect(packed.find((p) => p.id === "d")).toEqual({ id: "d", lane: 0 });
    expect(laneCount(packed)).toBe(3);
    expect(laneCount([])).toBe(0);
  });
});

describe("the row's geometry", () => {
  it("is max(44, lanes*22+5) tall", () => {
    expect([0, 1, 2, 3].map(teachingRowHeight)).toEqual([44, 44, 49, 71]);
  });

  it("puts a cue at lane*22+3", () => {
    expect([0, 1, 2].map(cueTop)).toEqual([3, 25, 47]);
  });
});

describe("teachingCues: where each cue paints and what it reads", () => {
  it("maps source time to OUTPUT time at the clip's speed, packs, and drops a cue outside its clip", () => {
    const p = project({
      clips: [
        clip("c1", "capture", "v1", 1_000, 10_000, "One", { in_ms: 2_000, speed: 2 }),
        clip("c2", "capture", "v1", 20_000, 5_000, "Two"),
      ],
      effects: [
        effect("fx1", "c1", 4_000, 8_000, { text: "Hello" }),
        effect("fx2", "c2", 0, 1_000, { kind: "arrow" }),
        effect("fx3", "c2", 9_000, 9_500),
        effect("fx4", "gone", 0, 1_000),
      ],
    });
    const cues = teachingCues(p);
    expect(cues.map((c) => [c.effect.id, c.startMs, c.endMs, c.lane, c.label])).toEqual([
      ["fx1", 2_000, 4_000, 0, "Hello"],
      ["fx2", 20_000, 21_000, 0, "Arrow"],
    ]);
    expect(cues[0].clip.id).toBe("c1");
    expect(teachingCues(null)).toEqual([]);
  });

  it("a zoom reads its factor and Focus; a cue with no text reads its kind", () => {
    expect(cueLabel(effect("z", "c", 0, 1, { kind: "zoom", factor: 1.65 }))).toBe("1.65× Focus");
    expect(cueLabel(effect("z", "c", 0, 1, { kind: "zoom" }))).toBe("Zoom");
    expect(cueLabel(effect("s", "c", 0, 1, { kind: "step" }))).toBe("Numbered step");
    expect(cueLabel(effect("m", "c", 0, 1, { kind: "mask" }))).toBe("Privacy cover");
    expect(cueLabel(effect("t", "c", 0, 1, { text: "Ready to find." }))).toBe("Ready to find.");
  });
});

// A clip at output 1 s, playing source [2 s, 12 s) at 2x: output [1 s, 6 s).
const fast: Clip = clip("c1", "capture", "v1", 1_000, 10_000, "One", { in_ms: 2_000, out_ms: 12_000, speed: 2 });
const cue = effect("fx1", "c1", 4_000, 8_000); // output [2 s, 4 s)

describe("cueMoveRange: one updateEffect's source range for a body drag", () => {
  it("shifts the source range by the OUTPUT delta times the clip's speed", () => {
    expect(cueMoveRange(fast, cue, 500, NO_SNAP).range).toEqual({ startMs: 5_000, endMs: 9_000 });
    expect(cueMoveRange(fast, cue, -1_000, NO_SNAP).range).toEqual({ startMs: 2_000, endMs: 6_000 });
  });

  it("stops at the clip's own source range at either end, keeping the cue's length", () => {
    expect(cueMoveRange(fast, cue, -60_000, NO_SNAP).range).toEqual({ startMs: 2_000, endMs: 6_000 });
    expect(cueMoveRange(fast, cue, 60_000, NO_SNAP).range).toEqual({ startMs: 8_000, endMs: 12_000 });
  });

  it("snaps the cue's start to a target the clip drag would use, and reports the guide", () => {
    const opts = { ...NO_SNAP, snapEnabled: true, targets: [3_000] };
    // Raw output start 2 s + 950 ms = 2.95 s: 2.5 px from 3 s at zoom 1.
    const step = cueMoveRange(fast, cue, 950, opts);
    expect(step.range).toEqual({ startMs: 6_000, endMs: 10_000 });
    expect(step.guide).toBe(3_000);
    expect(cueMoveRange(fast, cue, 950, NO_SNAP).guide).toBeNull();
  });
});

describe("cueTrimRange: an edge grip moves one end only", () => {
  it("the start grip moves the start and keeps the end", () => {
    expect(cueTrimRange(fast, cue, "start", -500, NO_SNAP).range).toEqual({ startMs: 3_000, endMs: 8_000 });
  });

  it("the end grip moves the end and keeps the start", () => {
    expect(cueTrimRange(fast, cue, "end", 1_000, NO_SNAP).range).toEqual({ startMs: 4_000, endMs: 10_000 });
  });

  it("never crosses the other end (100 ms of output stays) or leaves the clip", () => {
    expect(cueTrimRange(fast, cue, "start", 60_000, NO_SNAP).range).toEqual({ startMs: 7_800, endMs: 8_000 });
    expect(cueTrimRange(fast, cue, "end", -60_000, NO_SNAP).range).toEqual({ startMs: 4_000, endMs: 4_200 });
    expect(cueTrimRange(fast, cue, "start", -60_000, NO_SNAP).range).toEqual({ startMs: 2_000, endMs: 8_000 });
    expect(cueTrimRange(fast, cue, "end", 60_000, NO_SNAP).range).toEqual({ startMs: 4_000, endMs: 12_000 });
  });
});

describe("cueNudgeRange: an arrow key's move", () => {
  it("moves by the output step times the speed, clamped to the clip", () => {
    expect(cueNudgeRange(fast, cue, 33)).toEqual({ startMs: 4_066, endMs: 8_066 });
    expect(cueNudgeRange(fast, cue, 60_000)).toEqual({ startMs: 8_000, endMs: 12_000 });
  });
});
