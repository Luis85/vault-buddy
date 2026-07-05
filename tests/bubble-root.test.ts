import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import BubbleRoot from "../src/roots/BubbleRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

// Capture the bubble-anchor handler so tests can drive it like Rust would.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

describe("BubbleRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    for (const key of Object.keys(listeners)) delete listeners[key];
    mockIPC(() => {});
  });
  afterEach(() => clearMocks());

  it("renders the greeting text", async () => {
    const wrapper = mount(BubbleRoot);
    await flushPromises();
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);
  });

  it("defaults the tail to the right side before any anchor event", () => {
    const wrapper = mount(BubbleRoot);
    // Rust derives the side from the buddy's position and pushes it via
    // bubble-anchor; before that lands the bubble defaults to the right.
    expect(wrapper.get('[data-testid="speech-bubble"]').classes()).toContain(
      "side-right",
    );
  });

  it("pulls the anchor on mount so the tail is right before any event", async () => {
    // The bubble webview mounts after Rust's startup emits, so it must PULL the
    // anchor, not only wait for the event (the "bubble too high until I drag"
    // race).
    mockIPC((cmd) => {
      if (cmd === "get_bubble_anchor") return { side: "left", valign: "bottom" };
    });
    const wrapper = mount(BubbleRoot);
    await flushPromises();
    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).toContain("side-left");
    expect(bubble.classes()).toContain("valign-bottom");
  });

  it("points the tail per the bubble-anchor event from Rust", async () => {
    const wrapper = mount(BubbleRoot);
    await flushPromises();
    expect(wrapper.get('[data-testid="speech-bubble"]').classes()).toContain(
      "side-right",
    );

    // Rust placed the bubble to the LEFT (e.g. buddy at the right edge) with
    // the tail low (buddy near the bottom edge) — the tail must follow.
    listeners["bubble-anchor"]?.({ payload: { side: "left", valign: "bottom" } });
    await flushPromises();

    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).toContain("side-left");
    expect(bubble.classes()).toContain("valign-bottom");
  });
});
