/**
 * `src/editor/cueGeometry.ts` (Task 35; F-27–F-33): which teaching cues are
 * on screen at an output instant, the arrow's polygons, the spotlight's dim
 * rectangles and the zoom's stage transform — all pure, all in the numbers
 * the render (Tasks 42/43) must reproduce.
 *
 * Fixtures are asymmetric (the "fixture flaw" rule): a speed-2 clip whose
 * start/in/out are all distinct, a focal point in a CORNER (x and y at
 * different edges), non-square canvases in the arrow table.
 */
import { describe, expect, it } from "vitest";

import type { ActiveCue } from "../src/editor/cueGeometry";
import {
  activeCues,
  activeZoom,
  arrowPath,
  IDENTITY_ZOOM,
  spotlightRects,
  unzoomPoint,
  zoomTransform,
} from "../src/editor/cueGeometry";
import type { Clip, Effect, Project, Track } from "../src/editorTypes";
import rawArrows from "./fixtures/editor-arrow-cases.json";

// ---- fixtures ---------------------------------------------------------------

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
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
function project(effects: Effect[], overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "p",
    title: "T",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "a1", kind: "video", name: "a.mp4", duration_ms: 10_000 }],
    tracks: [track("v1")],
    clips: [clip()],
    effects,
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "v", folder: "", dated: false },
    ...overrides,
  };
}

// ---- activeCues --------------------------------------------------------------

describe("activeCues", () => {
  // Source 1500..2500 on a 2× clip that starts at output 1000 with in 500
  // plays at output 1000 + (1500-500)/2 = 1500 .. 1000 + 2000/2 = 2000.
  it("maps a source-time cue through the clip's speed, half-open", () => {
    const p = project([effect()]);
    expect(activeCues(p, 1_499)).toEqual([]);
    const [cue] = activeCues(p, 1_500);
    expect(cue.effect.id).toBe("e1");
    expect([cue.startMs, cue.endMs]).toEqual([1_500, 2_000]);
    expect(activeCues(p, 1_999)).toHaveLength(1);
    expect(activeCues(p, 2_000)).toEqual([]);
    // A reading that ignored the speed would have it on at 2400 (1000+1400).
    expect(activeCues(p, 2_400)).toEqual([]);
  });

  it("a cue on a hidden track, an audio track or a dangling clip never paints", () => {
    const hidden = project([effect()], { tracks: [track("v1", { visible: false })] });
    expect(activeCues(hidden, 1_600)).toEqual([]);
    const audio = project([effect()], { tracks: [track("v1", { kind: "audio" })] });
    expect(activeCues(audio, 1_600)).toEqual([]);
    expect(activeCues(project([effect({ clip_id: "gone" })]), 1_600)).toEqual([]);
    expect(activeCues(null, 1_600)).toEqual([]);
  });

  it("keeps the project's own order", () => {
    const p = project([effect({ id: "b" }), effect({ id: "a", start_ms: 1_000 })]);
    expect(activeCues(p, 1_700).map((c) => c.effect.id)).toEqual(["b", "a"]);
  });
});

// ---- arrowPath ---------------------------------------------------------------

interface ArrowCase {
  name: string;
  canvas: { width: number; height: number };
  x: number;
  y: number;
  x2: number;
  y2: number;
  stroke: number;
  expected: { shaft: [number, number][]; head: [number, number][] } | null;
}
const ARROWS = (rawArrows as unknown as { cases: ArrowCase[] }).cases;

describe("arrowPath", () => {
  // Named test (brief). The same file is read by Task 43's ass.rs, so the
  // preview's SVG arrow and the rendered ASS arrow cannot drift apart.
  it("arrow path matches the shared fixture", () => {
    // Size guard: a table that silently shrinks proves nothing (the
    // timeline-cases.json discipline). Re-point this number, never drop it.
    expect(ARROWS).toHaveLength(4);
    let nulls = 0;
    for (const c of ARROWS) {
      const got = arrowPath(c.x, c.y, c.x2, c.y2, c.stroke, c.canvas);
      expect({ name: c.name, got }).toEqual({ name: c.name, got: c.expected });
      if (c.expected === null) nulls += 1;
    }
    // Exactly one degenerate row: the other three must each draw.
    expect(nulls).toBe(1);
  });
});

// ---- spotlightRects ------------------------------------------------------------

describe("spotlightRects", () => {
  it("dims the four bands around the box, clipped to the canvas", () => {
    const rects = spotlightRects({ x: 0.2, y: 0.3, w: 0.5, h: 0.25 });
    expect(rects).toHaveLength(4);
    const [top, bottom, left, right] = rects;
    expect(top).toEqual({ x: 0, y: 0, w: 1, h: 0.3 });
    expect(bottom.y).toBeCloseTo(0.55, 12);
    expect(bottom.h).toBeCloseTo(0.45, 12);
    expect([left.x, left.y, left.w]).toEqual([0, 0.3, 0.2]);
    expect(left.h).toBeCloseTo(0.25, 12);
    expect(right.x).toBeCloseTo(0.7, 12);
    expect(right.w).toBeCloseTo(0.3, 12);
    // A box hanging off the right/bottom edges leaves no negative band.
    const edge = spotlightRects({ x: 0.8, y: 0.9, w: 0.5, h: 0.4 });
    expect(edge[1].h).toBe(0);
    expect(edge[3].w).toBe(0);
    expect(edge[2].h).toBeCloseTo(0.1, 12);
  });
});

// ---- zoomTransform ----------------------------------------------------------------

function zoomCue(overrides: Partial<Effect> = {}, span: [number, number] = [2_000, 6_000]): ActiveCue {
  return {
    effect: effect({ kind: "zoom", factor: 2, easing: 800, x: 1, y: 0, ...overrides }),
    clip: clip(),
    startMs: span[0],
    endMs: span[1],
  };
}

describe("zoomTransform", () => {
  // Named test (brief). Mutation: drop the clamp of the window centre, and
  // a corner focal point at factor 2 translates the stage past its own
  // edge (tx 0.5 - 2*1 = -1.5 < 1 - 2), showing an empty margin.
  it("zoom never exposes empty margins", () => {
    const cue = zoomCue();
    const full = zoomTransform(cue, 4_000);
    expect(full.scale).toBe(2);
    // Focal x=1 (right edge), y=0 (top edge): the window hugs the corner.
    expect(full.tx).toBeCloseTo(-1, 12);
    expect(full.ty).toBeCloseTo(0, 12);
    for (let t = 1_900; t <= 6_100; t += 50) {
      const z = zoomTransform(cue, t);
      expect(z.scale).toBeGreaterThanOrEqual(1);
      // The scaled canvas [tx, tx + s] must always cover [0, 1].
      expect(z.tx, `tx at ${t}`).toBeLessThanOrEqual(1e-12);
      expect(z.tx + z.scale, `right edge at ${t}`).toBeGreaterThanOrEqual(1 - 1e-12);
      expect(z.ty, `ty at ${t}`).toBeLessThanOrEqual(1e-12);
      expect(z.ty + z.scale, `bottom edge at ${t}`).toBeGreaterThanOrEqual(1 - 1e-12);
    }
  });

  it("eases in and out over `easing` ms of output time, identity outside", () => {
    const cue = zoomCue();
    expect(zoomTransform(cue, 1_999)).toEqual(IDENTITY_ZOOM);
    expect(zoomTransform(cue, 6_000)).toEqual(IDENTITY_ZOOM);
    expect(zoomTransform(cue, 2_000).scale).toBe(1);
    // Halfway through the ramp: smoothstep(0.5) = 0.5.
    expect(zoomTransform(cue, 2_400).scale).toBeCloseTo(1.5, 12);
    expect(zoomTransform(cue, 2_800).scale).toBe(2);
    expect(zoomTransform(cue, 5_600).scale).toBeCloseTo(1.5, 12);
    // Fix round 1 (the reference editor's `camera`, which the render
    // follows): an unset/zero easing falls back to the reference's 600 ms
    // ramp -- halfway through it, 300 ms in, smoothstep(0.5) = 0.5 ...
    expect(zoomTransform(zoomCue({ easing: 0 }), 2_300).scale).toBeCloseTo(1.5, 12);
    expect(zoomTransform(zoomCue({ easing: undefined }), 5_700).scale).toBeCloseTo(1.5, 12);
    // ... and the ramp is capped at half the span, so a long ease still
    // reaches full zoom at the span's midpoint (4000 in 2000..6000).
    expect(zoomTransform(zoomCue({ easing: 10_000 }), 4_000).scale).toBe(2);
    expect(zoomTransform(zoomCue({ easing: 10_000 }), 3_000).scale).toBeCloseTo(1.5, 12);
  });

  it("a centred focal point zooms straight in", () => {
    const z = zoomTransform(zoomCue({ x: 0.5, y: 0.5, factor: 3 }), 4_000);
    expect(z).toEqual({ scale: 3, tx: -1, ty: -1 });
  });

  // Fix round 1: the reference's `camera` takes the LAST active zoom
  // (`.at(-1)`), i.e. the one later in the project's order wins.
  it("activeZoom takes the LAST active zoom cue, identity otherwise", () => {
    expect(activeZoom([], 3_000)).toEqual(IDENTITY_ZOOM);
    const other = { ...zoomCue(), effect: effect() };
    const earlier = zoomCue({ x: 0.5, y: 0.5, factor: 3 });
    const later = zoomCue({ x: 0.5, y: 0.5, factor: 2 });
    expect(activeZoom([earlier, other, later], 4_000).scale).toBe(2);
    expect(activeZoom([later, earlier, other], 4_000).scale).toBe(3);
  });

  it("unzoomPoint maps a point on the zoomed stage back to canvas fractions", () => {
    const z = { scale: 2, tx: -1, ty: 0 };
    expect(unzoomPoint(z, { x: 0, y: 0 })).toEqual({ x: 0.5, y: 0 });
    expect(unzoomPoint(z, { x: 1, y: 1 })).toEqual({ x: 1, y: 0.5 });
    expect(unzoomPoint(IDENTITY_ZOOM, { x: 0.3, y: 0.7 })).toEqual({ x: 0.3, y: 0.7 });
  });
});
