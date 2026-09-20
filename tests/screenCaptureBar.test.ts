import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ScreenCaptureBar from "../src/components/ScreenCaptureBar.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import { useScreenCaptureStore } from "../src/stores/screenCapture";

/** The `screen:stopped` payload the store parks in `lastStaged` — the one
 * handle anything has on the footage until phase 5's staged-capture browser
 * lands. */
const STAGED = {
  base: "cap one",
  path: "C:/staging/cap one.mp4",
  durationMs: 30_000,
  sourceTitle: "Screen 1",
  width: 1920,
  height: 1080,
};

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
    // Rust's "Recorded {base} with a warning: {w}" stop toast; it is the
    // live window this line is the only surface for.)
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

  // THE ENTRY POINT. `open_capture_editor` had no caller at all until this
  // action existed, so the whole editor was unreachable from the running app.
  it("opens the editor on the capture that just finished", async () => {
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, ...(args as object) });
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.$patch({ status: "idle", lastStaged: STAGED });
    const w = mount(ScreenCaptureBar);
    await w.get('[data-testid="screen-edit"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "open_capture_editor")).toMatchObject({
      base: "cap one",
    });
  });

  // The Edit action is about a FINISHED capture. Offering it mid-recording
  // would invite a click that opens an editor on a file still being written.
  // `lastStaged` is deliberately POPULATED here: a gate that read only that
  // field — the obvious mutation — would pass against a null one.
  it("offers no Edit action while a capture is running", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    store.$patch({
      status: "capturing",
      sourceTitle: "S",
      startedAtMs: 0,
      lastStaged: STAGED,
    });
    const w = mount(ScreenCaptureBar);
    expect(w.find('[data-testid="screen-edit"]').exists()).toBe(false);
    // ...and the live controls are still the ones on screen.
    expect(w.find('[data-testid="screen-stop"]').exists()).toBe(true);
  });

  // The finished state is not the live one wearing a different label: a bar
  // still offering Pause/Stop on a capture that has already been staged
  // sends control messages to a session that no longer exists.
  it("retires the live controls once the capture is staged", async () => {
    mockIPC(() => undefined);
    useScreenCaptureStore().$patch({ status: "idle", lastStaged: STAGED });
    const w = mount(ScreenCaptureBar);
    expect(w.find('[data-testid="screen-stop"]').exists()).toBe(false);
    expect(w.find('[data-testid="screen-pause"]').exists()).toBe(false);
    expect(w.get('[data-testid="screen-ready"]').text()).toContain("cap one");
  });

  // A refused open (`is_safe_base` rejecting the name, or the editor window
  // being missing) must not be a click that does nothing: AGENTS.md's
  // diagnostics invariant says nothing may be caught and hidden.
  it("surfaces a refused open instead of swallowing it", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_capture_editor") throw new Error("That capture name is not one of ours.");
      return undefined;
    });
    useScreenCaptureStore().$patch({ status: "idle", lastStaged: STAGED });
    const w = mount(ScreenCaptureBar);
    await w.get('[data-testid="screen-edit"]').trigger("click");
    await flushPromises();
    expect(useNotificationsStore().items.map((i) => i.message).join(" ")).toContain(
      "not one of ours",
    );
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

  // M-4. `applyStopped` calls `reset()`, which nulls the store's
  // `sourceTitle` — but the staged payload carries the very title the live
  // row rendered a second earlier, so the finished row identified the
  // capture by its base name alone.
  it("keeps naming the source once the capture is staged", () => {
    // The store's own `sourceTitle` is set to a DIFFERENT value rather than
    // to null, although `reset()` really does null it: with both null and
    // "Screen 1" the two operands are interchangeable, and the assertion
    // would pass just as happily with the fallback written the other way
    // round. The staged capture's own copy has to win.
    useScreenCaptureStore().$patch({
      status: "idle",
      sourceTitle: "a previous capture",
      lastStaged: STAGED,
    });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-source"]').text()).toBe("Screen 1");
  });

  // M-3. The store toasts a `screen:warning` that arrives while idle
  // *because* this bar shows it inline while capturing — and since phase 4
  // the bar outlives the capture, so an ungated line rendered the same
  // warning twice: a toast AND a sticky line nothing clears until the next
  // capture starts. The live row is asserted in the same test so this cannot
  // be "fixed" by deleting the line outright.
  it("shows a warning inline only while a capture is live", () => {
    const store = useScreenCaptureStore();
    store.$patch({ status: "idle", lastStaged: STAGED, warning: "The audio device vanished." });
    const staged = mount(ScreenCaptureBar);
    expect(staged.find('[data-testid="screen-warning"]').exists()).toBe(false);

    store.$patch({ status: "capturing", lastStaged: null, startedAtMs: Date.now() });
    const live = mount(ScreenCaptureBar);
    expect(live.get('[data-testid="screen-warning"]').text()).toContain("vanished");
  });

  // M-2. The bar now stays mounted for the rest of the process once any
  // capture has finished, and the panel window is hidden rather than
  // unmounted — so an ungated `useNowTicker` ran a 1 Hz interval forever to
  // update a value the finished row renders nowhere. The live case is
  // asserted alongside it: a ticker that never runs would break the elapsed
  // reading instead.
  it("runs no clock for a finished capture, and one for a live capture", async () => {
    vi.useFakeTimers();
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    store.$patch({ status: "idle", lastStaged: STAGED });
    mount(ScreenCaptureBar);
    // A delta, not an absolute count: mounting anything under happy-dom can
    // register timers of its own, and the claim here is about this bar's
    // ticker alone.
    const idle = vi.getTimerCount();

    store.$patch({ status: "capturing", lastStaged: null, startedAtMs: Date.now() });
    await flushPromises();
    expect(vi.getTimerCount()).toBe(idle + 1);
  });
});
