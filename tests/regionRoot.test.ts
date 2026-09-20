import { mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Each test mounts its own RegionRoot and several leave it mid-drag
// (unresolved) rather than driving it to a terminal state. Without
// unmounting between tests, an earlier instance's window `keydown`
// listener stays live and answers a LATER test's Escape dispatch too,
// double-counting `resolve_region_selection` calls the assertions below
// count exactly. Every other suite in this file tree that leaves a
// dangling window listener sidesteps this with `toContain` instead of
// `toHaveLength`; this suite asserts exact counts, so it needs real
// per-test teardown instead.
enableAutoUnmount(afterEach);

// `mockIPC` installs `__TAURI_INTERNALS__`, which makes `logging.ts` stop
// being a no-op and route through the log plugin — harmless, but it would
// put `plugin:log|log` calls in `calls`. Mocked for the same reason every
// other suite mocks it: the assertions read that array.
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import RegionRoot from "../src/roots/RegionRoot.vue";

type Call = { cmd: string; args: Record<string, unknown> };

let calls: Call[] = [];

beforeEach(() => {
  calls = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return Promise.resolve(null);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

function surface(w: ReturnType<typeof mount>) {
  return w.get('[data-testid="region-surface"]');
}

/** One pointer drag, in logical CSS pixels. */
async function drag(
  w: ReturnType<typeof mount>,
  from: [number, number],
  to: [number, number],
) {
  await surface(w).trigger("pointerdown", { clientX: from[0], clientY: from[1] });
  await surface(w).trigger("pointermove", { clientX: to[0], clientY: to[1] });
  await surface(w).trigger("pointerup", { clientX: to[0], clientY: to[1] });
}

function resolved() {
  return calls.filter((c) => c.cmd === "resolve_region_selection");
}

describe("RegionRoot", () => {
  it("reports a drag as a logical rectangle", async () => {
    const w = mount(RegionRoot);
    await drag(w, [100, 50], [420, 230]);
    expect(resolved()).toHaveLength(1);
    // Hand-derived: x = min(100, 420) = 100, y = min(50, 230) = 50,
    // width = |420 - 100| = 320, height = |230 - 50| = 180.
    expect(resolved()[0].args.rect).toMatchObject({
      x: 100,
      y: 50,
      width: 320,
      height: 180,
    });
  });

  // A drag up-and-left is the same rectangle. Without normalisation the
  // width and height go negative, `to_physical` reads them as zero (its
  // `non_negative` guard) and the region silently becomes 1x1.
  it("normalises a drag made up and to the left", async () => {
    const w = mount(RegionRoot);
    await drag(w, [420, 230], [100, 50]);
    expect(resolved()[0].args.rect).toMatchObject({
      x: 100,
      y: 50,
      width: 320,
      height: 180,
    });
  });

  it("shows a live size readout while dragging", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    await surface(w).trigger("pointermove", { clientX: 210, clientY: 110 });
    // Still dragging: nothing reported yet, but the user can read the size.
    expect(resolved()).toHaveLength(0);
    expect(w.get('[data-testid="region-readout"]').text()).toContain("200");
    expect(w.get('[data-testid="region-readout"]').text()).toContain("100");
  });

  it("cancels on Escape", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();
  });

  // A stray click is not a selection. Without this, a click produces a
  // 0x0 (or 1x1) region, `clamp_to_frame` refuses it and the user gets an
  // error toast for having clicked.
  it("treats a drag below the minimum as a cancel, not a tiny region", async () => {
    const w = mount(RegionRoot);
    await drag(w, [100, 100], [104, 103]);
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();
  });

  // Exactly at the threshold counts, so the boundary is pinned in both
  // directions rather than only on the reject side.
  it("accepts a drag exactly at the minimum", async () => {
    const w = mount(RegionRoot);
    await drag(w, [100, 100], [108, 108]);
    expect(resolved()[0].args.rect).toMatchObject({ width: 8, height: 8 });
  });

  it("reports the device pixel ratio alongside the rectangle", async () => {
    const w = mount(RegionRoot);
    await drag(w, [0, 0], [100, 100]);
    const rect = resolved()[0].args.rect as { dpr: number };
    expect(rect.dpr).toBe(window.devicePixelRatio);
  });

  // The overlay resolves exactly once. A second resolve would answer a
  // selection nobody asked for -- the Rust side has already taken its
  // one-shot sender out of the state by then, so the extra call is a
  // silent no-op that hides a real double-fire.
  it("resolves only once even if pointerup fires again", async () => {
    const w = mount(RegionRoot);
    await drag(w, [0, 0], [100, 100]);
    await surface(w).trigger("pointerup", { clientX: 200, clientY: 200 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();
    expect(resolved()).toHaveLength(1);
  });

  it("removes its window key listener on unmount", async () => {
    const remove = vi.spyOn(window, "removeEventListener");
    const w = mount(RegionRoot);
    w.unmount();
    expect(remove).toHaveBeenCalledWith("keydown", expect.any(Function));
  });
});
