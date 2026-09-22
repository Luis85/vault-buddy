/**
 * `src/editor/timelineLayout.ts` — the pure geometry/snap/virtualization
 * math behind `TimelineView.vue` (Task 20; F-04, F-14, F-26). Kept in its
 * own suite, separate from the component tests
 * (`tests/editorTimelineView.test.ts`), because performance (600 clips)
 * depends on this staying a plain function nothing here has to mount a
 * component to exercise.
 */
import { describe, expect, it } from "vitest";

import {
  BASE_PX_PER_MS,
  fitZoom,
  msToX,
  pxPerMs,
  snap,
  snapTargets,
  tickIntervalMs,
  visibleClips,
  xToMs,
} from "../src/editor/timelineLayout";
import type { Clip, Marker, Project } from "../src/editorTypes";

function track(id: string): Project["tracks"][number] {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1 };
}

function clip(overrides: Partial<Clip> & Pick<Clip, "id" | "start_ms" | "in_ms" | "out_ms">): Clip {
  return {
    asset_id: "a1",
    track_id: "v1",
    name: overrides.id,
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

function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks: [track("v1")],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

describe("pxPerMs", () => {
  it("scales linearly with zoom off the BASE_PX_PER_MS constant", () => {
    expect(pxPerMs(1)).toBeCloseTo(BASE_PX_PER_MS, 10);
    expect(pxPerMs(2)).toBeCloseTo(BASE_PX_PER_MS * 2, 10);
    expect(pxPerMs(0.5)).toBeCloseTo(BASE_PX_PER_MS * 0.5, 10);
  });

  it("floors a negative zoom at 0 rather than inverting the axis", () => {
    expect(pxPerMs(-5)).toBe(0);
  });
});

describe("msToX and xToMs", () => {
  // Asymmetric fixture (the global constraints' "fixture flaw" rule): a
  // non-round ms, a non-unit zoom and a non-zero scrollLeft so a swapped
  // argument or a dropped scroll term fails instead of hiding behind a
  // coincidental symmetry.
  it("are inverse within a pixel across a range of zoom/scroll/ms combos", () => {
    const cases: [number, number, number][] = [
      [0, 1, 0],
      [12345, 1, 0],
      [12345, 2.5, 0],
      [12345, 2.5, 400],
      [999999, 0.1, 1000],
      [1, 20, 0],
    ];
    for (const [ms, zoom, scrollLeft] of cases) {
      const x = msToX(ms, zoom, scrollLeft);
      const back = xToMs(x, zoom, scrollLeft);
      expect(Math.abs(back - ms)).toBeLessThanOrEqual(1 / pxPerMs(zoom) + 1e-6);
    }
  });

  it("defaults scrollLeft to 0 (content-space positioning)", () => {
    expect(msToX(1000, 1)).toBeCloseTo(msToX(1000, 1, 0), 10);
  });

  it("xToMs reads a degenerate (non-positive) zoom as time 0, never Infinity/NaN", () => {
    expect(xToMs(500, 0)).toBe(0);
    expect(xToMs(500, -1)).toBe(0);
  });
});

describe("fitZoom", () => {
  it("shows the entire edit: pxPerMs(fitZoom(duration, viewport)) * duration <= viewport", () => {
    const cases: [number, number][] = [
      [60_000, 1000],
      [7_200_000, 1200],
      [500, 900],
      [1, 50],
    ];
    for (const [durationMs, viewportPx] of cases) {
      const zoom = fitZoom(durationMs, viewportPx);
      const width = pxPerMs(zoom) * durationMs;
      expect(width).toBeLessThanOrEqual(viewportPx + 1e-6);
    }
  });

  it("degenerates to a sane default (never NaN/Infinity) for a zero duration or viewport", () => {
    expect(Number.isFinite(fitZoom(0, 1000))).toBe(true);
    expect(Number.isFinite(fitZoom(1000, 0))).toBe(true);
    expect(fitZoom(0, 1000)).toBeGreaterThan(0);
  });
});

describe("snapTargets", () => {
  it("gathers clip edges, marker output positions and the playhead", () => {
    const c1 = clip({ id: "c1", start_ms: 0, in_ms: 0, out_ms: 2000 });
    const c2 = clip({ id: "c2", start_ms: 3000, in_ms: 500, out_ms: 1500, speed: 2 });
    const marker: Marker = { id: "m1", clip_id: "c2", source_ms: 1000, title: "chapter" };
    const p = project({ clips: [c1, c2], markers: [marker] });

    const targets = snapTargets(p, 5000);

    // c1: start 0, end 0 + (2000-0)/1 = 2000.
    expect(targets).toContain(0);
    expect(targets).toContain(2000);
    // c2: start 3000, end 3000 + (1500-500)/2 = 3500.
    expect(targets).toContain(3000);
    expect(targets).toContain(3500);
    // marker at source 1000 inside c2 [500,1500): output = 3000 + (1000-500)/2 = 3250.
    expect(targets).toContain(3250);
    // the playhead itself.
    expect(targets).toContain(5000);
  });

  it("returns just the playhead for a null project", () => {
    expect(snapTargets(null, 42)).toEqual([42]);
  });
});

describe("snap", () => {
  const targets = [0, 1000, 5000];

  it("picks the nearest target within the threshold only", () => {
    // 40px at zoom 1 -> 40 / BASE_PX_PER_MS ms threshold.
    const thresholdMs = 40 / pxPerMs(1);
    expect(thresholdMs).toBeGreaterThan(0);

    // Well inside the threshold of 1000 -> snaps.
    expect(snap(1000 - thresholdMs / 2, targets, 40, 1)).toBe(1000);
    // Outside every target's threshold -> unchanged.
    expect(snap(1000 + thresholdMs * 5, targets, 40, 1)).toBe(1000 + thresholdMs * 5);
    // Between two targets, each just outside threshold on both sides.
    expect(snap(2500, targets, 40, 1)).toBe(2500);
  });

  it("picks the CLOSER of two targets both within threshold", () => {
    const wideTargets = [1000, 1010];
    // A huge threshold puts both within range; 1010 is nearer to 1006.
    expect(snap(1006, wideTargets, 100_000, 1)).toBe(1010);
  });
});

describe("visibleClips (virtualization)", () => {
  // 600 clips, evenly spaced with real gaps between them (2000ms apart, each
  // 1000ms long) so a viewport window only ever covers a small, bounded
  // fraction of the total 1,200,000ms edit — the brief's own scenario.
  const CLIP_COUNT = 600;
  const SPACING_MS = 2000;
  function manyClips(): Clip[] {
    const out: Clip[] = [];
    for (let i = 0; i < CLIP_COUNT; i += 1) {
      const start = i * SPACING_MS;
      out.push(clip({ id: `c${i}`, start_ms: start, in_ms: 0, out_ms: 1000 }));
    }
    return out;
  }

  it("renders only visible clips: 600 clips, viewport 1000px -> rendered count < 60", () => {
    const clips = manyClips();
    const zoom = 1; // pxPerMs = BASE_PX_PER_MS
    const scrollLeft = 300_000 * pxPerMs(zoom); // scrolled to the middle of the edit
    const result = visibleClips(clips, scrollLeft, 1000, zoom);
    expect(result.length).toBeLessThan(60);
    expect(result.length).toBeGreaterThan(0);
  });

  it("keeps a clip that is only HALF visible at the padded edge (the ± one screen rule)", () => {
    // A single clip whose start sits just past the viewport's right edge but
    // well within the ONE-SCREEN padding the brief's mutation check names.
    const zoom = 1;
    const width = 1000;
    const scrollLeft = 0;
    // Right at the edge of "one screen past the viewport" -> must still be
    // included; comfortably past two screens -> must be excluded.
    const edgeMs = (width + width * 0.5) / pxPerMs(zoom); // 1.5 screens out
    const farMs = (width * 3) / pxPerMs(zoom); // 3 screens out
    const edgeClip = clip({ id: "edge", start_ms: edgeMs, in_ms: 0, out_ms: 500 });
    const farClip = clip({ id: "far", start_ms: farMs, in_ms: 0, out_ms: 500 });

    const result = visibleClips([edgeClip, farClip], scrollLeft, width, zoom);

    expect(result.map((c) => c.id)).toContain("edge");
    expect(result.map((c) => c.id)).not.toContain("far");
  });

  // Fix round 1, finding 6: this is a plain correctness check, NOT the
  // padding mutation check its previous name claimed -- a clip straddling
  // the UNPADDED viewport boundary is `start < hiMs && end > loMs` by
  // construction regardless of `pad`, so it stays visible with or without
  // the ± one screen padding and cannot, on its own, catch that padding
  // being dropped. The test right above this one ("keeps a clip that is
  // only HALF visible at the padded edge") is the one that actually
  // exercises the padding: it places a clip entirely OUTSIDE the unpadded
  // window but inside the padded one, which is exactly the case the
  // brief's own mutation check names and this repo's own mutation-check
  // run (see this task's report) confirmed goes red without the padding.
  //
  // The fixture here previously did not even straddle the boundary it
  // claimed to: a 200ms clip starting 100px before `hiMs` ended 10px
  // short of `hiMs` (at zoom 1, `pxPerMs` = 0.05), landing entirely
  // inside the unpadded window rather than crossing its edge. Re-pointed
  // to a clip whose span genuinely spans `hiMs` (50px on either side).
  it("a clip straddling the exact (unpadded) viewport boundary is never dropped", () => {
    const zoom = 1;
    const width = 1000;
    const scrollLeft = 500 * pxPerMs(zoom);
    const hiMs = (scrollLeft + width) / pxPerMs(zoom);
    const straddling = clip({
      id: "straddle",
      start_ms: hiMs - 50 / pxPerMs(zoom),
      in_ms: 0,
      out_ms: 100 / pxPerMs(zoom), // 100px of span, 50px on either side of hiMs
    });
    const result = visibleClips([straddling], scrollLeft, width, zoom);
    expect(result.map((c) => c.id)).toContain("straddle");
  });
});

describe("tickIntervalMs", () => {
  it("returns a nice interval whose pixel spacing meets the minimum at a given zoom", () => {
    const zoom = 1;
    const interval = tickIntervalMs(zoom);
    expect(interval * pxPerMs(zoom)).toBeGreaterThanOrEqual(60 - 1e-6);
  });

  it("shrinks (or holds) as zoom increases -- more pixels per ms needs fewer ms per tick", () => {
    const low = tickIntervalMs(0.5);
    const high = tickIntervalMs(8);
    expect(high).toBeLessThanOrEqual(low);
  });

  it("falls back to the largest nice interval for a non-positive zoom", () => {
    expect(tickIntervalMs(0)).toBe(3_600_000);
  });

  it("falls back to the largest nice interval when even THAT can't clear MIN_TICK_PX", () => {
    // At a vanishingly small positive zoom, no interval in the table --
    // not even the largest (1 hour) -- reaches the 60px minimum spacing;
    // the loop must exhaust and fall through to the same largest-interval
    // fallback the non-positive-zoom case above returns directly.
    expect(tickIntervalMs(0.0000001)).toBe(3_600_000);
  });
});
