import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { useBuddyBubble, BUBBLE_MS } from "../src/composables/useBuddyBubble";

// A throwaway host: returning the composable from setup() exposes its
// refs/functions on wrapper.vm (refs are unwrapped there).
const Host = defineComponent({
  setup: () => useBuddyBubble(),
  render: () => null,
});

describe("useBuddyBubble", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    vi.useFakeTimers();
  });
  afterEach(() => vi.useRealTimers());

  it("shows the greeting on mount", () => {
    const wrapper = mount(Host);
    expect(wrapper.vm.visible).toBe(true);
    expect(wrapper.vm.text.length).toBeGreaterThan(0);
  });

  it("auto-dismisses the greeting after the greeting duration", () => {
    const wrapper = mount(Host);
    vi.advanceTimersByTime(BUBBLE_MS.normal.greeting);
    expect(wrapper.vm.visible).toBe(false);
  });

  it("show() replaces the text and restarts the timer (latest-wins)", () => {
    const wrapper = mount(Host);
    wrapper.vm.show("Opening Personal ✨", BUBBLE_MS.normal.ack);
    expect(wrapper.vm.text).toBe("Opening Personal ✨");
    expect(wrapper.vm.visible).toBe(true);

    // a second message just before the first would expire must replace it and
    // restart the clock — the first's remaining time never dismisses the second
    vi.advanceTimersByTime(BUBBLE_MS.normal.ack - 100);
    wrapper.vm.show("Transcript ready! ✨", BUBBLE_MS.normal.ack);
    expect(wrapper.vm.text).toBe("Transcript ready! ✨");
    vi.advanceTimersByTime(BUBBLE_MS.normal.ack - 100);
    expect(wrapper.vm.visible).toBe(true); // still up: timer was restarted
    vi.advanceTimersByTime(100);
    expect(wrapper.vm.visible).toBe(false);
  });

  it("dismiss() hides immediately and cancels the timer", () => {
    const wrapper = mount(Host);
    expect(vi.getTimerCount()).toBe(1); // the auto-dismiss timer is pending
    wrapper.vm.dismiss();
    expect(wrapper.vm.visible).toBe(false);
    expect(vi.getTimerCount()).toBe(0); // cleared, not merely fired
  });

  it("clears the timer on unmount", () => {
    const wrapper = mount(Host);
    expect(vi.getTimerCount()).toBe(1);
    wrapper.unmount();
    expect(vi.getTimerCount()).toBe(0);
    expect(() => vi.advanceTimersByTime(BUBBLE_MS.normal.greeting)).not.toThrow();
  });

  it("normal preserves today's exact timings", () => {
    // the setting must be a no-op until the user touches it
    expect(BUBBLE_MS.normal).toEqual({ ack: 3200, greeting: 5000 });
  });

  it("greeting uses the configured duration tier", () => {
    localStorage.setItem("vault-buddy.messageDuration", "long");
    const wrapper = mount(Host);
    vi.advanceTimersByTime(BUBBLE_MS.normal.greeting);
    expect(wrapper.vm.visible).toBe(true); // long outlives the normal timing
    vi.advanceTimersByTime(BUBBLE_MS.long.greeting - BUBBLE_MS.normal.greeting);
    expect(wrapper.vm.visible).toBe(false);
  });
});
