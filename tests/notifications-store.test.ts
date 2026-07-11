import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useNotificationsStore } from "../src/stores/notifications";

describe("notifications store", () => {
  beforeEach(() => { setActivePinia(createPinia()); vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("error/warning are sticky; success auto-dismisses", () => {
    const n = useNotificationsStore();
    n.error("boom");
    n.success("done");
    expect(n.items.map((i) => i.message)).toEqual(["boom", "done"]);
    vi.advanceTimersByTime(4000);
    expect(n.items.map((i) => i.message)).toEqual(["boom"]); // success gone, error stays
  });

  it("dedupes the newest identical message", () => {
    const n = useNotificationsStore();
    const a = n.error("same");
    const b = n.error("same");
    expect(a).toBe(b);
    expect(n.items).toHaveLength(1);
  });

  it("caps at MAX_ITEMS, dropping oldest", () => {
    const n = useNotificationsStore();
    for (let i = 0; i < 7; i++) n.error(`e${i}`);
    expect(n.items).toHaveLength(5);
    expect(n.items[0].message).toBe("e2");
    expect(n.items[4].message).toBe("e6");
  });

  it("dismiss removes by id; clear empties", () => {
    const n = useNotificationsStore();
    const id = n.warning("w");
    n.info("i");
    n.dismiss(id);
    expect(n.items.map((i) => i.message)).toEqual(["i"]);
    n.clear();
    expect(n.items).toEqual([]);
  });

  it("stores an action and keeps an actionable toast sticky past the success TTL", () => {
    // A "Imported X — Open it?" toast must wait for the user's decision, so an
    // action-carrying success toast overrides the normal 4s auto-dismiss.
    const n = useNotificationsStore();
    const run = vi.fn();
    n.notify("success", "Imported X", { action: { label: "Open", run } });
    const item = n.items[0]!;
    expect(item.action?.label).toBe("Open");
    expect(item.action?.run).toBe(run);
    vi.advanceTimersByTime(10000);
    expect(n.items).toHaveLength(1); // still there — the action must not vanish
  });

  it("does not dedupe actionable toasts so each keeps its own callback", () => {
    // Two imports that happen to yield the same message must not collapse into
    // one toast — the second's Open action would otherwise open the first note.
    const n = useNotificationsStore();
    const a = n.notify("success", "Imported X", {
      action: { label: "Open", run: vi.fn() },
    });
    const b = n.notify("success", "Imported X", {
      action: { label: "Open", run: vi.fn() },
    });
    expect(a).not.toBe(b);
    expect(n.items).toHaveLength(2);
  });

  it("dedupe-reuse restarts the TTL (GAP-32)", () => {
    // A re-raise at t=3.9s used to vanish at t=4.0s and read as flicker.
    const n = useNotificationsStore();
    const id = n.success("saved");
    vi.advanceTimersByTime(3_900);
    expect(n.notify("success", "saved")).toBe(id); // dedupe-reuse
    vi.advanceTimersByTime(3_000); // 6.9s after first, 3.0s after reuse
    expect(n.items.some((i) => i.id === id)).toBe(true);
    vi.advanceTimersByTime(1_100);
    expect(n.items.some((i) => i.id === id)).toBe(false);
  });
});
