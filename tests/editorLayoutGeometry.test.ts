/**
 * The pure picture-in-picture arithmetic (Task 31; F-21, F-23): how a box
 * is clamped, moved and resized from its eight handles
 * (`layoutGeometry.ts`), and where a picture sits inside its box — fit,
 * crop, quarter turns, mirror and flip (`previewTransform.ts`). The corner
 * presets and the handles themselves are `editorPipLayout.test.ts`.
 * Fixtures are asymmetric (a box with four different numbers, a 16:9
 * source in a 4:3 frame) so a swapped axis fails.
 */
import { describe, expect, it } from "vitest";

import { clampBox, MIN_SIZE, moveBox, resizeFromHandle } from "../src/editor/layoutGeometry";
import type { LayerLook } from "../src/editor/previewTransform";
import { mediaPlacement } from "../src/editor/previewTransform";

describe("clampBox / moveBox / resizeFromHandle", () => {
  const BOX = { x: 0.5, y: 0.2, w: 0.3, h: 0.4 };

  it("clampBox keeps the box inside the frame and inside [0.1, 1]", () => {
    expect(clampBox({ x: 0.9, y: -0.2, w: 0.3, h: 0.05 })).toEqual({ x: 0.7, y: 0, w: 0.3, h: MIN_SIZE });
    expect(moveBox(BOX, 0.4, -0.5)).toEqual({ x: 0.7, y: 0, w: 0.3, h: 0.4 });
  });

  it("an edge handle moves only its own edge; the opposite one stays put", () => {
    const e = resizeFromHandle(BOX, "e", 0.1, 0.3, false);
    expect(e.x).toBeCloseTo(0.5, 9);
    expect(e.w).toBeCloseTo(0.4, 9);
    expect(e.y).toBe(0.2);
    expect(e.h).toBeCloseTo(0.4, 9);
    const nw = resizeFromHandle(BOX, "nw", 0.1, 0.05, false);
    expect([nw.x, nw.y, nw.w, nw.h].map((v) => +v.toFixed(9))).toEqual([0.6, 0.25, 0.2, 0.35]);
  });

  it("no edge crosses its opposite closer than the minimum size", () => {
    const w = resizeFromHandle(BOX, "w", 0.9, 0, false);
    expect(w.w).toBeCloseTo(MIN_SIZE, 9);
    expect(w.x + w.w).toBeCloseTo(0.8, 9);
  });

  it("keepAspect from an edge scales that axis and keeps the other centred", () => {
    const w = resizeFromHandle(BOX, "w", -0.15, 0, true);
    expect(w.x + w.w).toBeCloseTo(0.8, 9);
    expect(w.w).toBeCloseTo(0.45, 9);
    expect(w.h).toBeCloseTo(0.6, 9);
    expect(w.y + w.h / 2).toBeCloseTo(0.4, 9);
    const n = resizeFromHandle(BOX, "n", 0, 0.1, true);
    expect(n.y + n.h).toBeCloseTo(0.6, 9);
    expect(n.h).toBeCloseTo(0.3, 9);
    expect(n.x + n.w / 2).toBeCloseTo(0.65, 9);
  });

  it("keepAspect scales from the opposite corner and keeps the ratio", () => {
    const se = resizeFromHandle(BOX, "se", 0.15, 0.02, true);
    expect(se.w / se.h).toBeCloseTo(BOX.w / BOX.h, 9);
    expect([se.x, se.y]).toEqual([0.5, 0.2]);
    expect(se.w).toBeCloseTo(0.45, 9);
    // Growing past the frame stops at the frame, still in ratio.
    const huge = resizeFromHandle(BOX, "se", 2, 2, true);
    expect(huge.x + huge.w).toBeLessThanOrEqual(1 + 1e-9);
    expect(huge.y + huge.h).toBeLessThanOrEqual(1 + 1e-9);
    expect(huge.w / huge.h).toBeCloseTo(BOX.w / BOX.h, 9);
  });
});

describe("mediaPlacement", () => {
  const LOOK: LayerLook = {
    shape: "rectangle",
    fit: "contain",
    rotation: 0,
    mirror: false,
    flipY: false,
    cropZoom: 1,
    cropX: 0.5,
    cropY: 0.5,
    sourceWidth: 1600,
    sourceHeight: 900,
  };
  const FRAME = { width: 400, height: 300 };

  it("contain letterboxes the source, centred", () => {
    const m = mediaPlacement(FRAME, LOOK);
    expect([m.left, m.top, m.width, m.height]).toEqual([0, 37.5, 400, 225]);
    expect(m.transform).toBe("none");
    expect(m.objectFit).toBe("fill");
  });

  it("cover crops a window of the frame's aspect around the anchor, zoomed", () => {
    // 1600x900 into 4:3 -> a 1200x900 window, /2 zoom -> 600x450, centred
    // on (0.25*1600, 0.8*900) = (400, 720) and clamped to (100, 450).
    const m = mediaPlacement(FRAME, { ...LOOK, fit: "cover", cropZoom: 2, cropX: 0.25, cropY: 0.8 });
    const s = 400 / 600;
    expect(m.width).toBeCloseTo(1600 * s, 9);
    expect(m.height).toBeCloseTo(900 * s, 9);
    expect(m.left).toBeCloseTo(-100 * s, 9);
    expect(m.top).toBeCloseTo(-450 * s, 9);
  });

  it("a quarter turn swaps the source's axes and a mirror mirrors the window", () => {
    const turned = mediaPlacement(FRAME, { ...LOOK, rotation: 90 });
    // Turned, the 1600x900 source is 900x1600: contain -> 168.75 x 300 drawn,
    // so the UNTURNED element is 300 x 168.75 centred on the frame.
    expect(turned.width).toBeCloseTo(300, 9);
    expect(turned.height).toBeCloseTo(168.75, 9);
    expect(turned.left + turned.width / 2).toBeCloseTo(200, 9);
    expect(turned.transform).toBe("rotate(90deg)");

    const cover = { ...LOOK, fit: "cover" as const, cropX: 0 };
    const plain = mediaPlacement(FRAME, cover);
    const mirrored = mediaPlacement(FRAME, { ...cover, mirror: true, flipY: true });
    expect(mirrored.left).toBeCloseTo(FRAME.width - plain.left - plain.width, 9);
    expect(mirrored.transform).toBe("scaleX(-1) scaleY(-1)");
  });

  it("a circle frame is fully rounded; an unknown source falls back to object-fit", () => {
    const m = mediaPlacement(FRAME, { ...LOOK, shape: "circle", sourceWidth: null, sourceHeight: null, fit: "cover" });
    expect(m.radius).toBe("50%");
    expect(m.objectFit).toBe("cover");
    expect([m.left, m.top, m.width, m.height]).toEqual([0, 0, 400, 300]);
  });
});
