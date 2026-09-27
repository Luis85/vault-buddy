/**
 * Which clip a click on the preview picture lands on (visual-parity Task 11;
 * design D15, no-op audit finding 5): the TOPMOST clip whose picture covers
 * the point at the current output time — media layers and title cards
 * alike, on visible tracks only, audio never (it has no picture).
 *
 * Pure: the layer rules are `previewLayers`/`previewCardLayer`'s own, laid
 * out on a stage the canvas's own size, so every box is in output pixels
 * and the point only needs scaling from canvas fractions. A layer faded to
 * nothing is not a picture either: a click passes through it to whatever
 * shows underneath, as the eye would expect.
 */
import type { Project } from "../editorTypes";
import { computeCardLayers } from "./previewCardLayer";
import { computeLayers } from "./previewLayers";

/** Monitoring does not change what is on screen; any value will do. */
const MONITOR = { muted: true, volume: 0 };

interface Picture {
  clipId: string;
  box: { left: number; top: number; width: number; height: number } | null;
  opacity: number;
  z: number;
}

/** `p` (output pixels) is inside the picture's box. */
function covers(picture: Picture, x: number, y: number): boolean {
  const b = picture.box;
  return b !== null && picture.opacity > 0 && x >= b.left && x < b.left + b.width && y >= b.top && y < b.top + b.height;
}

/** The id of the topmost visible clip under `point` (canvas fractions), or
 * `null` when the point is on no picture. */
export function clipAtPoint(project: Project, timeMs: number, point: { x: number; y: number }): string | null {
  const stage = { width: project.canvas.width, height: project.canvas.height };
  const pictures: Picture[] = [...computeLayers(project, timeMs, stage, MONITOR), ...computeCardLayers(project, timeMs, stage)];
  pictures.sort((a, b) => b.z - a.z);
  const hit = pictures.find((p) => covers(p, point.x * stage.width, point.y * stage.height));
  return hit ? hit.clipId : null;
}
