import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h, ref } from "vue";

import { useNowTicker } from "../src/composables/useNowTicker";

/**
 * The shared 1 Hz clock behind every elapsed-time display. It had no test of
 * its own while three components relied on it; it grew one when a fourth
 * (`ScreenCaptureBar`) stopped satisfying the assumption its doc rested on —
 * "call sites only mount while work is running". Since phase 4 that bar stays
 * mounted for the rest of the process once a capture has finished, and the
 * panel window is hidden rather than unmounted, so an ungated interval ran
 * forever to update a value nothing rendered.
 */
function harness(enabled?: () => boolean) {
  const now = ref(0);
  const w = mount(
    defineComponent({
      setup() {
        const ticker = useNowTicker(enabled);
        return () => {
          now.value = ticker.value;
          return h("span", String(ticker.value));
        };
      },
    }),
  );
  return { w, now };
}

describe("useNowTicker", () => {
  afterEach(() => vi.useRealTimers());

  it("ticks once a second while mounted, and stops on unmount", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const { w, now } = harness();
    expect(now.value).toBe(1_000);

    // Fake timers advance the clock as well as the queue, so one tick lands
    // the ref exactly one second on.
    await vi.advanceTimersByTimeAsync(1_000);
    expect(now.value).toBe(2_000);

    // Behavioural rather than a timer count: mounting anything under
    // happy-dom can register timers of its own, so an absolute count would
    // pin the environment rather than this composable.
    w.unmount();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(now.value).toBe(2_000);
  });

  // The gate a call site that outlives its work needs. Asserted in BOTH
  // directions: a ticker that never starts would break the elapsed readings
  // this composable exists for, which an "it does not tick" assertion alone
  // would happily accept.
  it("runs no interval while the call site says there is nothing to tick for", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const live = ref(false);
    const { w, now } = harness(() => live.value);

    await vi.advanceTimersByTimeAsync(5_000);
    // Five seconds of wall clock passed and the ref did not move.
    expect(now.value).toBe(1_000);

    live.value = true;
    await w.vm.$nextTick();
    // Re-read on the way in: a display going live must not first render the
    // moment its component happened to mount, which is what a stale `now`
    // left over from the idle period would show.
    expect(now.value).toBe(6_000);

    // ...and it really is ticking now, not merely re-seeded.
    await vi.advanceTimersByTimeAsync(2_000);
    expect(now.value).toBe(8_000);

    live.value = false;
    await w.vm.$nextTick();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(now.value).toBe(8_000);
  });
});
