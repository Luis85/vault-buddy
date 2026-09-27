/**
 * What each of the Layout tab's one-click controls sends (visual-parity
 * Task 14; concept spec §5 "Layout", `webcam.js`, `editing-features.js`):
 * Full frame, Picture-in-picture, the corners, the frame shapes and the
 * Transform source buttons — each a `setLayout` patch computed from the
 * first selected clip, so the section only has to send it. The geometry is
 * `layoutGeometry.ts`'s, the same the preview handles use.
 */
import type { Asset, Clip, FrameShape, Rotation } from "../editorTypes";
import type { EditorCommand } from "./editorCommandTypes";
import type { Corner, NormBox } from "./layoutGeometry";
import { aspectHeight, centerBox, clampBox, cornerPreset, PIP_SIZE, roundBox } from "./layoutGeometry";
import type { Size } from "./previewGeometry";

export type LayoutPatch = Omit<Extract<EditorCommand, { kind: "setLayout" }>, "kind" | "clipIds">;

/** The layout fields the patches read: the box and its optional extras. */
export type LayoutClip = NormBox & Partial<Pick<Clip, "asset_id" | "frame_shape" | "rotation" | "mirror" | "flip_y">>;

/** A webcam picture: the synchronized webcam of a staged capture (`webcam`),
 * a recorded take (`take-…`) or the sample presenter. */
export function isWebcamAsset(asset: Asset | undefined): boolean {
  if (!asset) return false;
  return asset.builtin === "presenter" || asset.id === "webcam" || asset.id.startsWith("take-");
}

export const FULL_FRAME: LayoutPatch = { x: 0, y: 0, w: 1, h: 1, frameShape: "rectangle", fit: "contain" };

/** The default picture-in-picture: a rounded, filled box in the bottom right. */
export function pipPatch(canvas: Size, asset: Asset | undefined): LayoutPatch {
  const box = cornerPreset("br", canvas, PIP_SIZE, { width: asset?.width, height: asset?.height });
  return { ...roundBox(box), frameShape: "rounded", fit: "cover" };
}

/** A corner keeps a boxed picture's width; a full-frame one becomes the
 * default picture-in-picture size. A circle stays square in pixels. */
export function cornerPatch(corner: Corner, clip: LayoutClip, canvas: Size, asset: Asset | undefined): LayoutPatch {
  const size = clip.w < 0.98 ? clip.w : PIP_SIZE;
  const source = { width: asset?.width, height: asset?.height, circle: clip.frame_shape === "circle" };
  return roundBox(cornerPreset(corner, canvas, size, source));
}

/** A circle becomes square in pixels and fills its frame (the reference's
 * `setVideoShape`); a box that would then be taller than the canvas narrows
 * instead. Leaving a circle keeps the box as it is. */
export function shapePatch(shape: FrameShape, clip: LayoutClip, canvas: Size): LayoutPatch {
  if (shape !== "circle") return { frameShape: shape };
  const tall = aspectHeight(clip.w, canvas, 1);
  const w = tall > 1 ? clip.w / tall : clip.w;
  return { frameShape: "circle", fit: "cover", ...roundBox(clampBox({ x: clip.x, y: clip.y, w, h: Math.min(tall, 1) })) };
}

export type TransformKey = "rotate" | "flip" | "flip-y" | "center" | "fit" | "fill";

const TRANSFORMS: Record<TransformKey, (clip: LayoutClip) => LayoutPatch> = {
  rotate: (c) => ({ rotation: (((c.rotation ?? 0) + 90) % 360) as Rotation }),
  flip: (c) => ({ mirror: !(c.mirror ?? false) }),
  "flip-y": (c) => ({ flipY: !(c.flip_y ?? false) }),
  center: (c) => {
    const { x, y } = roundBox(centerBox(c));
    return { x, y };
  },
  fit: () => ({ fit: "contain" }),
  fill: () => ({ fit: "cover" }),
};

/** The Transform source buttons (`editing-features.js: transformClip`). */
export function transformPatch(key: TransformKey, clip: LayoutClip): LayoutPatch {
  return TRANSFORMS[key](clip);
}
