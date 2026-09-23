/**
 * Teaching-cue geometry (Task 35; F-27–F-33; SCREENS-AND-INTERACTIONS.md:
 * "Teaching tools add editable clip-linked cues … Preview handles are never
 * part of the encoded output"). PURE: no DOM, no store, no clock — the
 * preview overlay (`CueOverlay.vue`), its handles (`CueHandles.vue`) and the
 * stage's zoom all read these, and the render (Tasks 42/43) must produce the
 * same numbers.
 *
 * **Time.** An `Effect`'s `start_ms`/`end_ms` are SOURCE time, linked to its
 * clip (Task 34's `cues.rs`). Where it paints on the OUTPUT timeline is
 * `timeMap.cueOutputSpan` at the clip's current speed — Rust's
 * `time::cue_output_span` twin, held to the shared time fixture — never a
 * second mapping here. A cue paints only while that span holds the playhead
 * (half-open) and only when its clip sits on a VISIBLE VIDEO track: a hidden
 * track does not paint, and an audio clip has no picture to annotate.
 *
 * **The arrow is shared with the render.** `arrowPath` returns the shaft and
 * the head as filled polygons in CANVAS PIXELS, because that is what an ASS
 * `\p1` drawing needs (ADR: cues are an ASS document) — so the SVG arrow and
 * the burned-in arrow are the same shapes. The rule and four hand-computed
 * rows live in `tests/fixtures/editor-arrow-cases.json`, which Task 43's
 * `ass.rs` reads with `include_str!`. Two choices keep the two languages
 * bit-comparable: the length is `sqrt(dx²+dy²)` (correctly rounded in
 * IEEE-754 everywhere, where `hypot` is library-dependent), and every output
 * is rounded half AWAY from zero to 2 decimals (`round2`, Rust's own
 * `f64::round` rule — `Math.round` alone rounds a negative tie the other
 * way).
 *
 * **Zoom never shows an empty margin** (F-31: "does not expose empty frame
 * margins"). At scale `s` the visible window is `1/s` wide; its centre is
 * the focal point CLAMPED to `[1/(2s), 1 − 1/(2s)]`, so the scaled canvas
 * always covers the whole frame — the reference compositor's own
 * `bounds()` clamp for a zoom. The ramp is a smoothstep over `easing` ms of
 * OUTPUT time at each end of the cue's span.
 */
import type { Clip, Effect, Project } from "../editorTypes";
import { clipSpanOf } from "./actionTargets";
import type { Size } from "./previewGeometry";
import { cueOutputSpan } from "./timeMap";

/** A cue on screen now: its effect, its clip, and its OUTPUT span. */
export interface ActiveCue {
  effect: Effect;
  clip: Clip;
  startMs: number;
  endMs: number;
}

/** The stage transform, in fractions of the canvas: a canvas point `p`
 * lands at `p * scale + t`. */
export interface ZoomTransform {
  scale: number;
  tx: number;
  ty: number;
}

export interface NormRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface NormPoint {
  x: number;
  y: number;
}

export type Pt = [number, number];

/** An arrow as two filled polygons, canvas px. */
export interface ArrowPath {
  shaft: Pt[];
  head: Pt[];
}

export const IDENTITY_ZOOM: ZoomTransform = { scale: 1, tx: 0, ty: 0 };

/** Every cue whose output span holds `t`, in the project's own order
 * (later effects paint on top, the render's event order). */
export function activeCues(project: Project | null, t: number): ActiveCue[] {
  if (!project) return [];
  const out: ActiveCue[] = [];
  for (const effect of project.effects) {
    const clip = project.clips.find((c) => c.id === effect.clip_id);
    const track = clip ? project.tracks.find((tr) => tr.id === clip.track_id) : undefined;
    if (!clip || !track || track.kind !== "video" || !track.visible) continue;
    const span = cueOutputSpan(clipSpanOf(clip), effect.start_ms, effect.end_ms);
    if (span && t >= span[0] && t < span[1]) out.push({ effect, clip, startMs: span[0], endMs: span[1] });
  }
  return out;
}

/** Round half away from zero to 2 decimals, never `-0`. */
function round2(v: number): number {
  const r = (Math.sign(v) * Math.round(Math.abs(v) * 100)) / 100;
  return r === 0 ? 0 : r;
}

function pt(x: number, y: number): Pt {
  return [round2(x), round2(y)];
}

/**
 * The arrow from the tail `(x, y)` to the tip `(x2, y2)` (canvas fractions),
 * `stroke` canvas px thick, as a shaft quad and a head triangle in canvas
 * px; `null` for a zero-length arrow, which draws nothing. The head is
 * `3·stroke + 12` long and `2·(2·stroke + 5)` wide, scaled down as a whole
 * when the arrow is shorter than its own head.
 */
export function arrowPath(
  x: number,
  y: number,
  x2: number,
  y2: number,
  stroke: number,
  canvas: Size,
): ArrowPath | null {
  const [ax, ay] = [x * canvas.width, y * canvas.height];
  const [bx, by] = [x2 * canvas.width, y2 * canvas.height];
  const [dx, dy] = [bx - ax, by - ay];
  const len = Math.sqrt(dx * dx + dy * dy);
  if (len === 0) return null;
  const [ux, uy] = [dx / len, dy / len];
  const [nx, ny] = [-uy, ux];
  let headLen = 3 * stroke + 12;
  let halfWidth = 2 * stroke + 5;
  if (len < headLen) {
    halfWidth *= len / headLen;
    headLen = len;
  }
  const [baseX, baseY] = [bx - ux * headLen, by - uy * headLen];
  const s = stroke / 2;
  return {
    shaft: [
      pt(ax + nx * s, ay + ny * s),
      pt(baseX + nx * s, baseY + ny * s),
      pt(baseX - nx * s, baseY - ny * s),
      pt(ax - nx * s, ay - ny * s),
    ],
    head: [pt(bx, by), pt(baseX + nx * halfWidth, baseY + ny * halfWidth), pt(baseX - nx * halfWidth, baseY - ny * halfWidth)],
  };
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** The four rectangles a spotlight dims around `box` (canvas fractions):
 * the full-width bands above and below, then the left and right bands
 * beside it — never negative where the box hangs off an edge. */
export function spotlightRects(box: NormRect): NormRect[] {
  const top = clamp(box.y, 0, 1);
  const bottom = clamp(box.y + box.h, top, 1);
  const left = clamp(box.x, 0, 1);
  const right = clamp(box.x + box.w, left, 1);
  const midH = bottom - top;
  return [
    { x: 0, y: 0, w: 1, h: top },
    { x: 0, y: bottom, w: 1, h: 1 - bottom },
    { x: 0, y: top, w: left, h: midH },
    { x: right, y: top, w: 1 - right, h: midH },
  ];
}

/** 0..1 ramp → smoothstep, the preview's own ease (a `smooth` fade uses it too). */
function smoothstep(k: number): number {
  return k * k * (3 - 2 * k);
}

/** How far in (0..1) the zoom is at `t`, with `easing` ms ramps at each end. */
function zoomAmount(cue: ActiveCue, t: number): number {
  const easing = cue.effect.easing ?? 0;
  if (easing <= 0) return 1;
  return smoothstep(Math.min(1, (t - cue.startMs) / easing, (cue.endMs - t) / easing));
}

/**
 * The stage transform a zoom cue asks for at output time `t` — identity
 * outside the cue's span. The window's centre is clamped so the scaled
 * canvas always covers the frame (see the module doc).
 */
export function zoomTransform(cue: ActiveCue, t: number): ZoomTransform {
  if (t < cue.startMs || t >= cue.endMs) return IDENTITY_ZOOM;
  const factor = cue.effect.factor ?? 1;
  const scale = 1 + (factor - 1) * zoomAmount(cue, t);
  const half = 1 / (2 * scale);
  const cx = clamp(cue.effect.x, half, 1 - half);
  const cy = clamp(cue.effect.y, half, 1 - half);
  return { scale, tx: 0.5 - scale * cx, ty: 0.5 - scale * cy };
}

/** The first active zoom cue's transform (the project's order decides
 * between overlapping zooms), or identity. */
export function activeZoom(cues: ActiveCue[], t: number): ZoomTransform {
  const zoom = cues.find((c) => c.effect.kind === "zoom");
  return zoom ? zoomTransform(zoom, t) : IDENTITY_ZOOM;
}

/** A point on the ZOOMED stage (canvas fractions) back to the canvas point
 * under it — what a pointer on a zoomed preview actually touches. */
export function unzoomPoint(zoom: ZoomTransform, p: NormPoint): NormPoint {
  return { x: (p.x - zoom.tx) / zoom.scale, y: (p.y - zoom.ty) / zoom.scale };
}

/** A zoom as an SVG `transform` over a `viewBox` of the canvas in px. */
export function svgZoomTransform(zoom: ZoomTransform, canvas: Size): string {
  return `translate(${zoom.tx * canvas.width} ${zoom.ty * canvas.height}) scale(${zoom.scale})`;
}
