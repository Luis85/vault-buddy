/**
 * Where a visual layer's media sits INSIDE its frame (Task 31; F-21,
 * F-23): fit, crop zoom and anchor, the quarter-turn rotation, the mirror
 * and the vertical flip — a pure function the preview controller applies
 * to the media element inside each layer's clipping frame
 * (`previewController.ts`). The frame itself is the clip's box
 * (`previewGeometry.clipBox`) with its shape (`radius`).
 *
 * The arithmetic is the reference compositor's (`reference/webcam.js`
 * `drawVideoImage`, `editing-features.js`'s rotated-frame wrapper), so the
 * preview and a render built from the same rules agree:
 * - the SOURCE is flipped vertically, then rotated — a 90°/270° turn swaps
 *   its width and height before anything is fitted;
 * - `contain` scales the whole turned source into the frame, centred;
 * - `cover` cuts a window of the frame's own aspect out of the turned
 *   source, shrinks it by `cropZoom`, centres it on the anchor
 *   (`cropX`/`cropY`, 0..1 of the source) and clamps it inside the source,
 *   then scales that window to fill the frame;
 * - the mirror flips the finished FRAME horizontally (the window's position
 *   mirrors too, not just the pixels).
 *
 * A source whose pixel size is unknown (an asset with no recorded
 * width/height) is treated as the frame's own shape and falls back to CSS
 * `object-fit` for the actual pixels: an approximation, but never a
 * stretched picture. What the preview still does NOT model is recorded in
 * docs/Gaps.md GAP-173.
 */
import type { Asset, Clip, Fit, FrameShape, Rotation } from "../editorTypes";
import type { Size } from "./previewGeometry";

/** A layer's look — every render-affecting field of the clip that is not
 * its box, opacity or timing. */
export interface LayerLook {
  shape: FrameShape;
  fit: Fit;
  rotation: Rotation;
  mirror: boolean;
  flipY: boolean;
  cropZoom: number;
  cropX: number;
  cropY: number;
  /** The source's own pixel size, `null` when the asset does not say. */
  sourceWidth: number | null;
  sourceHeight: number | null;
}

/** The reference's defaults for every look field a clip may leave unset:
 * the whole picture, unturned, uncropped (zoom 1, centred anchor). */
const LOOK_DEFAULTS = {
  fit: "contain" as Fit,
  rotation: 0 as Rotation,
  mirror: false,
  flip_y: false,
  crop_zoom: 1,
  crop_x: 0.5,
  crop_y: 0.5,
};
type LookKey = keyof typeof LOOK_DEFAULTS;

function lookField<K extends LookKey>(clip: Clip, key: K): (typeof LOOK_DEFAULTS)[K] {
  return (clip[key] ?? LOOK_DEFAULTS[key]) as (typeof LOOK_DEFAULTS)[K];
}

/** A clip's frame shape: its own, else the reference's default — `rounded`
 * for a picture-in-picture box (narrower than 0.98 of the canvas) and a
 * plain `rectangle` for a full-frame one. */
export function shapeOf(clip: Pick<Clip, "frame_shape" | "w">): FrameShape {
  if (clip.frame_shape) return clip.frame_shape;
  return clip.w < 0.98 ? "rounded" : "rectangle";
}

/** A positive recorded dimension, or `null`. */
function dimension(v: number | undefined): number | null {
  return typeof v === "number" && v > 0 ? v : null;
}

/** A clip's look, with the asset's recorded pixel size. */
export function layerLook(clip: Clip, asset: Asset): LayerLook {
  return {
    shape: shapeOf(clip),
    fit: lookField(clip, "fit"),
    rotation: lookField(clip, "rotation"),
    mirror: lookField(clip, "mirror"),
    flipY: lookField(clip, "flip_y"),
    cropZoom: lookField(clip, "crop_zoom"),
    cropX: lookField(clip, "crop_x"),
    cropY: lookField(clip, "crop_y"),
    sourceWidth: dimension(asset.width),
    sourceHeight: dimension(asset.height),
  };
}

/** The media element's placement, in pixels relative to its frame. */
export interface MediaPlacement {
  left: number;
  top: number;
  width: number;
  height: number;
  /** CSS `transform` (about the element's centre), or `"none"`. */
  transform: string;
  /** `fill` when the rect above already has the source's shape. */
  objectFit: "fill" | Fit;
  /** CSS `border-radius` for the frame. */
  radius: string;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), hi);
}

/** The reference's rounded corner: 7.5% of the frame's shorter side. */
function frameRadius(shape: FrameShape, frame: Size): string {
  if (shape === "circle") return "50%";
  if (shape === "rounded") return `${Math.min(frame.width, frame.height) * 0.075}px`;
  return "0";
}

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The turned source drawn into the frame, in frame pixels. */
function drawnRect(frame: Size, look: LayerLook, rw: number, rh: number): Rect {
  if (look.fit === "contain") {
    const s = Math.min(frame.width / rw, frame.height / rh);
    return { x: (frame.width - rw * s) / 2, y: (frame.height - rh * s) / 2, w: rw * s, h: rh * s };
  }
  const dest = frame.width / frame.height;
  let winW = rw;
  let winH = rh;
  if (rw / rh > dest) winW = rh * dest;
  else winH = rw / dest;
  winW /= look.cropZoom;
  winH /= look.cropZoom;
  const sx = clamp(look.cropX * rw - winW / 2, 0, rw - winW);
  const sy = clamp(look.cropY * rh - winH / 2, 0, rh - winH);
  const s = frame.width / winW;
  return { x: -sx * s, y: -sy * s, w: rw * s, h: rh * s };
}

function transformOf(look: LayerLook): string {
  const parts: string[] = [];
  if (look.mirror) parts.push("scaleX(-1)");
  if (look.rotation !== 0) parts.push(`rotate(${look.rotation}deg)`);
  if (look.flipY) parts.push("scaleY(-1)");
  return parts.length > 0 ? parts.join(" ") : "none";
}

/** The source's size once turned: a quarter turn swaps its axes. An
 * unknown source is taken as the frame's own (turned) shape. */
function turnedSource(frame: Size, look: LayerLook): { rw: number; rh: number; swap: boolean } {
  const swap = look.rotation === 90 || look.rotation === 270;
  if (look.sourceWidth === null || look.sourceHeight === null) {
    return { rw: frame.width, rh: frame.height, swap };
  }
  return swap
    ? { rw: look.sourceHeight, rh: look.sourceWidth, swap }
    : { rw: look.sourceWidth, rh: look.sourceHeight, swap };
}

export function mediaPlacement(frame: Size, look: LayerLook): MediaPlacement {
  const known = look.sourceWidth !== null && look.sourceHeight !== null;
  const { rw, rh, swap } = turnedSource(frame, look);
  const empty = frame.width <= 0 || frame.height <= 0;
  const drawn = empty ? { x: 0, y: 0, w: 0, h: 0 } : drawnRect(frame, look, rw, rh);
  const x = look.mirror ? frame.width - drawn.x - drawn.w : drawn.x;
  // The element is laid out UNTURNED (source-shaped) and turned about its
  // centre, so its own box is the drawn rect with the axes swapped back.
  const width = swap ? drawn.h : drawn.w;
  const height = swap ? drawn.w : drawn.h;
  return {
    left: x + drawn.w / 2 - width / 2,
    top: drawn.y + drawn.h / 2 - height / 2,
    width,
    height,
    transform: transformOf(look),
    objectFit: known ? "fill" : look.fit,
    radius: frameRadius(look.shape, frame),
  };
}
