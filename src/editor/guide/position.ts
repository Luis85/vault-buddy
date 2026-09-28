/**
 * Where the guide's coach card and its target label go (Task 56; visual-
 * parity Task 23; concept spec §9.2, the reference's `onboarding.js:
 * function layout`). Pure: viewport pixels in, a placement out — no DOM, so
 * every branch is a unit test (`tests/editorGuidePosition.test.ts`).
 *
 * The card is `min(362, window − 24)` wide and never taller than the
 * window less 24. Four candidates are tried in the concept's order —
 * **right** of the target (18 px clear, centred on it vertically, clamped
 * 12 px inside the window), **left**, **below** (centred horizontally) and
 * **above** — and the first that fits wholly inside the window wins,
 * preferring one that leaves an open menu uncovered. When none fits, the
 * larger free band above or below the target, if it is at least 240 px,
 * takes the card at that band's height and only its copy scrolls
 * ("compact-scroll": the explanation scrolls rather than cover the very
 * control it explains); otherwise the card is pinned to the far edge
 * ("compact"). With no target it waits in the top-right corner.
 *
 * The label chip names the control: at the target's left edge, 30 px above
 * it (10 px below it near the top of the window), never wider than 250 and
 * hidden under 700 px wide or while a menu is open.
 */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface Size {
  width: number;
  height: number;
}

export type PlacementMode = "right" | "left" | "below" | "above" | "compact-scroll" | "compact" | "free";

export interface Placement {
  mode: PlacementMode;
  x: number;
  y: number;
  width: number;
  /** The card never grows past this; its copy scrolls instead. */
  maxHeight: number;
}

export const CARD_MAX_WIDTH = 362;
/** Clear space between the card and the highlighted control. */
export const TARGET_GAP_PX = 18;
/** Clear space between the card and the window's edge. */
export const EDGE_PAD_PX = 12;
/** The smallest free band the card may shrink into and scroll. */
export const COMPACT_MIN_BAND_PX = 240;
/** Where a card with no target sits from the top. */
export const FREE_TOP_PX = 95;
/** The card's height floor in a very short window. */
const MIN_CARD_HEIGHT_PX = 175;

export const LABEL_MAX_WIDTH = 250;
export const LABEL_HIDDEN_BELOW_PX = 700;
const LABEL_EDGE_PX = 8;
const LABEL_ABOVE_PX = 30;
const LABEL_BELOW_PX = 10;
/** A target closer to the top than this gets its label below it. */
const LABEL_NEAR_TOP_PX = 34;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), Math.max(lo, hi));
}

function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/** The card's width in a window `viewportWidth` wide. */
export function coachWidth(viewportWidth: number): number {
  return Math.min(CARD_MAX_WIDTH, viewportWidth - 2 * EDGE_PAD_PX);
}

interface Candidate {
  mode: PlacementMode;
  x: number;
  y: number;
}

/** The four candidates beside `t`, in the concept's order. */
function candidates(t: Rect, w: number, h: number, vp: Size): Candidate[] {
  const pad = EDGE_PAD_PX;
  const g = TARGET_GAP_PX;
  const cy = clamp(t.y + t.height / 2 - h / 2, pad, vp.height - h - pad);
  const cx = clamp(t.x + t.width / 2 - w / 2, pad, vp.width - w - pad);
  return [
    { mode: "right", x: t.x + t.width + g, y: cy },
    { mode: "left", x: t.x - w - g, y: cy },
    { mode: "below", x: cx, y: t.y + t.height + g },
    { mode: "above", x: cx, y: t.y - h - g },
  ];
}

/** The first candidate wholly inside the window, preferring one that
 * leaves `menu` uncovered; `null` when none fits. */
function firstFit(t: Rect, w: number, h: number, vp: Size, menu: Rect | null): Candidate | null {
  const pad = EDGE_PAD_PX;
  const valid = candidates(t, w, h, vp).filter(
    (c) => c.x >= pad && c.y >= pad && c.x + w <= vp.width - pad && c.y + h <= vp.height - pad,
  );
  const clear = menu ? valid.find((c) => !overlaps({ x: c.x, y: c.y, width: w, height: h }, menu)) : undefined;
  return clear ?? valid[0] ?? null;
}

/** No candidate fits: the larger band above/below, or pinned. */
function squeezed(t: Rect, h: number, vp: Size): { mode: PlacementMode; y: number; height: number } {
  const above = t.y - TARGET_GAP_PX - EDGE_PAD_PX;
  const below = vp.height - (t.y + t.height) - TARGET_GAP_PX - EDGE_PAD_PX;
  const room = Math.max(above, below);
  if (room >= COMPACT_MIN_BAND_PX && room < h) {
    const y = below >= above ? t.y + t.height + TARGET_GAP_PX : t.y - TARGET_GAP_PX - room;
    return { mode: "compact-scroll", y, height: room };
  }
  const y = t.y > vp.height / 2 ? EDGE_PAD_PX : vp.height - h - EDGE_PAD_PX;
  return { mode: "compact", y, height: h };
}

/** Where the coach card goes for a target (or none), given the card's
 * natural height and any open menu's box. */
export function placeCoach(target: Rect | null, naturalHeight: number, viewport: Size, menu: Rect | null = null): Placement {
  const w = coachWidth(viewport.width);
  let maxHeight = Math.max(MIN_CARD_HEIGHT_PX, viewport.height - 2 * EDGE_PAD_PX);
  let h = Math.min(naturalHeight, maxHeight);
  let mode: PlacementMode = "free";
  let x = viewport.width - w - EDGE_PAD_PX;
  let y = Math.min(FREE_TOP_PX, viewport.height - h - EDGE_PAD_PX);
  if (target) {
    const fit = firstFit(target, w, h, viewport, menu);
    if (fit) ({ mode, x, y } = fit);
    else {
      const band = squeezed(target, h, viewport);
      ({ mode, y } = band);
      if (band.mode === "compact-scroll") maxHeight = h = band.height;
    }
  }
  return {
    mode,
    x: clamp(x, EDGE_PAD_PX, viewport.width - w - EDGE_PAD_PX),
    y: clamp(y, EDGE_PAD_PX, viewport.height - h - EDGE_PAD_PX),
    width: w,
    maxHeight,
  };
}

/** Where the target's label chip goes, or `null` when it is hidden. */
export function placeLabel(target: Rect | null, labelWidth: number, viewport: Size, menuOpen: boolean): { x: number; y: number } | null {
  if (!target || menuOpen || viewport.width < LABEL_HIDDEN_BELOW_PX) return null;
  const w = Math.min(LABEL_MAX_WIDTH, labelWidth);
  return {
    x: clamp(target.x, LABEL_EDGE_PX, viewport.width - w - LABEL_EDGE_PX),
    y: target.y > LABEL_NEAR_TOP_PX ? target.y - LABEL_ABOVE_PX : target.y + target.height + LABEL_BELOW_PX,
  };
}
