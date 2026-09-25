import { mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Captured so a test can drive `region:begin` the way Rust does, from the
// same main-thread closure that shows the overlay.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

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
  for (const key of Object.keys(listeners)) delete listeners[key];
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

/** Mount and wait for the async `region:begin` subscription to land. */
async function mountArmed() {
  const w = mount(RegionRoot);
  await flushPromises();
  return w;
}

/** What Rust does immediately before it shows the overlay for a NEW
 * selection. */
async function beginSelection(w: ReturnType<typeof mount>) {
  const begin = listeners["region:begin"];
  expect(begin, "RegionRoot must subscribe to region:begin").toBeTypeOf(
    "function",
  );
  begin({ payload: null });
  await w.vm.$nextTick();
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

  // The overlay window is hidden and REUSED, never reloaded, so this
  // component and its one-shot latch survive every selection. Without the
  // `region:begin` reset the second selection of an app run paints a
  // full-screen, always-on-top scrim that drops every pointerdown and every
  // Escape for the whole of Rust's 120-second wait, then reports a cancel
  // the user never made.
  it("arms a second selection when Rust says one is beginning", async () => {
    const w = await mountArmed();
    await drag(w, [0, 0], [100, 100]);
    expect(resolved()).toHaveLength(1);

    await beginSelection(w);

    // The band paints again, so the press was honoured rather than dropped
    // by the stale latch.
    await surface(w).trigger("pointerdown", { clientX: 200, clientY: 100 });
    await surface(w).trigger("pointermove", { clientX: 520, clientY: 280 });
    expect(w.find('[data-testid="region-box"]').exists()).toBe(true);
    await surface(w).trigger("pointerup", { clientX: 520, clientY: 280 });

    expect(resolved()).toHaveLength(2);
    // Hand-derived: x = 200, y = 100, width = 320, height = 180.
    expect(resolved()[1].args.rect).toMatchObject({
      x: 200,
      y: 100,
      width: 320,
      height: 180,
    });
  });

  // Escape is the other half: `report` has its own latch check, so a reset
  // that only re-enabled `onDown` would still leave the second selection
  // impossible to cancel.
  it("cancels a re-armed selection on Escape", async () => {
    const w = await mountArmed();
    await drag(w, [0, 0], [100, 100]);
    await beginSelection(w);

    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();

    expect(resolved()).toHaveLength(2);
    expect(resolved()[1].args.rect).toBeNull();
  });

  // Re-arming must not weaken the duplicate-answer guard WITHIN one
  // selection: Rust has already taken its one-shot sender out of the state
  // by then, so an extra call is a silent no-op that hides a real
  // double-fire. Only `region:begin` re-opens the latch.
  it("still swallows a duplicate answer inside a re-armed selection", async () => {
    const w = await mountArmed();
    await drag(w, [0, 0], [100, 100]);
    await beginSelection(w);
    await drag(w, [200, 100], [520, 280]);
    expect(resolved()).toHaveLength(2);

    // A second pointerup, a stray press, and an Escape — all after the
    // re-armed selection has answered.
    await surface(w).trigger("pointerup", { clientX: 600, clientY: 600 });
    await surface(w).trigger("pointerdown", { clientX: 600, clientY: 600 });
    await surface(w).trigger("pointermove", { clientX: 700, clientY: 700 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();

    expect(resolved()).toHaveLength(2);
    expect(w.find('[data-testid="region-box"]').exists()).toBe(false);
  });

  // A selection that Rust's own bounded wait timed out while the user was
  // still holding the button leaves `dragging` true with nothing ever
  // resolving. The overlay then hides and is reused, so re-arming without
  // clearing `dragging` would open the next selection already painting the
  // PREVIOUS one's rectangle.
  it("clears a leftover band when it re-arms", async () => {
    const w = await mountArmed();
    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    await surface(w).trigger("pointermove", { clientX: 410, clientY: 210 });
    expect(w.find('[data-testid="region-box"]').exists()).toBe(true);
    expect(resolved()).toHaveLength(0);

    await beginSelection(w);

    expect(w.find('[data-testid="region-box"]').exists()).toBe(false);
    expect(w.find('[data-testid="region-hint"]').exists()).toBe(true);
  });

  it("stops listening for region:begin on unmount", async () => {
    const w = await mountArmed();
    expect(listeners["region:begin"]).toBeTypeOf("function");
    w.unmount();
    await flushPromises();
    expect(listeners["region:begin"]).toBeUndefined();
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
