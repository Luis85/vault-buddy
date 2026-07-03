import { describe, expect, it } from "vitest";
import { planPanelPlacement } from "../src/composables/companionPlacement";
import { COLLAPSED, EXPANDED } from "../src/composables/useCompanionWindow";

// 1920x1080 monitor at origin, scale 1
const monitor = { x: 0, y: 0, width: 1920, height: 1080 };

const plan = (pos: { x: number; y: number }, scale = 1, mon = monitor as typeof monitor | null) =>
  planPanelPlacement(pos, mon, scale, COLLAPSED, EXPANDED);

describe("planPanelPlacement", () => {
  it("opens right/down with no offset when there is room", () => {
    expect(plan({ x: 100, y: 100 })).toEqual({
      side: "right",
      valign: "down",
      offset: { x: 0, y: 0 },
    });
  });

  it("opens left when the expanded window would overflow the right edge", () => {
    // collapsed window hugging the right edge: 1920 - 140 = 1780
    const p = plan({ x: 1780, y: 100 });
    expect(p.side).toBe("left");
    // window must shift left by the width delta so the buddy stays put
    expect(p.offset.x).toBe(EXPANDED.width - COLLAPSED.width);
    expect(p.valign).toBe("down");
  });

  it("opens up when the expanded window would overflow the bottom edge", () => {
    const p = plan({ x: 100, y: 1080 - COLLAPSED.height });
    expect(p.valign).toBe("up");
    expect(p.offset.y).toBe(EXPANDED.height - COLLAPSED.height);
    expect(p.side).toBe("right");
  });

  it("opens left and up in the bottom-right corner", () => {
    const p = plan({ x: 1780, y: 910 });
    expect(p).toEqual({
      side: "left",
      valign: "up",
      offset: {
        x: EXPANDED.width - COLLAPSED.width,
        y: EXPANDED.height - COLLAPSED.height,
      },
    });
  });

  it("scales offsets by the monitor scale factor", () => {
    // scale 2: physical monitor is 3840x2160 for the same logical monitor
    const p = plan({ x: 3560, y: 100 }, 2, {
      x: 0,
      y: 0,
      width: 3840,
      height: 2160,
    });
    expect(p.side).toBe("left");
    expect(p.offset.x).toBe((EXPANDED.width - COLLAPSED.width) * 2);
  });

  it("stays right/down when shifting left would leave the monitor", () => {
    // monitor narrower than the expanded window: nowhere to go
    const p = plan({ x: 10, y: 10 }, 1, { x: 0, y: 0, width: 400, height: 1080 });
    expect(p.side).toBe("right");
    expect(p.offset).toEqual({ x: 0, y: 0 });
  });

  it("falls back to right/down when the monitor is unknown", () => {
    expect(plan({ x: 5000, y: 5000 }, 1, null)).toEqual({
      side: "right",
      valign: "down",
      offset: { x: 0, y: 0 },
    });
  });
});
