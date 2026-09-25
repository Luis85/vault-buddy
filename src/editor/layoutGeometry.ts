/**
 * The picture-in-picture layout arithmetic (Task 31; F-21, F-23, F-38) —
 * pure functions over a clip's normalized box, shared by the preview's
 * `LayoutHandles.vue` (drag to move, eight handles to resize) and the
 * inspector's `LayoutSection.vue` (numeric entry, corner presets), so the
 * two can never disagree about where a corner is or how far a box may go.
 *
 * A box is `x, y, w, h` as FRACTIONS of the output canvas, origin top-left
 * (DATA-MODEL.md § Layout and compositing properties). Rust accepts the
 * schema's ranges only (`commands::layout`: `x,y ∈ [0,1]`, `w,h ∈ [0.1,1]`)
 * and does not insist that a box stays inside the frame; `clampBox` here
 * is where every gesture and preset keeps it inside, so what the user drags
 * is what renders.
 *
 * **Normalized is not square.** On a 1280×720 canvas a box with `w == h`
 * is 16:9 in pixels, not square — so anything that must look a certain
 * shape (a circle above all, F-38: "Circle frames stay circular") has to
 * carry the canvas aspect into its height. `cornerPreset` is that one
 * place.
 */
import type { Size } from "./previewGeometry";

export interface NormBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type Corner = "tl" | "tr" | "bl" | "br";
/** The eight resize handles, by compass point. */
export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
export const HANDLES: readonly Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/** Mirrors `validate::check_clip`'s `w`/`h` range `[0.1, 1]` (read from the
 * Rust source, never invented). */
export const MIN_SIZE = 0.1;
const MAX_SIZE = 1;
/** The brief's corner margin — the webcam's own presenter placement is the
 * ADR's separate constant (Task 50), not this. */
export const CORNER_MARGIN = 0.025;
/** The default picture-in-picture width, a fraction of the canvas width. */
export const PIP_SIZE = 0.19;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), hi);
}

/** Keep a box inside the frame and inside the schema's size range: sizes
 * first, then the position within whatever room the size leaves. */
export function clampBox(box: NormBox): NormBox {
  const w = clamp(box.w, MIN_SIZE, MAX_SIZE);
  const h = clamp(box.h, MIN_SIZE, MAX_SIZE);
  return { x: clamp(box.x, 0, 1 - w), y: clamp(box.y, 0, 1 - h), w, h };
}

/** A box moved by a normalized delta, kept inside the frame. */
export function moveBox(box: NormBox, dx: number, dy: number): NormBox {
  return clampBox({ ...box, x: box.x + dx, y: box.y + dy });
}

/** A box rounded to 1/10000 of the canvas (a tenth of a pixel on the
 * largest supported canvas) before it is sent, so an undo label or a saved
 * project never carries `0.6100000000000001`. */
export function roundBox(box: NormBox): NormBox {
  const r = (v: number) => Math.round(v * 10_000) / 10_000;
  return clampBox({ x: r(box.x), y: r(box.y), w: r(box.w), h: r(box.h) });
}

/**
 * A box whose PIXEL aspect equals `sourceWidth:sourceHeight` on `canvas`:
 * `h = w × canvasW / canvasH × sourceH / sourceW`. A circle frame always
 * asks for a square (1:1) so it stays circular on every canvas; an unknown
 * source defaults to the canvas's own shape (`h == w`).
 */
export function aspectHeight(w: number, canvas: Size, sourceAspect: number): number {
  return (w * canvas.width) / canvas.height / sourceAspect;
}

export interface PresetSource {
  /** Source pixel width/height, when known. */
  width?: number | null;
  height?: number | null;
  /** A circle frame is kept square in pixels whatever the source is. */
  circle?: boolean;
}

function sourceAspect(canvas: Size, source: PresetSource): number {
  if (source.circle) return 1;
  if (source.width && source.height) return source.width / source.height;
  return canvas.width / canvas.height;
}

/**
 * A `size`-wide box in `corner`, `CORNER_MARGIN` in from both edges, with
 * an aspect-correct height (see `aspectHeight`). A height outside the
 * schema's range scales the whole box, so the aspect survives.
 */
export function cornerPreset(
  corner: Corner,
  canvas: Size,
  size = PIP_SIZE,
  source: PresetSource = {},
): NormBox {
  let w = size;
  let h = aspectHeight(w, canvas, sourceAspect(canvas, source));
  const fit = Math.min(1, (MAX_SIZE - 2 * CORNER_MARGIN) / h, MAX_SIZE / w);
  const grow = Math.max(1, MIN_SIZE / (h * fit), MIN_SIZE / (w * fit));
  w *= fit * grow;
  h *= fit * grow;
  const x = corner.endsWith("r") ? 1 - w - CORNER_MARGIN : CORNER_MARGIN;
  const y = corner.startsWith("b") ? 1 - h - CORNER_MARGIN : CORNER_MARGIN;
  return clampBox({ x, y, w, h });
}

/** Scale `(w, h)` so both fit `[MIN_SIZE, max]` without changing their
 * ratio (a ratio no size can satisfy is left to `clampBox`). */
function fitRatio(w: number, h: number, maxW: number, maxH: number): { w: number; h: number } {
  const down = Math.min(1, maxW / w, maxH / h);
  const up = Math.max(1, MIN_SIZE / (w * down), MIN_SIZE / (h * down));
  return { w: w * down * up, h: h * down * up };
}

/** One axis of a handle: does it drag the low edge (`w`/`n`), the high
 * edge (`e`/`s`), or neither (the axis stays centred)? */
interface AxisGrip {
  low: boolean;
  high: boolean;
}

function grips(handle: Handle): { h: AxisGrip; v: AxisGrip } {
  return {
    h: { low: handle.includes("w"), high: handle.includes("e") },
    v: { low: handle.includes("n"), high: handle.includes("s") },
  };
}

/** How long this axis may grow while its fixed edge (or centre) stays. */
function axisRoom(start: number, size: number, grip: AxisGrip): number {
  if (grip.low) return start + size;
  if (grip.high) return 1 - start;
  const centre = start + size / 2;
  return 2 * Math.min(centre, 1 - centre);
}

/** Where this axis starts once resized to `next`, its fixed edge kept. */
function axisStart(start: number, size: number, next: number, grip: AxisGrip): number {
  if (grip.low) return start + size - next;
  if (grip.high) return start;
  return start + (size - next) / 2;
}

/** The ratio-keeping scale: a corner follows whichever axis moved more
 * (relative to the box), an edge its own axis. */
function ratioScale(orig: NormBox, free: NormBox, g: { h: AxisGrip; v: AxisGrip }): number {
  const sx = free.w / orig.w;
  const sy = free.h / orig.h;
  const horizontal = g.h.low || g.h.high;
  const vertical = g.v.low || g.v.high;
  if (horizontal && vertical) return Math.abs(sx - 1) >= Math.abs(sy - 1) ? sx : sy;
  return horizontal ? sx : sy;
}

/** The anchored resize for `keepAspect`: the opposite edge/corner never
 * moves, and an edge handle keeps the other axis centred. */
function keepRatio(orig: NormBox, free: NormBox, handle: Handle): NormBox {
  const g = grips(handle);
  const scale = ratioScale(orig, free, g);
  const { w, h } = fitRatio(
    orig.w * scale,
    orig.h * scale,
    axisRoom(orig.x, orig.w, g.h),
    axisRoom(orig.y, orig.h, g.v),
  );
  return { x: axisStart(orig.x, orig.w, w, g.h), y: axisStart(orig.y, orig.h, h, g.v), w, h };
}

/**
 * The box after dragging `handle` by a normalized `(dx, dy)`: the handle's
 * own edges move, the opposite ones stay put, and no edge crosses its
 * opposite closer than `MIN_SIZE`. `keepAspect` keeps the box's current
 * ratio (a corner drag of a picture, any drag of a circle).
 */
export function resizeFromHandle(box: NormBox, handle: Handle, dx: number, dy: number, keepAspect: boolean): NormBox {
  let left = box.x;
  let top = box.y;
  let right = box.x + box.w;
  let bottom = box.y + box.h;
  if (handle.includes("w")) left = clamp(left + dx, 0, right - MIN_SIZE);
  if (handle.includes("e")) right = clamp(right + dx, left + MIN_SIZE, 1);
  if (handle.includes("n")) top = clamp(top + dy, 0, bottom - MIN_SIZE);
  if (handle.includes("s")) bottom = clamp(bottom + dy, top + MIN_SIZE, 1);
  const free = { x: left, y: top, w: right - left, h: bottom - top };
  return clampBox(keepAspect ? keepRatio(box, free, handle) : free);
}

/**
 * The presenter's placement (Task 50; F-21; pre-flight F35): the ADR's OWN
 * constant — the §4 staged-capture migration's webcam clip and the
 * reference workspace both put a presenter at `(0.775, 0.06)`, `0.19` wide,
 * a circle filled `cover`. Defined ONCE, here, and passed explicitly by the
 * webcam dialog: `cornerPreset("tr", …)` would compute `CORNER_MARGIN`'s
 * generic `(0.785, 0.025)` for the same width, a different place.
 */
export const PRESENTER_CORNER = { x: 0.775, y: 0.06, w: 0.19, frameShape: "circle", fit: "cover" } as const;

/** `PRESENTER_CORNER` as a box on `canvas`: a circle, so its height keeps
 * it square in PIXELS (`0.3378` on 1280×720 — the ADR's own figure). */
export function presenterBox(canvas: Size): NormBox {
  const { x, y, w } = PRESENTER_CORNER;
  return roundBox({ x, y, w, h: aspectHeight(w, canvas, 1) });
}
