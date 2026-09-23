/**
 * What each teaching cue DRAWS in the preview (Task 35; F-27–F-33), as plain
 * SVG shape descriptors in canvas pixels — so `CueOverlay.vue` stays a thin
 * loop and every number is testable without mounting anything.
 *
 * The drawing follows the reference compositor (`drawing-primitives.js`'s
 * `drawEffect`), the design the render (Task 43, an ASS document) is held
 * to as well: a text callout with an optional dark card behind it; an arrow
 * as `cueGeometry.arrowPath`'s two polygons; a highlight as a tinted,
 * outlined rounded box; a spotlight as four dimmed bands round an outline;
 * a numbered step as a coloured badge on a dark pill; a privacy cover as a
 * FULLY OPAQUE box (F-33 — it is a stationary cover, never a blur that
 * could be mistaken for redaction). A zoom draws nothing: it IS the stage
 * transform (`cueGeometry.zoomTransform`), and its focal marker is an
 * editing aid that lives in the handles layer, never in the picture.
 *
 * The preview does NOT wrap text or measure glyphs — a text callout breaks
 * only at the user's own newlines, and a step's pill is sized from an
 * average glyph width — an approximation the render (libass) does not share
 * (docs/Gaps.md GAP-173).
 */
import type { Effect } from "../editorTypes";
import type { Pt } from "./cueGeometry";
import { arrowPath, spotlightRects } from "./cueGeometry";
import type { Size } from "./previewGeometry";

export interface CueShape {
  tag: "rect" | "polygon" | "circle" | "text" | "line";
  /** `data-part` — which piece of the cue this is. */
  part: string;
  attrs: Record<string, string | number>;
  /** Text content, one entry per line (text shapes only). */
  lines?: string[];
}

/** The reference's dark card behind text and steps. */
const CARD_FILL = "#292332";
/** The reference's spotlight dim colour (`rgba(15,12,24,dim)`). */
const DIM_FILL = "#0f0c18";
const STEP_RADIUS = 22;
const STEP_LABEL_SIZE = 26;
/** Average glyph width as a fraction of the font size, for sizing a pill. */
const GLYPH_WIDTH = 0.55;

function pointsAttr(pts: Pt[]): string {
  return pts.map(([x, y]) => `${x},${y}`).join(" ");
}

/** A step pill's width in canvas px: the badge plus its label. */
function stepPillWidth(effect: Effect): number {
  return 2 * STEP_RADIUS + 7 + (effect.text ?? "").length * STEP_LABEL_SIZE * GLYPH_WIDTH;
}

function box(effect: Effect, canvas: Size) {
  return {
    x: effect.x * canvas.width,
    y: effect.y * canvas.height,
    width: (effect.w ?? 0) * canvas.width,
    height: (effect.h ?? 0) * canvas.height,
  };
}

function textShapes(e: Effect, canvas: Size): CueShape[] {
  const b = box(e, canvas);
  const size = e.fontSize ?? 32;
  const shapes: CueShape[] = [];
  if (e.background) {
    shapes.push({ tag: "rect", part: "background", attrs: { ...b, rx: 9, fill: CARD_FILL, "fill-opacity": 0.94 } });
  }
  shapes.push({
    tag: "text",
    part: "text",
    attrs: { x: b.x + 17, y: b.y + 12 + size, "font-size": size, "font-weight": 550, fill: e.color },
    lines: (e.text ?? "").split("\n"),
  });
  return shapes;
}

function arrowShapes(e: Effect, canvas: Size): CueShape[] {
  const path = arrowPath(e.x, e.y, e.x2 ?? e.x, e.y2 ?? e.y, e.stroke ?? 5, canvas);
  if (!path) return [];
  return [
    { tag: "polygon", part: "shaft", attrs: { points: pointsAttr(path.shaft), fill: e.color } },
    { tag: "polygon", part: "head", attrs: { points: pointsAttr(path.head), fill: e.color } },
  ];
}

function highlightShapes(e: Effect, canvas: Size): CueShape[] {
  const attrs = { ...box(e, canvas), rx: 8, fill: e.color, "fill-opacity": 0.125, stroke: e.color, "stroke-width": e.stroke ?? 4 };
  return [{ tag: "rect", part: "box", attrs }];
}

function spotlightShapes(e: Effect, canvas: Size): CueShape[] {
  const rects = spotlightRects({ x: e.x, y: e.y, w: e.w ?? 0, h: e.h ?? 0 });
  const dims: CueShape[] = rects.map((r) => ({
    tag: "rect",
    part: "dim",
    attrs: {
      x: r.x * canvas.width,
      y: r.y * canvas.height,
      width: r.w * canvas.width,
      height: r.h * canvas.height,
      fill: DIM_FILL,
      "fill-opacity": e.dim ?? 0.65,
    },
  }));
  return [...dims, { tag: "rect", part: "outline", attrs: { ...box(e, canvas), fill: "none", stroke: e.color, "stroke-width": 2 } }];
}

function stepShapes(e: Effect, canvas: Size): CueShape[] {
  const [cx, cy] = [e.x * canvas.width, e.y * canvas.height];
  const pill = { x: cx - STEP_RADIUS, y: cy - STEP_RADIUS, width: stepPillWidth(e), height: 2 * STEP_RADIUS + 1 };
  return [
    { tag: "rect", part: "pill", attrs: { ...pill, rx: STEP_RADIUS, fill: CARD_FILL, "fill-opacity": 0.93 } },
    { tag: "circle", part: "badge", attrs: { cx, cy, r: STEP_RADIUS, fill: e.color } },
    {
      tag: "text",
      part: "number",
      attrs: { x: cx, y: cy + 8, "text-anchor": "middle", "font-size": 23, "font-weight": 700, fill: "#47364e" },
      lines: [String(e.number ?? 1)],
    },
    {
      tag: "text",
      part: "label",
      attrs: { x: cx + 35, y: cy + 8, "font-size": STEP_LABEL_SIZE, "font-weight": 500, fill: "#ffffff" },
      lines: [e.text ?? ""],
    },
  ];
}

function maskShapes(e: Effect, canvas: Size): CueShape[] {
  return [{ tag: "rect", part: "cover", attrs: { ...box(e, canvas), fill: e.color } }];
}

const DRAW: Record<Effect["kind"], (e: Effect, canvas: Size) => CueShape[]> = {
  text: textShapes,
  arrow: arrowShapes,
  highlight: highlightShapes,
  spotlight: spotlightShapes,
  zoom: () => [],
  step: stepShapes,
  mask: maskShapes,
};

/** Every shape one cue paints into the picture, bottom first. */
export function cueShapes(effect: Effect, canvas: Size): CueShape[] {
  return DRAW[effect.kind](effect, canvas);
}

/** The invisible shape a pointer grabs to select (and, once selected,
 * move) a cue, in canvas px. An arrow is grabbed along its line, a zoom at
 * its focal point, everything else by its box. */
export function hitShape(effect: Effect, canvas: Size): CueShape {
  const [x, y] = [effect.x * canvas.width, effect.y * canvas.height];
  switch (effect.kind) {
    case "arrow":
      return {
        tag: "line",
        part: "hit",
        attrs: {
          x1: x,
          y1: y,
          x2: (effect.x2 ?? effect.x) * canvas.width,
          y2: (effect.y2 ?? effect.y) * canvas.height,
          "stroke-width": Math.max(24, (effect.stroke ?? 5) * 3),
        },
      };
    case "zoom":
      return { tag: "circle", part: "hit", attrs: { cx: x, cy: y, r: 24 } };
    case "step":
      return {
        tag: "rect",
        part: "hit",
        attrs: { x: x - STEP_RADIUS, y: y - STEP_RADIUS, width: stepPillWidth(effect), height: 2 * STEP_RADIUS + 1 },
      };
    default:
      return { tag: "rect", part: "hit", attrs: box(effect, canvas) };
  }
}
