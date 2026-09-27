/**
 * Where the guide's coach card and its target label go (visual-parity Task
 * 23; concept spec §9.2, `onboarding.js: function layout`). Pure
 * arithmetic, one test per branch: the four candidates in their order
 * (right, left, below, above), the open-menu preference, the scrolling
 * band ("compact-scroll"), the pinned fallback ("compact") and the
 * no-target corner — then the label chip's own rules.
 */
import { describe, expect, it } from "vitest";

import type { Rect, Size } from "../src/editor/guide/position";
import {
  CARD_MAX_WIDTH,
  coachWidth,
  COMPACT_MIN_BAND_PX,
  EDGE_PAD_PX,
  FREE_TOP_PX,
  LABEL_HIDDEN_BELOW_PX,
  LABEL_MAX_WIDTH,
  placeCoach,
  placeLabel,
  TARGET_GAP_PX,
} from "../src/editor/guide/position";

const VP: Size = { width: 1600, height: 1000 };
const H = 400;

function inside(p: { x: number; y: number; width: number; maxHeight: number }, h: number, vp: Size): boolean {
  const height = Math.min(h, p.maxHeight);
  return p.x >= EDGE_PAD_PX && p.y >= EDGE_PAD_PX && p.x + p.width <= vp.width - EDGE_PAD_PX && p.y + height <= vp.height - EDGE_PAD_PX;
}

function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

describe("coachWidth", () => {
  it("is 362, or the viewport less 24 when that is narrower", () => {
    expect(CARD_MAX_WIDTH).toBe(362);
    expect(coachWidth(1600)).toBe(362);
    expect(coachWidth(300)).toBe(276);
  });
});

describe("placeCoach", () => {
  it("no target: the top-right corner, 95 from the top", () => {
    const p = placeCoach(null, H, VP);
    expect(p).toEqual({ mode: "free", x: 1600 - 362 - 12, y: FREE_TOP_PX, width: 362, maxHeight: 1000 - 24 });
    // A short window: 95 would push the card off the bottom.
    const short = placeCoach(null, 500, { width: 1000, height: 560 });
    expect(short.y).toBe(560 - 500 - 12);
  });

  it("right of the target first: gap 18, centred vertically", () => {
    const t = { x: 100, y: 300, width: 120, height: 40 };
    const p = placeCoach(t, H, VP);
    expect(p).toMatchObject({ mode: "right", x: 100 + 120 + TARGET_GAP_PX, y: 320 - H / 2, width: 362 });
  });

  it("the vertical centre is clamped to 12 from the edges", () => {
    const t = { x: 100, y: 20, width: 120, height: 30 };
    expect(placeCoach(t, H, VP)).toMatchObject({ mode: "right", y: EDGE_PAD_PX });
    const low = { x: 100, y: 960, width: 120, height: 30 };
    expect(placeCoach(low, H, VP)).toMatchObject({ mode: "right", y: 1000 - H - EDGE_PAD_PX });
  });

  it("left when the right does not fit (the inspector's Fades section)", () => {
    const t = { x: 1330, y: 250, width: 260, height: 300 };
    const p = placeCoach(t, H, VP);
    expect(p).toMatchObject({ mode: "left", x: 1330 - 362 - TARGET_GAP_PX, y: 400 - H / 2 });
    expect(overlaps({ x: p.x, y: p.y, width: p.width, height: H }, t)).toBe(false);
  });

  it("below, centred horizontally, when neither side fits", () => {
    const t = { x: 200, y: 100, width: 1200, height: 40 };
    const p = placeCoach(t, H, VP);
    expect(p).toMatchObject({ mode: "below", x: 800 - 181, y: 140 + TARGET_GAP_PX });
  });

  it("above when below does not fit either", () => {
    const t = { x: 200, y: 800, width: 1200, height: 40 };
    const p = placeCoach(t, H, VP);
    expect(p).toMatchObject({ mode: "above", x: 800 - 181, y: 800 - TARGET_GAP_PX - H });
  });

  it("prefers a candidate that leaves an open menu uncovered", () => {
    const t = { x: 100, y: 300, width: 120, height: 40 };
    // A menu open right where the right-hand candidate would land.
    const menu = { x: 240, y: 100, width: 260, height: 250 };
    const p = placeCoach(t, H, VP, menu);
    expect(p.mode).not.toBe("right");
    expect(overlaps({ x: p.x, y: p.y, width: p.width, height: H }, menu)).toBe(false);
    expect(inside(p, H, VP)).toBe(true);
  });

  it("keeps the first fit when every candidate would cover the menu", () => {
    const t = { x: 100, y: 300, width: 120, height: 40 };
    const menu = { x: 0, y: 0, width: 1600, height: 1000 };
    expect(placeCoach(t, H, VP, menu).mode).toBe("right");
  });

  it("compact-scroll: the larger free band of at least 240, the copy scrolling", () => {
    const vp = { width: 960, height: 640 };
    // A full-width toolbar: no side fits, and the card is taller than
    // either band.
    const t = { x: 0, y: 380, width: 960, height: 32 };
    const p = placeCoach(t, 500, vp);
    const band = 380 - TARGET_GAP_PX - EDGE_PAD_PX;
    expect(band).toBeGreaterThanOrEqual(COMPACT_MIN_BAND_PX);
    expect(p).toMatchObject({ mode: "compact-scroll", maxHeight: band, y: 380 - TARGET_GAP_PX - band });
    expect(overlaps({ x: p.x, y: p.y, width: p.width, height: p.maxHeight }, t)).toBe(false);
    // The band below, when it is the larger one.
    const high = { x: 0, y: 200, width: 960, height: 32 };
    const q = placeCoach(high, 500, vp);
    expect(q).toMatchObject({ mode: "compact-scroll", y: 232 + TARGET_GAP_PX, maxHeight: 640 - 232 - TARGET_GAP_PX - EDGE_PAD_PX });
  });

  it("compact: pinned to the far edge when no band reaches 240", () => {
    // A target in the lower half of a short window: pinned to the top.
    const short = { width: 960, height: 400 };
    const t = { x: 0, y: 210, width: 960, height: 100 };
    expect(placeCoach(t, 300, short)).toMatchObject({ mode: "compact", y: EDGE_PAD_PX });
    // In the upper half: pinned to the bottom.
    const vp = { width: 960, height: 640 };
    const u = { x: 0, y: 200, width: 960, height: 250 };
    expect(placeCoach(u, 500, vp)).toMatchObject({ mode: "compact", y: 640 - 500 - EDGE_PAD_PX });
  });

  it("the card never grows past the window less 24", () => {
    const p = placeCoach(null, 2000, VP);
    expect(p.maxHeight).toBe(1000 - 24);
    expect(p.y).toBe(EDGE_PAD_PX);
  });
});

describe("placeLabel", () => {
  const t = { x: 1330, y: 250, width: 260, height: 300 };

  it("sits at the target's left, 30 above its top", () => {
    expect(placeLabel(t, 100, VP, false)).toEqual({ x: 1330, y: 220 });
  });

  it("goes below a target near the top of the window", () => {
    expect(placeLabel({ x: 400, y: 20, width: 80, height: 30 }, 100, VP, false)).toEqual({ x: 400, y: 60 });
  });

  it("stays 8 inside the window, measured at most 250 wide", () => {
    expect(placeLabel({ x: 1560, y: 300, width: 30, height: 30 }, 120, VP, false)?.x).toBe(1600 - 120 - 8);
    expect(placeLabel({ x: 1560, y: 300, width: 30, height: 30 }, 400, VP, false)?.x).toBe(1600 - LABEL_MAX_WIDTH - 8);
    expect(placeLabel({ x: 2, y: 300, width: 30, height: 30 }, 120, VP, false)?.x).toBe(8);
  });

  it("is hidden under 700 wide, while a menu is open, and with no target", () => {
    expect(LABEL_HIDDEN_BELOW_PX).toBe(700);
    expect(placeLabel(t, 100, { width: 699, height: 600 }, false)).toBeNull();
    expect(placeLabel(t, 100, { width: 700, height: 600 }, false)).not.toBeNull();
    expect(placeLabel(t, 100, VP, true)).toBeNull();
    expect(placeLabel(null, 100, VP, false)).toBeNull();
  });
});
