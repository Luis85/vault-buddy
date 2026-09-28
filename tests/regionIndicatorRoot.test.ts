import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Captured so a test can drive the screen-capture lifecycle the way Rust
// does. These are app-wide emits in production (`app.emit`), so this window
// really does receive every one of them.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

import RegionIndicatorRoot from "../src/roots/RegionIndicatorRoot.vue";

enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const k of Object.keys(listeners)) delete listeners[k];
});

async function mountRoot() {
  const w = mount(RegionIndicatorRoot);
  await flushPromises();
  return w;
}

const border = (w: ReturnType<typeof mount>) =>
  w.get('[data-testid="region-indicator-border"]').classes();

describe("RegionIndicatorRoot", () => {
  it("starts in the recording colour", async () => {
    const w = await mountRoot();
    expect(border(w)).toContain("border-recording");
    expect(border(w)).not.toContain("border-amber-400");
  });

  it("turns amber while paused and back on resume", async () => {
    const w = await mountRoot();
    listeners["screen:paused"]({ payload: { atMs: 1000 } });
    await flushPromises();
    expect(border(w)).toContain("border-amber-400");
    expect(border(w)).not.toContain("border-recording");

    listeners["screen:resumed"]({ payload: { pausedTotalMs: 500 } });
    await flushPromises();
    expect(border(w)).toContain("border-recording");
  });

  // The defect these two exist for. This window is declared in the config
  // and hidden-not-destroyed, so its webview mounts ONCE per process — the
  // property that forced `editor:open` to exist. With only the pause edges
  // wired, a capture paused and then ended leaves the border amber, and the
  // NEXT capture comes up amber while genuinely recording: a silent lie in
  // the one surface whose whole job is telling the truth about what is
  // being recorded. Spec amendment A5.
  it("resets to the recording colour when a paused capture stops", async () => {
    const w = await mountRoot();
    listeners["screen:paused"]({ payload: { atMs: 1000 } });
    await flushPromises();
    expect(border(w)).toContain("border-amber-400");

    listeners["screen:stopped"]({ payload: { base: "cap one" } });
    await flushPromises();
    expect(border(w)).toContain("border-recording");
    expect(border(w)).not.toContain("border-amber-400");
  });

  it("resets to the recording colour when a paused capture fails", async () => {
    const w = await mountRoot();
    listeners["screen:paused"]({ payload: { atMs: 1000 } });
    await flushPromises();
    listeners["screen:failed"]({ payload: { message: "the source is gone" } });
    await flushPromises();
    expect(border(w)).toContain("border-recording");
    expect(border(w)).not.toContain("border-amber-400");
  });

  it("stops listening when unmounted", async () => {
    const w = await mountRoot();
    expect(Object.keys(listeners).sort()).toEqual([
      "screen:failed",
      "screen:paused",
      "screen:resumed",
      "screen:stopped",
    ]);
    w.unmount();
    await flushPromises();
    expect(Object.keys(listeners)).toHaveLength(0);
  });
});
