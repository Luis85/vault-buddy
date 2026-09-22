/**
 * The preview's pure geometry (Task 22; DATA-MODEL.md § "Clip placement
 * uses normalized output-canvas `x,y,w,h`. The reference preview may
 * letterbox the canvas; pointer coordinates must first undo CSS
 * sizing/letterboxing, then map into output coordinates").
 *
 * Split out of `previewController.ts` so the arithmetic every preview
 * surface depends on — where the canvas sits inside the stage, where a clip
 * sits inside the canvas, where a pointer lands in output pixels — is
 * testable without a DOM, an `HTMLVideoElement` or an `AudioContext`.
 *
 * Every box here is in STAGE pixels (the preview stage element's own
 * content box, origin top-left), never window/client pixels: the only
 * function that ever sees a client coordinate is `clientToCanvas`, and it
 * subtracts the stage's own client rect first.
 */

export interface Size {
  width: number;
  height: number;
}

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The canvas `contain`-fitted into the stage: the largest box with the
 * canvas's own aspect ratio that fits, centred on both axes (letterboxed
 * top/bottom or pillarboxed left/right). A degenerate stage or canvas
 * yields an empty box at the stage's centre rather than `NaN`. */
export function containRect(canvas: Size, stage: Size): Box {
  if (canvas.width <= 0 || canvas.height <= 0 || stage.width <= 0 || stage.height <= 0) {
    return { left: Math.max(0, stage.width) / 2, top: Math.max(0, stage.height) / 2, width: 0, height: 0 };
  }
  const scale = Math.min(stage.width / canvas.width, stage.height / canvas.height);
  const width = canvas.width * scale;
  const height = canvas.height * scale;
  return { left: (stage.width - width) / 2, top: (stage.height - height) / 2, width, height };
}

/** A clip's normalized `x,y,w,h` (fractions of the output canvas, origin
 * top-left) placed inside the letterboxed canvas box. */
export function clipBox(
  canvasBox: Box,
  clip: { x: number; y: number; w: number; h: number },
): Box {
  return {
    left: canvasBox.left + clip.x * canvasBox.width,
    top: canvasBox.top + clip.y * canvasBox.height,
    width: clip.w * canvasBox.width,
    height: clip.h * canvasBox.height,
  };
}

/**
 * A pointer event's client coordinates mapped into OUTPUT-canvas pixels
 * (`canvas.width` × `canvas.height`), undoing the stage's own position in
 * the window and the letterboxing inside it. A point in a letterbox bar maps
 * outside `[0, width) × [0, height)` rather than being clamped — a caller
 * deciding "was that on the canvas at all" needs to be able to tell.
 */
export function clientToCanvas(
  evt: { clientX: number; clientY: number },
  stageRect: { left: number; top: number; width: number; height: number },
  canvas: Size,
): { x: number; y: number } {
  const box = containRect(canvas, stageRect);
  if (box.width === 0 || box.height === 0) return { x: 0, y: 0 };
  const localX = evt.clientX - stageRect.left - box.left;
  const localY = evt.clientY - stageRect.top - box.top;
  return {
    x: (localX / box.width) * canvas.width,
    y: (localY / box.height) * canvas.height,
  };
}
