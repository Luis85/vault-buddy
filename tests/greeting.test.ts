import { describe, expect, it, vi } from "vitest";
import {
  daypartFor,
  isWeekend,
  greetingFor,
  type Daypart,
} from "../src/greeting";

// Local-time constructor: new Date(y, monthIndex, day, hour, min) uses the
// runtime's local zone, and daypartFor/isWeekend read getHours()/getDay(),
// so these assertions hold regardless of the CI timezone.
// Jan 2026: 1st = Thu, 3rd = Sat, 4th = Sun, 5th = Mon.
const weekdayAt = (h: number) => new Date(2026, 0, 5, h, 0); // Monday
const weekendAt = (h: number) => new Date(2026, 0, 3, h, 0); // Saturday

describe("daypartFor", () => {
  it("buckets each hour into the right daypart at the boundaries", () => {
    expect(daypartFor(weekdayAt(4))).toBe("night");
    expect(daypartFor(weekdayAt(5))).toBe("morning");
    expect(daypartFor(weekdayAt(11))).toBe("morning");
    expect(daypartFor(weekdayAt(12))).toBe("afternoon");
    expect(daypartFor(weekdayAt(16))).toBe("afternoon");
    expect(daypartFor(weekdayAt(17))).toBe("evening");
    expect(daypartFor(weekdayAt(21))).toBe("evening");
    expect(daypartFor(weekdayAt(22))).toBe("night");
    expect(daypartFor(weekdayAt(0))).toBe("night");
  });
});

describe("isWeekend", () => {
  it("is true only for Saturday and Sunday", () => {
    expect(isWeekend(new Date(2026, 0, 5))).toBe(false); // Mon
    expect(isWeekend(new Date(2026, 0, 1))).toBe(false); // Thu
    expect(isWeekend(new Date(2026, 0, 3))).toBe(true); // Sat
    expect(isWeekend(new Date(2026, 0, 4))).toBe(true); // Sun
  });
});

describe("greetingFor", () => {
  it("selects from the daypart+weekday cell via the injected pick", () => {
    const pick = vi.fn(() => 0);
    const msg = greetingFor(weekdayAt(9), pick); // morning, weekday
    expect(pick).toHaveBeenCalledWith(3); // exactly 3 phrases per cell
    expect(typeof msg).toBe("string");
    expect(msg.length).toBeGreaterThan(0);
  });

  it("selects from the weekend cell on weekends", () => {
    const weekday = greetingFor(weekdayAt(9), () => 0);
    const weekend = greetingFor(weekendAt(9), () => 0);
    expect(weekend).not.toBe(weekday); // different cell, distinct copy
  });

  it("returns a non-empty phrase for every daypart and day type", () => {
    const dayparts: Daypart[] = ["morning", "afternoon", "evening", "night"];
    const hours: Record<Daypart, number> = {
      morning: 9,
      afternoon: 14,
      evening: 19,
      night: 23,
    };
    for (const dp of dayparts) {
      for (const at of [weekdayAt(hours[dp]), weekendAt(hours[dp])]) {
        for (let i = 0; i < 3; i++) {
          const msg = greetingFor(at, () => i);
          expect(msg.length).toBeGreaterThan(0);
        }
      }
    }
  });
});
