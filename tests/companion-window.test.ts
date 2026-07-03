import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

const calls = vi.hoisted(() => [] as string[]);
const pending = vi.hoisted(
  () => ({ resolveOuterPosition: null }) as {
    resolveOuterPosition: ((pos: { x: number; y: number }) => void) | null;
  },
);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    outerPosition: () =>
      new Promise<{ x: number; y: number }>((resolve) => {
        pending.resolveOuterPosition = resolve;
      }),
    scaleFactor: () => Promise.resolve(1),
    setPosition: () => {
      calls.push("setPosition");
      return Promise.resolve();
    },
    setSize: (size: { width: number }) => {
      calls.push(`setSize:${size.width}`);
      return Promise.resolve();
    },
  }),
  currentMonitor: () => Promise.resolve(null),
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

describe("useCompanionWindow", () => {
  beforeEach(() => {
    calls.length = 0;
    pending.resolveOuterPosition = null;
  });

  it("expands on open and collapses on close", async () => {
    const open = ref(false);
    useCompanionWindow(open);

    open.value = true;
    await nextTick();
    pending.resolveOuterPosition?.({ x: 100, y: 100 });
    await flush();
    expect(calls).toContain(`setSize:${EXPANDED.width}`);

    open.value = false;
    await nextTick();
    await flush();
    expect(calls[calls.length - 1]).toBe(`setSize:${COLLAPSED.width}`);
  });

  it("never leaves the window expanded when close arrives mid-open", async () => {
    const open = ref(false);
    useCompanionWindow(open);

    open.value = true;
    await nextTick(); // open transition is now awaiting outerPosition
    open.value = false;
    await nextTick(); // close requested while open is still in flight

    pending.resolveOuterPosition?.({ x: 100, y: 100 });
    await flush();
    await flush();

    // the stale open must not expand after the close collapsed the window
    const last = calls.filter((c) => c.startsWith("setSize")).pop();
    expect(last).toBe(`setSize:${COLLAPSED.width}`);
  });
});
