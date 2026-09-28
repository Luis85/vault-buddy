/**
 * The ruler's tick step and labels (visual-parity Task 16; concept spec
 * §6.3): `session-safety.js`' `steps.find(n => n * scale >= 70)` and
 * `drawing-primitives.js`' `fmt`, as pure functions.
 *
 * The app's pixels per second come from the zoom alone
 * (`pxPerMs(zoom) * 1000` = 50 × zoom), not from the window width the
 * concept divides by — so at the default zoom (1) the ruler reads 2 s ticks
 * at every width, and 5 s ticks need a zoomed-out timeline (Fit on a long
 * edit, or the zoom slider below 0.7).
 */
import { describe, expect, it } from "vitest";

import { formatTick, rulerTicks, TICK_STEPS_S, tickStep } from "../src/editor/rulerTicks";
import { pxPerMs } from "../src/editor/timelineLayout";

const ppsAt = (zoom: number) => pxPerMs(zoom) * 1000;

describe("tickStep", () => {
  it("is the first step whose spacing reaches 70px", () => {
    // The default zoom: 50 px/s, so 1 s (50px) is too tight and 2 s (100px) is the first to fit.
    expect(tickStep(ppsAt(1))).toBe(2);
    // Zoomed out to 20 px/s: 2 s is 40px, 5 s is 100px.
    expect(tickStep(20)).toBe(5);
    // Exactly 70px qualifies (>=, not >).
    expect(tickStep(70)).toBe(1);
    expect(tickStep(69.9)).toBe(2);
    // Zoomed right in: tenths.
    expect(tickStep(700)).toBe(0.1);
    expect(tickStep(350)).toBe(0.2);
  });

  it("uses the concept's steps, in order", () => {
    expect(TICK_STEPS_S).toEqual([0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600]);
    for (const step of TICK_STEPS_S) expect(tickStep(70 / step)).toBe(step);
  });

  it("falls back to the widest step when nothing reaches 70px, or the scale is degenerate", () => {
    expect(tickStep(0.01)).toBe(600);
    expect(tickStep(0)).toBe(600);
    expect(tickStep(-5)).toBe(600);
  });
});

describe("formatTick", () => {
  it("reads MM:SS, with tenths only when the step is under a second", () => {
    expect(formatTick(2000, 2)).toBe("00:02");
    expect(formatTick(500, 0.5)).toBe("00:00.5");
    expect(formatTick(75_000, 15)).toBe("01:15");
    expect(formatTick(61_300, 0.1)).toBe("01:01.3");
    expect(formatTick(0, 1)).toBe("00:00");
  });

  it("absorbs float noise instead of flooring it away", () => {
    // 7 × 0.1 s is 0.7000000000000001 s, and 0.7 × 1000 is 699.99…
    expect(formatTick(0.7 * 1000 - 1e-9, 0.1)).toBe("00:00.7");
    expect(formatTick(-3, 1)).toBe("00:00");
  });
});

describe("rulerTicks", () => {
  it("places a tick every step across the width, at step × pps", () => {
    const ticks = rulerTicks(50, 450);
    expect(ticks.map((t) => t.ms)).toEqual([0, 2000, 4000, 6000, 8000]);
    expect(ticks.map((t) => t.x)).toEqual([0, 100, 200, 300, 400]);
    expect(ticks.map((t) => t.label)).toEqual(["00:00", "00:02", "00:04", "00:06", "00:08"]);
  });

  it("keeps sub-second ticks on whole milliseconds", () => {
    const ticks = rulerTicks(700, 500);
    expect(ticks.slice(0, 8).map((t) => t.ms)).toEqual([0, 100, 200, 300, 400, 500, 600, 700]);
    expect(ticks[7].label).toBe("00:00.7");
  });

  it("never renders a runaway number of ticks", () => {
    expect(rulerTicks(0.0001, 1e12).length).toBeLessThanOrEqual(2000);
    expect(rulerTicks(0, 1000)).toEqual([{ ms: 0, x: 0, label: "00:00" }]);
  });
});
