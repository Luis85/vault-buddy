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

import { logWarning } from "../src/logging";
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

  // Asserted against a LITERAL 1.5, not against `window.devicePixelRatio`:
  // happy-dom's ratio is 1, so comparing the payload to the same global the
  // component read made the test `1 === 1` — it passed just as happily with
  // `dpr` hard-coded, which is the one thing it exists to catch. Rust logs a
  // disagreement between this and the monitor's scale factor, so a constant
  // here would silence that diagnostic on every fractional-scaling machine.
  it("reports the device pixel ratio alongside the rectangle", async () => {
    const original = window.devicePixelRatio;
    (window as unknown as { devicePixelRatio: number }).devicePixelRatio = 1.5;
    try {
      const w = mount(RegionRoot);
      await drag(w, [0, 0], [100, 100]);
      const rect = resolved()[0].args.rect as { dpr: number };
      expect(rect.dpr).toBe(1.5);
    } finally {
      (window as unknown as { devicePixelRatio: number }).devicePixelRatio =
        original;
    }
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

  // The rectangle the user SEES. The reported rect was pinned above, but the
  // band's own `style` was not: anchoring it at the raw start corner (or
  // swapping width and height) paints a box that disagrees with the region
  // actually captured, and every existing assertion still passed.
  it("paints the band at the normalised box, not at the start corner", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 420, clientY: 230 });
    await surface(w).trigger("pointermove", { clientX: 100, clientY: 50 });
    // Hand-derived, same drag as "normalises a drag made up and to the left":
    // x = min(420, 100) = 100, y = min(230, 50) = 50, width = 320, height = 180.
    const style = (w.get('[data-testid="region-box"]').element as HTMLElement)
      .style;
    expect(style.left).toBe("100px");
    expect(style.top).toBe("50px");
    expect(style.width).toBe("320px");
    expect(style.height).toBe("180px");
  });

  // ONLY Escape cancels. The listener is on `window`, so every keystroke the
  // overlay ever sees reaches it; without this, Tab (or a stray modifier)
  // silently threw away a selection the user was still dragging.
  it("ignores keys other than Escape", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    for (const key of ["Tab", "Shift", "a", "Enter"]) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key }));
    }
    await w.vm.$nextTick();
    expect(resolved()).toHaveLength(0);
    // Still live: the band is painted, so the drag was not torn down either.
    expect(w.find('[data-testid="region-box"]').exists()).toBe(true);
  });

  // "Below the minimum on EITHER axis" — the guard's own wording. A single
  // test with both axes under it cannot tell `||` from `&&`, and under `&&`
  // a 4-pixel-wide sliver is accepted as a region.
  it("treats a sliver under the minimum on one axis as a cancel", async () => {
    const wide = mount(RegionRoot);
    await drag(wide, [100, 100], [104, 300]); // width 4, height 200
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();

    calls = [];
    const tall = mount(RegionRoot);
    await drag(tall, [100, 100], [300, 105]); // width 200, height 5
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();
  });

  // A right- or middle-press is not a selection gesture. AGENTS.md documents
  // that the buddy drag re-checks the logical primary button for the same
  // reason: a drag started by the wrong button surprises.
  it("ignores a non-primary pointer button", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 100, clientY: 100, button: 2 });
    await surface(w).trigger("pointermove", { clientX: 400, clientY: 400 });
    expect(w.find('[data-testid="region-box"]').exists()).toBe(false);
    await surface(w).trigger("pointerup", { clientX: 400, clientY: 400 });
    expect(resolved()).toHaveLength(0);
  });

  // A cancelled pointer (OS gesture, lost capture) otherwise leaves the band
  // painted and `dragging` true with nothing ever resolving — the overlay
  // would sit there until Rust's own bounded wait expired.
  it("cancels the selection when the pointer is cancelled", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 100, clientY: 100 });
    await surface(w).trigger("pointermove", { clientX: 400, clientY: 400 });
    await surface(w).trigger("pointercancel");
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();
    expect(w.find('[data-testid="region-box"]').exists()).toBe(false);
  });

  // ...but a cancel arriving outside a drag must not answer for the user: the
  // overlay's one reply is the selection, and a stray cancel is not one.
  it("ignores a pointer cancel outside a drag", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointercancel");
    expect(resolved()).toHaveLength(0);
  });

  // After the one answer is sent, a fresh press must not repaint a live band:
  // it can never report, so it is a rubber band that lies.
  it("ignores a fresh press after it has already resolved", async () => {
    const w = mount(RegionRoot);
    await drag(w, [0, 0], [100, 100]);
    await surface(w).trigger("pointerdown", { clientX: 200, clientY: 200 });
    await surface(w).trigger("pointermove", { clientX: 400, clientY: 400 });
    expect(w.find('[data-testid="region-box"]').exists()).toBe(false);
    expect(resolved()).toHaveLength(1);
  });

  // The repo's "no swallowed error" invariant, at this file's only catch.
  it("warns instead of swallowing a failed resolve", async () => {
    vi.mocked(logWarning).mockClear();
    mockIPC((cmd) => {
      if (cmd === "resolve_region_selection") {
        return Promise.reject(new Error("overlay already gone"));
      }
      return Promise.resolve(null);
    });
    const w = mount(RegionRoot);
    await drag(w, [0, 0], [100, 100]);
    await vi.waitFor(() => expect(logWarning).toHaveBeenCalledTimes(1));
    expect(vi.mocked(logWarning).mock.calls[0][0]).toContain(
      "resolve_region_selection failed",
    );
  });

  // Identity, not `expect.any(Function)`: with the loose matcher, cleanup that
  // removes a DIFFERENT closure (leaking the real listener onto `window` for
  // every later mount) passed this test unchanged.
  it("removes the same window key listener it added, on unmount", async () => {
    const add = vi.spyOn(window, "addEventListener");
    const remove = vi.spyOn(window, "removeEventListener");
    const w = mount(RegionRoot);
    const added = add.mock.calls.find(([type]) => type === "keydown")?.[1];
    expect(added).toBeTypeOf("function");
    w.unmount();
    expect(remove).toHaveBeenCalledWith("keydown", added);
  });
});
