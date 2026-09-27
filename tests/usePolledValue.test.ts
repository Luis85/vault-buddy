import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";

import { usePolledValue } from "../src/composables/usePolledValue";

/**
 * The shared interval-poll behind every meter that reads a plain function
 * rather than a reactive source (`MixerPeakMeter.vue`, `TransportBar.vue`'s
 * own peak bar): sampled on a fixed interval, not at frame rate, and only
 * while the owning component is mounted.
 */
function harness(sample: () => number, intervalMs = 100) {
  return mount(
    defineComponent({
      setup() {
        const value = usePolledValue(sample, intervalMs);
        return () => h("span", String(value.value));
      },
    }),
  );
}

describe("usePolledValue", () => {
  afterEach(() => vi.useRealTimers());

  it("samples once on mount and again on every interval, stopping on unmount", async () => {
    vi.useFakeTimers();
    let current = 1;
    const w = harness(() => current);
    expect(w.text()).toBe("1");

    current = 2;
    await vi.advanceTimersByTimeAsync(100);
    expect(w.text()).toBe("2");

    current = 3;
    await vi.advanceTimersByTimeAsync(100);
    expect(w.text()).toBe("3");

    w.unmount();
    current = 4;
    await vi.advanceTimersByTimeAsync(500);
    expect(w.text()).toBe("3");
  });

  it("re-reads on mount rather than trusting a value computed before it", () => {
    vi.useFakeTimers();
    let current = 1;
    const sample = vi.fn(() => current);
    current = 5;
    const w = harness(sample);
    expect(w.text()).toBe("5");
    // Called once for the initial ref seed and again in onMounted -- both
    // read the CURRENT value, never a stale one from construction time.
    expect(sample).toHaveBeenCalled();
  });
});
