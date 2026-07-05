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

  it("defaults the tail to the buddy's facing side before any anchor event", () => {
    localStorage.setItem("vault-buddy.facing", "left");
    const wrapper = mount(BubbleRoot);
    // facing left → the bubble opens to the left, tail on its right face
    expect(wrapper.get('[data-testid="speech-bubble"]').classes()).toContain(
      "side-left",
    );
  });

  it("points the tail per the bubble-anchor event from Rust", async () => {
    const wrapper = mount(BubbleRoot); // default facing is right
    await flushPromises();
    expect(wrapper.get('[data-testid="speech-bubble"]').classes()).toContain(
      "side-right",
    );

    // Rust placed the bubble to the LEFT (e.g. buddy at the right edge) and
    // bottom-aligned it — the tail must follow.
    listeners["bubble-anchor"]?.({ payload: { side: "left", valign: "up" } });
    await flushPromises();

    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).toContain("side-left");
    expect(bubble.classes()).toContain("valign-up");
  });
});
