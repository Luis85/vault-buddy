import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

interface Point {
  x: number;
  y: number;
}

const state = vi.hoisted(() => ({
  calls: [] as string[],
  pos: { x: 0, y: 0 } as Point,
  monitor: null as null | {
    position: Point;
    size: { width: number; height: number };
  },
  deferOuter: false,
  pendingOuter: [] as Array<() => void>,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    outerPosition: () =>
      state.deferOuter
        ? new Promise<Point>((resolve) => {
            state.pendingOuter.push(() => resolve({ ...state.pos }));
          })
        : Promise.resolve({ ...state.pos }),
    scaleFactor: () => Promise.resolve(1),
    setPosition: (p: Point) => {
      state.pos = { x: p.x, y: p.y };
      state.calls.push(`setPosition:${p.x},${p.y}`);
      return Promise.resolve();
    },
    setSize: (size: { width: number }) => {
      state.calls.push(`setSize:${size.width}`);
      return Promise.resolve();
    },
  }),
  currentMonitor: () => Promise.resolve(state.monitor),
  LogicalSize: class {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
  PhysicalPosition: class {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
}));

import {
  useCompanionWindow,
  COLLAPSED,
  EXPANDED,
} from "../src/composables/useCompanionWindow";

const flush = () => new Promise((r) => setTimeout(r));
const resolveOuter = () => {
  for (const resolve of state.pendingOuter.splice(0)) resolve();
};
// window size changes arrive as setGeometry (normal path) or setSize
// (fallback when window info is unavailable)
const lastResize = () => {
  const entry = state.calls
    .filter((c) => c.startsWith("setGeometry") || c.startsWith("setSize"))
    .pop();
  if (!entry) return undefined;
  if (entry.startsWith("setSize")) return entry.split(":")[1];
  return entry.split(",")[2];
};

describe("useCompanionWindow", () => {
  beforeEach(() => {
    state.calls.length = 0;
    state.pos = { x: 100, y: 100 };
    state.monitor = null;
    state.deferOuter = false;
    state.pendingOuter.length = 0;
    mockIPC((cmd, args) => {
      if (cmd === "set_panel_offset") {
        const { x, y } = args as { x: number; y: number };
        state.calls.push(`reportOffset:${x},${y}`);
      }
      if (cmd === "set_window_geometry") {
        const { x, y, width } = args as {
          x: number;
          y: number;
          width: number;
        };
        state.pos = { x, y };
        state.calls.push(`setGeometry:${x},${y},${width}`);
      }
    });
  });

  afterEach(() => {
    clearMocks();
  });

  it("expands on open and collapses on close", async () => {
    const open = ref(false);
    useCompanionWindow(open);

    open.value = true;
    await nextTick();
    await flush();
    expect(lastResize()).toBe(String(EXPANDED.width));

    open.value = false;
    await nextTick();
    await flush();
    expect(lastResize()).toBe(String(COLLAPSED.width));
  });

  it("never leaves the window expanded when close arrives mid-open", async () => {
    const open = ref(false);
    useCompanionWindow(open);

    state.deferOuter = true;
    open.value = true;
    await nextTick(); // open transition is now awaiting outerPosition
    open.value = false;
    await nextTick(); // close requested while open is still in flight

    state.deferOuter = false;
    resolveOuter();
    await flush();
    await flush();

    // the stale open must not expand after the close collapsed the window
    expect(lastResize()).toBe(String(COLLAPSED.width));
  });

  it("restores the original position after open→close→open at a screen edge", async () => {
    // buddy hugging the right edge of a 1920x1080 monitor: opening shifts
    // the window left so the panel can unfold on-screen
    state.monitor = {
      position: { x: 0, y: 0 },
      size: { width: 1920, height: 1080 },
    };
    state.pos = { x: 1780, y: 100 };

    const open = ref(false);
    useCompanionWindow(open);

    state.deferOuter = true;
    open.value = true;
    await nextTick(); // first open awaiting outerPosition
    open.value = false;
    await nextTick(); // close queued
    open.value = true;
    await nextTick(); // reopen queued

    state.deferOuter = false;
    resolveOuter();
    await flush();
    await flush();
    await flush();

    // the reopen must not re-plan from the shifted position and forget the
    // pending offset — the window stays shifted exactly once
    const shift = EXPANDED.width - COLLAPSED.width;
    expect(state.pos).toEqual({ x: 1780 - shift, y: 100 });

    // the Rust side must know about the shift so a tray quit saves the
    // unshifted home position
    expect(state.calls).toContain(`reportOffset:${shift},0`);

    open.value = false;
    await nextTick();
    await flush();

    // closing puts the buddy back exactly where the user left it
    expect(state.pos).toEqual({ x: 1780, y: 100 });
    expect(lastResize()).toBe(String(COLLAPSED.width));
    expect(state.calls[state.calls.length - 1]).toBe("reportOffset:0,0");
  });
});
