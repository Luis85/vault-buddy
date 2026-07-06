import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
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
});
