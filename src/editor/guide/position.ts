/**
 * Where the guide's coach card goes (Task 56; F-46; ONBOARDING.md § Overlay,
 * positioning and focus: "Prefer positioning beside the target without
 * covering it or essential controls. On narrow windows dock the card with a
 * visible target region, scroll if needed and retain pause/collapse
 * controls."). Pure: viewport pixels in, a placement out — no DOM, so the
 * arithmetic is tested as a table (`tests/editorGuideCoach.test.ts`).
 *
 * The free space around the target is four strips — right, left, below,
 * above — each running to the viewport's edge minus `EDGE_MARGIN_PX`, and
 * `TARGET_GAP_PX` clear of the target. They are tried largest first
 * ("candidate sides by available space"):
 *
 * - **beside** (viewport at least `DOCKED_BELOW_PX` wide): the first strip
 *   the whole card fits in; the card sits against the target, centred on
 *   it along the other axis and clamped into the strip.
 * - **docked** (narrower, or nothing fits whole): the largest strip at
 *   least `MIN_CARD` in size; the card is anchored to that strip's outer
 *   corner and sized to it, and `maxHeight` makes the card's own body
 *   scroll rather than grow over the target. Its header and its
 *   Pause/Back/Next row are what `MIN_CARD` keeps.
 *
 * Either way the card stays inside its strip, so it never covers the
 * target. A target the strips cannot clear at all (bigger than the
 * viewport less a card) is clipped to the viewport first; the caller
 * scrolls a lesson's control into view before placing.
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
export type Side = "right" | "left" | "bottom" | "top";
export interface Placement {
  mode: "beside" | "docked";
  /** The strip the card sits in; `null` when there is no target. */
  side: Side | null;
  x: number;
  y: number;
  width: number;
  /** The card never grows past this; its body scrolls instead. */
  maxHeight: number;
}

/** Below this viewport width the card docks (the brief's 1100 px). */
export const DOCKED_BELOW_PX = 1100;
/** Clear space between the card and the highlighted control. */
export const TARGET_GAP_PX = 12;
/** Clear space between the card and the viewport's edge. */
export const EDGE_MARGIN_PX = 8;
/** The smallest card that still shows its header and Pause/Back/Next. */
export const MIN_CARD: Size = { width: 240, height: 120 };

const SIDES: readonly Side[] = ["right", "left", "bottom", "top"];

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), Math.max(lo, hi));
}

/** `target` clipped to the viewport. */
function clipTo(target: Rect, vp: Size): Rect {
  const x = clamp(target.x, 0, vp.width);
  const y = clamp(target.y, 0, vp.height);
  return {
    x,
    y,
    width: clamp(target.x + target.width, x, vp.width) - x,
    height: clamp(target.y + target.height, y, vp.height) - y,
  };
}

/** The four free strips around `t`, each clear of it by the gap. */
function strips(t: Rect, vp: Size): Record<Side, Rect> {
  const m = EDGE_MARGIN_PX;
  const g = TARGET_GAP_PX;
  const full = { y: m, height: vp.height - 2 * m };
  const wide = { x: m, width: vp.width - 2 * m };
  const right = t.x + t.width + g;
  const below = t.y + t.height + g;
  return {
    right: { ...full, x: right, width: Math.max(0, vp.width - m - right) },
    left: { ...full, x: m, width: Math.max(0, t.x - g - m) },
    bottom: { ...wide, y: below, height: Math.max(0, vp.height - m - below) },
    top: { ...wide, y: m, height: Math.max(0, t.y - g - m) },
  };
}

/** Sides by the strip's area, largest first (ties in `SIDES` order). */
function bySpace(regions: Record<Side, Rect>): Side[] {
  const area = (s: Side) => regions[s].width * regions[s].height;
  return [...SIDES].sort((a, b) => area(b) - area(a));
}

function besideIn(side: Side, r: Rect, t: Rect, card: Size): Placement {
  const horizontal = side === "right" || side === "left";
  const x = horizontal
    ? side === "right" ? r.x : r.x + r.width - card.width
    : clamp(t.x + t.width / 2 - card.width / 2, r.x, r.x + r.width - card.width);
  const y = horizontal
    ? clamp(t.y + t.height / 2 - card.height / 2, r.y, r.y + r.height - card.height)
    : side === "bottom" ? r.y : r.y + r.height - card.height;
  return { mode: "beside", side, x, y, width: card.width, maxHeight: r.y + r.height - y };
}

function dockedIn(side: Side, r: Rect, card: Size): Placement {
  const width = Math.min(card.width, r.width);
  const height = Math.min(card.height, r.height);
  // The strip's outer corner: away from the target, along the edge.
  const x = side === "left" ? r.x : r.x + r.width - width;
  const y = side === "top" ? r.y : r.y + r.height - height;
  return { mode: "docked", side, x, y, width, maxHeight: r.y + r.height - y };
}

/** Where the coach card goes for a target (or none) in a viewport. */
export function placeCoach(target: Rect | null, card: Size, viewport: Size): Placement {
  if (target === null) {
    const width = Math.min(card.width, viewport.width - 2 * EDGE_MARGIN_PX);
    const height = Math.min(card.height, viewport.height - 2 * EDGE_MARGIN_PX);
    return {
      mode: "docked",
      side: null,
      x: viewport.width - EDGE_MARGIN_PX - width,
      y: viewport.height - EDGE_MARGIN_PX - height,
      width,
      maxHeight: height,
    };
  }
  const t = clipTo(target, viewport);
  const regions = strips(t, viewport);
  const order = bySpace(regions);
  if (viewport.width >= DOCKED_BELOW_PX) {
    const fits = order.find((s) => regions[s].width >= card.width && regions[s].height >= card.height);
    if (fits) return besideIn(fits, regions[fits], t, card);
  }
  const roomy = order.find((s) => regions[s].width >= MIN_CARD.width && regions[s].height >= MIN_CARD.height);
  const side = roomy ?? order[0];
  return dockedIn(side, regions[side], card);
}
