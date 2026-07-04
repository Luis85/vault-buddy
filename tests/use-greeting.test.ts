import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import { useGreeting, GREETING_MS } from "../src/composables/useGreeting";

// A throwaway host component: returning the composable from setup() exposes
// its refs/functions on wrapper.vm (refs are unwrapped there).
const Host = defineComponent({
  setup: () => useGreeting(),
  render: () => null,
});

describe("useGreeting", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows a greeting on mount", () => {
    const wrapper = mount(Host);
    expect(wrapper.vm.bubbleVisible).toBe(true);
    expect(wrapper.vm.bubbleText.length).toBeGreaterThan(0);
  });

  it("auto-dismisses after GREETING_MS", () => {
    const wrapper = mount(Host);
    vi.advanceTimersByTime(GREETING_MS);
    expect(wrapper.vm.bubbleVisible).toBe(false);
  });

  it("dismiss() hides immediately and cancels the timer", () => {
    const wrapper = mount(Host);
    wrapper.vm.dismiss();
    expect(wrapper.vm.bubbleVisible).toBe(false);
    // advancing past the original timeout must not throw or re-toggle
    vi.advanceTimersByTime(GREETING_MS);
    expect(wrapper.vm.bubbleVisible).toBe(false);
  });

  it("clears the timer on unmount", () => {
    const wrapper = mount(Host);
    wrapper.unmount();
    // no dangling callback flips a ref on a torn-down component
    expect(() => vi.advanceTimersByTime(GREETING_MS)).not.toThrow();
  });
});
