import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ScreenCaptureBar from "../src/components/ScreenCaptureBar.vue";
import { useScreenCaptureStore } from "../src/stores/screenCapture";

describe("ScreenCaptureBar", () => {
  beforeEach(() => setActivePinia(createPinia()));
  // Both undone unconditionally: a failing assertion must not leak fake
  // timers or a mocked IPC transport into the next test, which is how a
  // suite starts reporting failures in files that are not broken.
  afterEach(() => {
    vi.useRealTimers();
    clearMocks();
  });

  it("names the source being captured", () => {
    const store = useScreenCaptureStore();
    store.$patch({
      status: "capturing",
      sourceTitle: "Figma — Google Chrome",
      startedAtMs: Date.now(),
    });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-source"]').text()).toBe(
      "Figma — Google Chrome",
    );
  });

  it("excludes paused time from the elapsed reading", () => {
    // A two-minute pause must not show as two minutes of recording that is
    // not in the file. The clock excludes paused time by construction on the
    // Rust side; the bar must not reintroduce it.
    //
    // Hand-computed from the store's own arithmetic, not from a run:
    //   now 90_000 - startedAt 0 - pausedTotal 0 - open pause (90_000 -
    //   30_000 = 60_000) = 30_000 ms = "0:30".
    // Dropping the open-pause term (the bug) reads 90_000 ms = "1:30", so
    // the exact-match assertion below is what separates them.
    vi.useFakeTimers();
    vi.setSystemTime(90_000);
    const store = useScreenCaptureStore();
    store.$patch({
      status: "paused",
      sourceTitle: "Screen 1",
      startedAtMs: 0,
      pausedTotalMs: 0,
      pausedSinceMs: 30_000,
    });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-elapsed"]').text()).toBe("Paused 0:30");
  });

  it("offers Resume while paused and Pause while capturing", async () => {
    // The label alone is not the contract: a bar reading "Resume" that sends
    // `pause_screen_capture` is exactly as broken as one with the wrong
    // label, so each state asserts the command it actually issues.
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-pause"]').text()).toContain("Pause");
    await w.get('[data-testid="screen-pause"]').trigger("click");
    expect(calls).toEqual(["pause_screen_capture"]);

    store.$patch({ status: "paused", pausedSinceMs: 1 });
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-pause"]').text()).toContain("Resume");
    await w.get('[data-testid="screen-pause"]').trigger("click");
    expect(calls).toEqual(["pause_screen_capture", "resume_screen_capture"]);
  });

  it("shows the dropped-frame count only when frames have actually dropped", async () => {
    // Advisory and lossy by design (spec 11). A permanent "0 dropped" badge
    // is noise; a badge appearing at the moment frames start dropping is the
    // signal spec 17.3 wants visible.
    //
    // The clock is pinned so the elapsed reading cannot itself contain the
    // digits under assertion — with a real `Date.now()` against
    // startedAtMs 0 the bar renders a six-figure hour count, and whether it
    // happens to contain "12" is a property of the wall clock.
    vi.useFakeTimers();
    vi.setSystemTime(65_000);
    const store = useScreenCaptureStore();
    store.$patch({
      status: "capturing",
      sourceTitle: "S",
      startedAtMs: 0,
      dropped: 0,
    });
    const w = mount(ScreenCaptureBar);
    expect(w.find('[data-testid="screen-dropped"]').exists()).toBe(false);
    store.$patch({ dropped: 12 });
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-dropped"]').text()).toBe("12 dropped");
  });

  it("shows a live warning inline instead of tearing the bar down", async () => {
    // Spec 14: a vanished source or device warns and the capture finalizes
    // cleanly. The store deliberately withholds the toast while a capture is
    // live (its own comment says the bar shows it inline) — so if the bar
    // does not render it, the warning is invisible for the whole live
    // capture. (A TERMINAL warning still reaches the user either way, via
    // Rust's "Saved with a warning: {w}" stop toast; it is the live window
    // this line is the only surface for.)
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.find('[data-testid="screen-warning"]').exists()).toBe(false);
    store.$patch({ warning: "an audio device disappeared" });
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-warning"]').text()).toBe(
      "an audio device disappeared",
    );
    // ...and the bar is still the live capture bar, not a torn-down husk.
    expect(w.find('[data-testid="screen-elapsed"]').exists()).toBe(true);
    expect(w.find('[data-testid="screen-stop"]').exists()).toBe(true);
  });

  it("keeps the elapsed reading live while the capture runs", async () => {
    // The reading must be LIVE. A bar wired to a clock sampled once at setup
    // renders a correct first frame and then freezes, and no assertion on a
    // single static value can tell the two apart — so the tick itself is
    // what this pins (tests/import-progress.test.ts:45 is the precedent).
    //
    // Hand-computed from the store's arithmetic, not from a run:
    //   now 0 - startedAt 0 - pausedTotal 0 = 0 ms -> "Recording 0:00";
    //   five ticks later the clock reads 5_000 -> 5_000 ms -> "Recording 0:05".
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-elapsed"]').text()).toBe("Recording 0:00");
    await vi.advanceTimersByTimeAsync(5_000);
    expect(w.get('[data-testid="screen-elapsed"]').text()).toBe("Recording 0:05");
  });

  it("reads Saving… instead of counting on while the stop finalizes", async () => {
    // `status` stays `capturing` until `screen:stopped` lands and the ticker
    // keeps ticking, so a label with only Recording/Paused arms goes on
    // COUNTING for the whole finalize window — bounded at 30 s by
    // `STOP_TIMEOUT` (screen_commands.rs), which answers `stillSaving` on
    // expiry. Those seconds are not in the file: the same falsehood the
    // paused-time arithmetic exists to prevent. The sibling audio bar reads
    // "Saving…" here (RecordingBar.vue's `saving` arm).
    //
    // Hand-computed: startedAt 0, clock 60_000 -> 60_000 ms -> "Recording
    // 1:00" before the stop; 25 s of finalize later a counting bar reads
    // "Recording 1:25" while the file still ends at 1:00.
    vi.useFakeTimers();
    vi.setSystemTime(60_000);
    mockIPC((cmd) =>
      cmd === "stop_screen_capture" ? new Promise(() => {}) : undefined,
    );
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-elapsed"]').text()).toBe("Recording 1:00");
    await w.get('[data-testid="screen-stop"]').trigger("click");
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-elapsed"]').text()).toBe("Saving…");
    await vi.advanceTimersByTimeAsync(25_000);
    expect(w.get('[data-testid="screen-elapsed"]').text()).toBe("Saving…");
  });

  it("wears the same status dot as the audio bar it sits beside", async () => {
    // Two live-capture bars render side by side on the list view, so a
    // visibly different indicator on one of them is drift, not a detail:
    // RecordingBar's dot is h-2.5 w-2.5 and turns amber while paused, where
    // the StatusDot primitive is h-1.5 w-1.5 with no amber tone — not the
    // clean drop-in it looks like at this one call site.
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-dot"]').classes()).toEqual(
      expect.arrayContaining(["h-2.5", "w-2.5", "animate-pulse", "bg-recording"]),
    );
    store.$patch({ status: "paused", pausedSinceMs: 1 });
    await w.vm.$nextTick();
    const paused = w.get('[data-testid="screen-dot"]').classes();
    expect(paused).toEqual(expect.arrayContaining(["h-2.5", "w-2.5", "bg-amber-400"]));
    expect(paused).not.toContain("animate-pulse");
  });

  it("disables Stop while a stop is already in flight", async () => {
    // Finalize is unbounded; a second Stop during it would send a control
    // message to a session that is already tearing down.
    let stops = 0;
    mockIPC((cmd) => {
      if (cmd === "stop_screen_capture") {
        stops += 1;
        return new Promise(() => {}); // never resolves: the stop is in flight
      }
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    await w.get('[data-testid="screen-stop"]').trigger("click");
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-stop"]').attributes("disabled")).toBeDefined();
    // Pause is disabled too: pausing a session that is finalizing is the
    // same stale control message by another name.
    expect(w.get('[data-testid="screen-pause"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="screen-stop"]').trigger("click");
    expect(stops).toBe(1);
  });
});
