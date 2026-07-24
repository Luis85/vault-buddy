import { describe, expect, it } from "vitest";

import type { TaskItem } from "../src/types";
import { plannerDateOf, scheduledOf } from "../src/utils/taskFields";

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
