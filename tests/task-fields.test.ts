import { afterEach, describe, expect, it, vi } from "vitest";

import type { TaskItem } from "../src/types";
import {
  buildTaskPatch,
  comingSaturday,
  localDatePlus,
  plannerDateOf,
  relativeDateLabel,
  scheduledOf,
  shortDate,
  type TaskDraft,
} from "../src/utils/taskFields";

function task(p: Partial<TaskItem>): TaskItem {
  return {
    path: "p", title: "t", status: "new", created: "2026-07-01", done: false,
    due: null, scheduled: null, priority: null, tags: [], list: "", order: null, id: null, description: null, ...p,
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

// Real clock-fixed tests for localDatePlus/comingSaturday: previously these two
// only appeared in task-schedule-menu.test.ts as self-comparisons (asserting
// TaskScheduleMenu's "Tomorrow"/"This weekend" buttons emit the SAME value
// localDatePlus(1)/comingSaturday() compute), which passes vacuously even if
// the helper itself were wrong. These fix the system clock and check the
// actual calendar output instead.
describe("localDatePlus", () => {
  afterEach(() => vi.useRealTimers());

  it("returns the correct next-day date across a year rollover", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 11, 31, 12, 0, 0)); // Dec 31, 2026 (local noon)
    expect(localDatePlus(1)).toBe("2027-01-01");
  });

  it("returns the correct next-day date across a month rollover", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 31, 12, 0, 0)); // Jan 31, 2026 (local noon)
    expect(localDatePlus(1)).toBe("2026-02-01");
  });
});

describe("comingSaturday", () => {
  afterEach(() => vi.useRealTimers());

  it("returns the upcoming Saturday from a known Wednesday", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 6, 22, 12, 0, 0)); // Jul 22, 2026 is a Wednesday
    expect(comingSaturday()).toBe("2026-07-25");
  });

  it("returns today when today is already Saturday", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 6, 25, 12, 0, 0)); // Jul 25, 2026 is a Saturday
    expect(comingSaturday()).toBe("2026-07-25");
  });
});

describe("buildTaskPatch", () => {
  const base: TaskItem = {
    path: "p", title: "Old", status: "new", created: "2026-07-01", done: false,
    due: "2026-07-10", scheduled: null, priority: null, tags: ["a"], list: "Work",
    order: null, id: null, description: null,
  };
  const draft = (o: Partial<TaskDraft> = {}): TaskDraft => ({
    title: "Old", due: "2026-07-10", scheduled: "", priority: "normal", tags: "a", list: "Work", ...o,
  });

  it("emits only changed fields", () => {
    expect(buildTaskPatch(base, draft())).toEqual({});
    expect(buildTaskPatch(base, draft({ title: "New" }))).toEqual({ title: "New" });
    expect(buildTaskPatch(base, draft({ due: "" }))).toEqual({ clearDue: true });
    expect(buildTaskPatch(base, draft({ scheduled: "2026-07-15" }))).toEqual({ scheduled: "2026-07-15" });
    expect(buildTaskPatch(base, draft({ priority: "high" }))).toEqual({ priority: "high" });
    expect(buildTaskPatch(base, draft({ tags: "a b" }))).toEqual({ tags: ["a", "b"] });
    expect(buildTaskPatch(base, draft({ list: "Home" }))).toEqual({ list: "Home" });
  });
});
