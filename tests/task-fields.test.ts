import { describe, expect, it } from "vitest";

import type { TaskItem } from "../src/types";
import { plannerDateOf, relativeDateLabel, scheduledOf, shortDate } from "../src/utils/taskFields";

function task(p: Partial<TaskItem>): TaskItem {
  return {
    path: "p", title: "t", status: "new", created: "2026-07-01", done: false,
    due: null, scheduled: null, priority: null, tags: [], list: "", order: null, id: null, ...p,
  };
}

describe("scheduledOf", () => {
  it("returns a plain YYYY-MM-DD, null otherwise", () => {
    expect(scheduledOf(task({ scheduled: "2026-07-20" }))).toBe("2026-07-20");
    expect(scheduledOf(task({ scheduled: "next week" }))).toBeNull();
    expect(scheduledOf(task({ scheduled: null }))).toBeNull();
  });
});

describe("plannerDateOf", () => {
  it("prefers scheduled, falls back to due", () => {
    expect(plannerDateOf(task({ scheduled: "2026-07-20", due: "2026-07-10" }))).toBe("2026-07-20");
    expect(plannerDateOf(task({ scheduled: null, due: "2026-07-10" }))).toBe("2026-07-10");
    expect(plannerDateOf(task({ scheduled: null, due: null }))).toBeNull();
  });
});

describe("relativeDateLabel", () => {
  it("labels Today / Tomorrow / weekday / short date", () => {
    const today = "2026-07-24"; // a Friday
    expect(relativeDateLabel("2026-07-24", today)).toBe("Today");
    expect(relativeDateLabel("2026-07-25", today)).toBe("Tomorrow");
    expect(relativeDateLabel("2026-07-27", today)).toBe("Mon"); // within the next 6 days
    expect(relativeDateLabel("2026-08-10", today)).toBe(shortDate("2026-08-10")); // far → short date
    expect(relativeDateLabel("2026-07-20", today)).toBe("Jul 20"); // past → short date
  });

  it("falls back to the literal shortDate for a shape-valid but calendar-invalid date", () => {
    // is_valid_due checks only the YYYY-MM-DD shape, never calendar validity, so
    // a stored "2026-02-31" is possible (Obsidian's own date picker tolerates it
    // too). `new Date("2026-02-31T00:00:00")` silently normalizes to March 3.
    // `today` is deliberately "2026-03-01" so the normalized date lands just 2
    // days out — INSIDE the (1,7) weekday window — so WITHOUT the round-trip
    // guard this returns a weekday ("Tue" for Mar 3), and the guard is what makes
    // it "Feb 31". A far-apart `today` (e.g. "2026-02-01", 30 days out) falls
    // through to shortDate whether or not the guard exists, passing vacuously —
    // this fixture is what actually PINS the guard against a future regression.
    expect(relativeDateLabel("2026-02-31", "2026-03-01")).toBe(shortDate("2026-02-31"));
    expect(relativeDateLabel("2026-02-31", "2026-03-01")).toBe("Feb 31");
  });
});
