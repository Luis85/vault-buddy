/**
 * The pure pieces the teaching-cue components loop over (Task 35; F-27–F-33):
 * `cueShapes.ts` (what each cue draws, and the shape a pointer grabs),
 * `cueDrag.ts` (a handle drag as a clamped, rounded patch) and
 * `effectFields.ts` (the inspector's per-kind fields and its output→source
 * conversion). `cueGeometry.ts` has its own suite (`cueGeometry.test.ts`).
 *
 * Fixtures are asymmetric (the "fixture flaw" rule): a speed-2 clip whose
 * start/in/out are all distinct; a 16:9 canvas so a swapped axis lands
 * somewhere else.
 */
import { describe, expect, it } from "vitest";

import { clipSpanOf } from "../src/editor/actionTargets";
import { changedPatch, dragPatch, handleSpots } from "../src/editor/cueDrag";
import { cueShapes, hitShape } from "../src/editor/cueShapes";
import { fieldsFor } from "../src/editor/effectFields";
import { sourceAtClamped } from "../src/editor/timeMap";
import type { Clip, Effect } from "../src/editorTypes";

/** Output 1000..4000 plays source 500..6500 at 2×. */
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1", asset_id: "a1", track_id: "v1", name: "Screen",
    start_ms: 1_000, in_ms: 500, out_ms: 6_500,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear",
    opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1, speed: 2,
    ...overrides,
  };
}
function effect(overrides: Partial<Effect> = {}): Effect {
  return {
    id: "e1", clip_id: "c1", kind: "highlight", start_ms: 1_500, end_ms: 2_500,
    x: 0.2, y: 0.3, w: 0.2, h: 0.1, color: "#ffd279", stroke: 4,
    ...overrides,
  };
}


const CANVAS = { width: 1280, height: 720 };

describe("cueShapes", () => {
  it("falls back to the reference defaults for every optional field", () => {
    const bare = (kind: Effect["kind"]) => ({ id: kind, clip_id: "c1", kind, start_ms: 0, end_ms: 1, x: 0.5, y: 0.5, color: "#abcdef" }) as Effect;
    const [text] = cueShapes(bare("text"), CANVAS);
    expect(text.attrs["font-size"]).toBe(32);
    expect(text.lines).toEqual([""]);
    expect(cueShapes(bare("arrow"), CANVAS)).toEqual([]); // no x2/y2: a zero-length arrow
    expect(cueShapes(bare("highlight"), CANVAS)[0].attrs["stroke-width"]).toBe(4);
    expect(cueShapes(bare("spotlight"), CANVAS)[0].attrs["fill-opacity"]).toBe(0.65);
    const step = cueShapes(bare("step"), CANVAS);
    expect(step.find((s) => s.part === "number")!.lines).toEqual(["1"]);
    expect(step.find((s) => s.part === "label")!.lines).toEqual([""]);
    expect(cueShapes(bare("mask"), CANVAS)[0].attrs).toMatchObject({ width: 0, height: 0, fill: "#abcdef" });
    expect(cueShapes(bare("zoom"), CANVAS)).toEqual([]);
  });

  it("a hit shape per kind: arrow line, zoom circle, step pill, box otherwise", () => {
    const arrow = hitShape(effect({ kind: "arrow", x: 0.1, y: 0.2, x2: 0.5, y2: 0.4, stroke: 12 }), CANVAS);
    expect(arrow).toMatchObject({ tag: "line", attrs: { x1: 128, y1: 144, x2: 640, y2: 288, "stroke-width": 36 } });
    const thin = hitShape(effect({ kind: "arrow", x2: undefined, y2: undefined, stroke: undefined }), CANVAS);
    expect(thin.attrs).toMatchObject({ x1: 256, x2: 256, "stroke-width": 24 });
    expect(hitShape(effect({ kind: "zoom" }), CANVAS)).toMatchObject({ tag: "circle", attrs: { cx: 256, cy: 216, r: 24 } });
    const step = hitShape(effect({ kind: "step", text: "abcd" }), CANVAS);
    expect(step).toMatchObject({ tag: "rect", attrs: { x: 234, y: 194, height: 45 } });
    expect(step.attrs.width).toBeCloseTo(51 + 4 * 26 * 0.55, 9);
    expect(hitShape(effect({ kind: "step", text: undefined }), CANVAS).attrs.width).toBe(51);
    expect(hitShape(effect(), CANVAS)).toMatchObject({ tag: "rect", attrs: { x: 256, y: 216, width: 256, height: 72 } });
  });
});

describe("cueDrag", () => {
  const arrow = effect({ kind: "arrow", x: 0.2, y: 0.3, x2: 0.6, y2: 0.7 });
  const box = effect({ kind: "mask", x: 0.2, y: 0.3, w: 0.5, h: 0.4 });

  it("moving an arrow shifts both ends, stopping when either reaches an edge", () => {
    expect(dragPatch(arrow, "move", 0.1, -0.1)).toEqual({ x: 0.3, y: 0.2, x2: 0.7, y2: 0.6 });
    // The tip hits the right edge at +0.4 and the tail the top edge at -0.3.
    expect(dragPatch(arrow, "move", 0.9, -0.9)).toEqual({ x: 0.6, y: 0, x2: 1, y2: 0.4 });
    const bare = { ...arrow, x2: undefined, y2: undefined };
    expect(dragPatch(bare, "move", 0.1, 0)).toEqual({ x: 0.3, y: 0.3, x2: 0.3, y2: 0.3 });
    expect(dragPatch(bare, "end", 0.1, 0.1)).toEqual({ x2: 0.3, y2: 0.4 });
  });

  it("moving a box keeps it on the canvas; moving a point clamps it", () => {
    expect(dragPatch(box, "move", 0.5, -0.5)).toEqual({ x: 0.5, y: 0 });
    // A box already hanging off the right edge can move left, not right.
    expect(dragPatch({ ...box, x: 0.8 }, "move", 0.1, 0)).toEqual({ x: 0.8, y: 0.3 });
    const point = effect({ kind: "step", x: 0.9, y: 0.1, w: undefined, h: undefined });
    expect(dragPatch(point, "move", 0.3, -0.3)).toEqual({ x: 1, y: 0 });
    expect(dragPatch(point, "focal", -2, 2)).toEqual({ x: 0, y: 1 });
  });

  it("resizing clamps to the schema floor and the canvas edge", () => {
    expect(dragPatch(box, "resize", 0.5, 0.5)).toEqual({ w: 0.8, h: 0.7 });
    expect(dragPatch(box, "resize", -1, -1)).toEqual({ w: 0.01, h: 0.01 });
    expect(dragPatch({ ...box, w: undefined, h: undefined }, "resize", 0.1, 0.1)).toEqual({ w: 0.1, h: 0.1 });
  });

  it("changedPatch drops a patch that changes nothing at the sent precision", () => {
    expect(changedPatch(box, { x: 0.2, y: 0.3 })).toBeNull();
    expect(changedPatch(box, { x: 0.2001, y: 0.3 })).toEqual({ x: 0.2001, y: 0.3 });
    expect(changedPatch({ ...box, w: undefined }, { w: 0.2 })).toEqual({ w: 0.2 });
  });

  it("handle spots: arrow ends, zoom focus, a box's corner, nothing for a step", () => {
    expect(handleSpots(arrow, CANVAS).map((s) => [s.handle, Math.round(s.cx), Math.round(s.cy)])).toEqual([
      ["start", 256, 216],
      ["end", 768, 504],
    ]);
    expect(handleSpots({ ...arrow, x2: undefined, y2: undefined }, CANVAS)[1]).toMatchObject({ cx: 256, cy: 216 });
    expect(handleSpots(effect({ kind: "zoom", w: undefined, h: undefined }), CANVAS)).toEqual([
      { handle: "focal", cx: 256, cy: 216 },
    ]);
    expect(handleSpots(box, CANVAS)).toEqual([{ handle: "resize", cx: 0.7 * 1280, cy: 0.7 * 720 }]);
    expect(handleSpots(effect({ kind: "step", w: undefined, h: undefined }), CANVAS)).toEqual([]);
  });
});

describe("effectFields", () => {
  // Fix round 1: the inspector's output->source conversion lives in
  // timeMap beside `sourceAt` (one mapping module, one rounding rule), not
  // as a second copy in effectFields.
  it("sourceAtClamped maps through the speed and clamps to the INCLUSIVE source range", () => {
    const c = clipSpanOf(clip()); // start 1000, in 500, out 6500, speed 2
    expect(sourceAtClamped(c, 1_600)).toBe(1_700);
    expect(sourceAtClamped(c, 0)).toBe(500);
    // A cue END may sit exactly on out_ms, which `sourceAt` never returns.
    expect(sourceAtClamped(c, 99_000)).toBe(6_500);
    expect(sourceAtClamped(clipSpanOf(clip({ speed: undefined })), 1_600)).toBe(1_100);
  });

  it("every kind lists its own wire props; spotlight and zoom have no colour", () => {
    const keys = (k: Effect["kind"]) => fieldsFor(k).map((f) => f.key);
    expect(keys("spotlight")).not.toContain("color");
    expect(keys("zoom")).toEqual(["x", "y", "factor", "easing"]);
    expect(keys("mask")).toEqual(["x", "y", "w", "h", "color"]);
  });
});
