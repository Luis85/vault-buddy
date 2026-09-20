import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Same listener-capture harness capture-store.test.ts uses: the store's
// init() registers real `listen` calls, and the tests drive the events by
// hand rather than through a Tauri runtime.
const state = vi.hoisted(() => ({
  eventHandlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    state.eventHandlers[name] = handler;
    return Promise.resolve(() => {
      delete state.eventHandlers[name];
    });
  },
}));

vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { logWarning } from "../src/logging";
import { useNotificationsStore } from "../src/stores/notifications";
import { useScreenCaptureStore } from "../src/stores/screenCapture";

/** `ScreenStatusPayload::idle()` — every field cleared, as Rust emits it. */
const IDLE = {
  capturing: false,
  vaultId: null,
  startedAtMs: null,
  paused: false,
  pausedTotalMs: 0,
  pausedSinceMs: null,
  sourceTitle: null,
};

const RUNNING = {
  capturing: true,
  vaultId: "v1",
  startedAtMs: 1000,
  paused: false,
  pausedTotalMs: 0,
  pausedSinceMs: null,
  sourceTitle: "Screen 1",
};

function emit(name: string, payload: unknown) {
  state.eventHandlers[name]?.({ payload });
}

describe("screenCapture store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    state.eventHandlers = {};
  });

  afterEach(() => clearMocks());

  it("starts idle and reports nothing", () => {
    const store = useScreenCaptureStore();
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.sourceTitle).toBeNull();
  });

  it("holds the capture's identity after a successful start", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "start_screen_capture") return RUNNING;
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.start("v1", "screen:1", ["Mic"], ["Speakers"]);
    expect(store.status).toBe("capturing");
    expect(store.vaultId).toBe("v1");
    expect(store.sourceTitle).toBe("Screen 1");
    expect(store.startedAtMs).toBe(1000);
    // The argument names are the Rust command's parameters as Tauri
    // camelCases them (`source_id` -> `sourceId`). A drifted name arrives as
    // `undefined` on the Rust side and the start fails with a confusing
    // "Unknown capture source", so pin the exact shape.
    expect(calls).toEqual([
      {
        cmd: "start_screen_capture",
        args: { id: "v1", sourceId: "screen:1", inputs: ["Mic"], outputs: ["Speakers"] },
      },
    ]);
  });

  it("reports nothing running after a refused start when nothing is running", async () => {
    // Spec 14's alreadyCapturing, and every other typed refusal: a failed
    // start must not leave the store believing a capture is running, or the
    // capture bar renders over nothing and Stop has no session to stop. The
    // store starts out holding a stale capture here precisely so the
    // assertion cannot pass by never having changed.
    mockIPC((cmd) => {
      if (cmd === "start_screen_capture") {
        throw new Error("A recording is already in progress.");
      }
      if (cmd === "screen_capture_status") return IDLE;
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", vaultId: "ghost", sourceTitle: "Ghost", startedAtMs: 7 });
    await expect(store.start("v1", "screen:1", [], [])).rejects.toThrow();
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.sourceTitle).toBeNull();
    expect(store.startedAtMs).toBeNull();
    expect(store.error).toContain("already in progress");
  });

  it("does not blank a live capture when a second start is refused", async () => {
    // The refusal this path sees most IS alreadyCapturing, so the capture
    // being reported about is usually the one already on screen. Clearing the
    // store on any failed start would erase that capture's own bar — losing
    // Stop for a capture that is still writing to disk.
    mockIPC((cmd) => {
      if (cmd === "start_screen_capture") {
        throw new Error("A recording is already in progress.");
      }
      if (cmd === "screen_capture_status") return RUNNING;
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.applyStatus(RUNNING);
    await expect(store.start("v1", "screen:1", [], [])).rejects.toThrow();
    expect(store.status).toBe("capturing");
    expect(store.vaultId).toBe("v1");
    expect(store.sourceTitle).toBe("Screen 1");
  });

  it("clears its state on screen:stopped and keeps the staged capture", async () => {
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", vaultId: "v1", sourceTitle: "Screen 1", startedAtMs: 5 });
    store.applyStopped({
      base: "2026-09-19 1100 Screen 1",
      path: "C:/staging/2026-09-19 1100 Screen 1.mp4",
      durationMs: 5000,
      sourceTitle: "Screen 1",
      width: 1920,
      height: 1080,
    });
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.startedAtMs).toBeNull();
    expect(store.lastStaged?.durationMs).toBe(5000);
  });

  it("keeps paused time out of the elapsed reading", () => {
    // The capture bar counts from startedAtMs; without subtracting paused
    // time a two-minute pause shows as two minutes of recording that is not
    // in the file. 90s wall - 30s already banked - 30s of the current pause
    // (90_000 - 60_000) = 30s of real footage.
    const store = useScreenCaptureStore();
    store.$patch({
      status: "paused",
      startedAtMs: 0,
      pausedTotalMs: 30_000,
      pausedSinceMs: 60_000,
    });
    expect(store.elapsedMs(90_000)).toBe(30_000);
  });

  it("subtracts only the banked pause time while capturing", () => {
    // The open-pause term must be gated on being paused. Applying it while
    // capturing would subtract a stale pausedSinceMs forever and freeze the
    // clock after the first resume.
    const store = useScreenCaptureStore();
    store.$patch({
      status: "capturing",
      startedAtMs: 0,
      pausedTotalMs: 10_000,
      pausedSinceMs: 20_000,
    });
    expect(store.elapsedMs(50_000)).toBe(40_000);
  });

  it("reads zero elapsed while no capture is running", () => {
    const store = useScreenCaptureStore();
    expect(store.elapsedMs(90_000)).toBe(0);
  });

});

describe("screenCapture store events", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    state.eventHandlers = {};
  });

  afterEach(() => clearMocks());

  it("resyncs from screen_capture_status on init, so a reloaded webview is not blank", async () => {
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") return RUNNING;
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.init();
    await flushPromises();
    expect(store.status).toBe("capturing");
    expect(store.vaultId).toBe("v1");
    expect(store.sourceTitle).toBe("Screen 1");
  });

  it("maps a paused status payload onto the paused state", async () => {
    // `capturing` is a BOOLEAN on the wire and `status` is a three-valued
    // string here; the paused arm is the one the mapping can silently drop.
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") {
        return { ...RUNNING, paused: true, pausedTotalMs: 400, pausedSinceMs: 9000 };
      }
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.init();
    await flushPromises();
    expect(store.status).toBe("paused");
    expect(store.paused).toBe(true);
    expect(store.pausedTotalMs).toBe(400);
    expect(store.pausedSinceMs).toBe(9000);
  });

  it("stays idle when screen:started arrives AFTER screen:stopped", async () => {
    // Documented race (screen_commands.rs's emit site): the monitor thread is
    // live before start_screen_capture's tail emits, so a source that closes
    // in those few ms emits `screen:stopped` first. Deriving state from
    // arrival order would leave a phantom capture bar counting up from a
    // capture that already finished, with no session behind Stop. The Rust
    // comment names `screen_capture_status` as the authority, so the started
    // handler must consult it rather than trust its own payload.
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") return IDLE;
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.init();
    await flushPromises();
    emit("screen:stopped", {
      base: "b",
      path: "C:/staging/b.mp4",
      durationMs: 1,
      sourceTitle: "Screen 1",
      width: 8,
      height: 8,
    });
    emit("screen:started", RUNNING);
    await flushPromises();
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
  });

  it("goes capturing on a screen:started for a capture that is really running", async () => {
    // The other half of the authority rule: when the status really is
    // running, a non-initiating window (the buddy) learns it from this event.
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") return RUNNING;
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.init();
    store.reset();
    emit("screen:started", RUNNING);
    await flushPromises();
    expect(store.status).toBe("capturing");
    expect(store.sourceTitle).toBe("Screen 1");
  });

  it("discards a status read that a terminal event overtook", async () => {
    // The authority read is not instantaneous. A `screen:stopped` landing
    // while one is in flight describes a LATER moment than the answer does,
    // so applying that answer would resurrect the capture the store was just
    // told had finished — the same phantom bar the out-of-order rule exists
    // to prevent, arriving by the other route.
    mockIPC((cmd) => (cmd === "screen_capture_status" ? IDLE : undefined));
    const store = useScreenCaptureStore();
    await store.init();
    let answer!: (v: unknown) => void;
    mockIPC((cmd) =>
      cmd === "screen_capture_status"
        ? new Promise((resolve) => {
            answer = resolve;
          })
        : undefined,
    );
    const inFlight = store.resync();
    emit("screen:stopped", {
      base: "b",
      path: "C:/staging/b.mp4",
      durationMs: 1,
      sourceTitle: "Screen 1",
      width: 8,
      height: 8,
    });
    answer(RUNNING);
    await inFlight;
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
  });

  it("tracks pause and resume events", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.$patch({ status: "capturing", vaultId: "v1", startedAtMs: 0 });
    emit("screen:paused", { atMs: 4000 });
    expect(store.status).toBe("paused");
    expect(store.pausedSinceMs).toBe(4000);
    emit("screen:resumed", { pausedTotalMs: 1500 });
    expect(store.status).toBe("capturing");
    expect(store.pausedTotalMs).toBe(1500);
    expect(store.pausedSinceMs).toBeNull();
  });

  it("drives pause, resume and stop through their own commands", async () => {
    const cmds: string[] = [];
    mockIPC((cmd) => {
      cmds.push(cmd);
      if (cmd === "stop_screen_capture") return { stillSaving: false };
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.pause();
    await store.resume();
    await store.stop();
    expect(cmds).toEqual([
      "pause_screen_capture",
      "resume_screen_capture",
      "stop_screen_capture",
    ]);
  });

  it("marks a stop in flight and only clears it when the capture really ends", async () => {
    // Finalize is unbounded and `stop_screen_capture` can return
    // `stillSaving` while the session is still tearing down, so the flag
    // cannot clear on the command's own reply — it has to survive until
    // `screen:stopped` reports the capture gone. The bar's Stop button reads
    // this, and a flag that cleared early would re-arm it against a session
    // that is already finalizing.
    let settle: ((v: { stillSaving: boolean }) => void) | null = null;
    mockIPC((cmd) => {
      if (cmd === "stop_screen_capture") {
        return new Promise<{ stillSaving: boolean }>((r) => (settle = r));
      }
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.init();
    store.applyStatus(RUNNING);
    expect(store.stopping).toBe(false);
    const inFlight = store.stop();
    expect(store.stopping).toBe(true);
    settle!({ stillSaving: true });
    await inFlight;
    // The command has answered; the capture has not ended yet.
    expect(store.stopping).toBe(true);
    emit("screen:stopped", {
      base: "b",
      path: "C:/staging/b.mp4",
      durationMs: 1,
      sourceTitle: "Screen 1",
      width: 8,
      height: 8,
    });
    expect(store.stopping).toBe(false);
  });

  it("re-arms Stop when the stop itself was refused", async () => {
    // A refusal means this window's picture was wrong, not that a finalize
    // is running. Leaving the flag set would strand the bar with a dead Stop
    // button for as long as the capture kept running.
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") return RUNNING;
      throw new Error("No screen capture is running.");
    });
    const store = useScreenCaptureStore();
    store.applyStatus(RUNNING);
    await store.stop();
    expect(store.status).toBe("capturing");
    expect(store.stopping).toBe(false);
  });

  it("reconciles with Rust when a pause/resume/stop is refused", async () => {
    // These commands re-check their preconditions under the state mutex, so a
    // refusal means this window's picture is already wrong (a capture that
    // ended, or one that is still starting). Swallowing it would leave the
    // bar offering a control that does nothing.
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") return IDLE;
      throw new Error("No screen capture is running.");
    });
    const store = useScreenCaptureStore();
    store.applyStatus(RUNNING);
    await store.pause();
    expect(store.status).toBe("idle");
    store.applyStatus(RUNNING);
    await store.stop();
    expect(store.status).toBe("idle");
    store.applyStatus({ ...RUNNING, paused: true, pausedSinceMs: 1 });
    await store.resume();
    expect(store.status).toBe("idle");
  });

  it("keeps the retained path a failed finalize hands back", async () => {
    // screen:failed carries `retainedPath` when a stop failed AFTER real
    // footage was written (ScreenError::Retained). Dropping it loses the only
    // typed handle on a file that is still playable.
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.$patch({ status: "capturing", vaultId: "v1", startedAtMs: 0 });
    emit("screen:failed", {
      message: "the capture was stopped but the footage was kept",
      retainedPath: "C:/staging/.2026-09-19 1100 Demo.mp4.part",
    });
    expect(store.status).toBe("idle");
    expect(store.error).toContain("footage was kept");
    expect(store.retainedPath).toBe("C:/staging/.2026-09-19 1100 Demo.mp4.part");
  });

  it("clears a previous retained path when the next capture starts", async () => {
    // A stale retained path would offer the user a file from a capture two
    // sessions ago as if it belonged to the one they just ran.
    mockIPC((cmd) => {
      if (cmd === "start_screen_capture") return RUNNING;
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.init();
    store.$patch({ error: "old", retainedPath: "C:/staging/old.mp4.part" });
    await store.start("v1", "screen:1", [], []);
    expect(store.retainedPath).toBeNull();
    expect(store.error).toBeNull();
  });

  it("toasts a warning that arrives with no capture running", async () => {
    // While capturing, the capture bar shows the warning inline and a toast
    // would double up. Outside a capture there is no bar, so the toast is the
    // only thing standing between the user and a silently swallowed warning.
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    emit("screen:warning", { message: "a display was unplugged" });
    expect(useNotificationsStore().items.map((n) => n.message)).toContain(
      "a display was unplugged",
    );
  });

  it("logs a failed status read instead of throwing out of the handler", async () => {
    // Diagnostics invariant: nothing caught is hidden. Rethrowing would take
    // down the event handler (or onMounted) this runs inside.
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") throw new Error("status exploded");
      return undefined;
    });
    const store = useScreenCaptureStore();
    await expect(store.init()).resolves.toBeUndefined();
    expect(vi.mocked(logWarning).mock.calls.flat().join(" ")).toContain("status exploded");
  });

  it("does not resurrect a capture that stopped while start's reply was in flight", async () => {
    // The third route into the documented started-after-stopped race
    // (screen_commands.rs names it): the monitor thread is live BEFORE
    // start_screen_capture's tail returns, so a source closing in that window
    // emits `screen:stopped` first and the command's own `capturing: true`
    // reply arrives stale. Applying it would raise a bar over a finished
    // capture AND — worse — the `lastStaged = null` beside it would discard
    // the staged .mp4 that `screen:stopped` had just delivered, which is the
    // only handle anything has on the footage.
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    let answer!: (v: unknown) => void;
    mockIPC((cmd) =>
      cmd === "start_screen_capture"
        ? new Promise((resolve) => {
            answer = resolve;
          })
        : undefined,
    );
    const inFlight = store.start("v1", "screen:1", [], []);
    emit("screen:stopped", {
      base: "b",
      path: "C:/staging/b.mp4",
      durationMs: 1,
      sourceTitle: "Screen 1",
      width: 8,
      height: 8,
    });
    answer(RUNNING);
    await inFlight;
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.lastStaged?.path).toBe("C:/staging/b.mp4");
  });

  it("ignores pause and resume events that outlive their capture", async () => {
    // This store collapses Rust's two booleans into one tri-state, so unlike
    // the audio store (where `paused` is a separate flag that cannot fake a
    // recording) a stale `screen:paused` would set status="paused" from idle
    // and raise a paused bar with no startedAtMs behind it.
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    expect(store.status).toBe("idle");
    emit("screen:paused", { atMs: 4000 });
    expect(store.status).toBe("idle");
    expect(store.pausedSinceMs).toBeNull();
    emit("screen:resumed", { pausedTotalMs: 1500 });
    expect(store.status).toBe("idle");
    expect(store.pausedTotalMs).toBe(0);
  });

  it("clears the frame counters on the way to idle, so the next bar starts at zero", async () => {
    // reset() clears fps/dropped, but a capture can also end through
    // applyStatus(idle) — the refused-start reconcile and the pause/resume/
    // stop failure resyncs all land there. Leaving the counters live means
    // the NEXT capture's bar opens already reporting the previous capture's
    // dropped frames.
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.$patch({ status: "capturing", vaultId: "v1", startedAtMs: 0 });
    emit("screen:frames", { fps: 29.5, dropped: 12 });
    mockIPC((cmd) => {
      if (cmd === "screen_capture_status") return IDLE;
      throw new Error("No screen capture is running.");
    });
    await store.pause();
    expect(store.status).toBe("idle");
    expect(store.fps).toBe(0);
    expect(store.dropped).toBe(0);
    // Same rule, same arm, for the in-flight stop flag: a capture that ends
    // through applyStatus rather than reset() would otherwise hand the next
    // capture's bar a Stop button that is already disabled.
    store.$patch({ status: "capturing", startedAtMs: 0, stopping: true });
    store.applyStatus(IDLE);
    expect(store.stopping).toBe(false);
  });

  it("records advisory frame stats without touching capture state", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.$patch({ status: "capturing", vaultId: "v1", startedAtMs: 0 });
    emit("screen:frames", { fps: 29.5, dropped: 12 });
    expect(store.dropped).toBe(12);
    expect(store.fps).toBe(29.5);
    expect(store.status).toBe("capturing");
  });

  it("surfaces a live warning without ending the capture", async () => {
    // Spec 14: a source that vanishes mid-capture warns and finalizes
    // cleanly. Treating the warning as a failure would tear the UI down while
    // the capture is still recording.
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.$patch({ status: "capturing", vaultId: "v1", startedAtMs: 0 });
    emit("screen:warning", { message: "an audio device disappeared" });
    expect(store.warning).toBe("an audio device disappeared");
    expect(store.status).toBe("capturing");
    // The capture bar renders this inline while capturing, so a toast on top
    // of it would report the same fact twice.
    expect(useNotificationsStore().items).toHaveLength(0);
  });

  // A staged capture that has been exported into a vault, or discarded, is
  // GONE from the staging directory. `lastStaged` is the panel capture bar's
  // only handle on that footage and its Edit button invokes
  // `open_capture_editor` on it, so leaving it set offers Edit on a base
  // `load_staged_capture` can only refuse with a banner the user cannot act
  // on. Rust emits `screen:discarded` for exactly this (export_commands's
  // emit_discarded says so in as many words).
  const STAGED = {
    base: "2026-09-20 1000 Figma",
    path: "C:/staging/2026-09-20 1000 Figma.mp4",
    durationMs: 5000,
    sourceTitle: "Figma",
    width: 1920,
    height: 1080,
  };

  it("clears lastStaged when that capture is exported", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.lastStaged = { ...STAGED };
    emit("screen:exported", {
      base: STAGED.base,
      videoPath: "V:/vault/Screen Captures/a.mp4",
      notePath: null,
      vaultId: "v1",
      warning: null,
    });
    expect(store.lastStaged).toBeNull();
  });

  it("clears lastStaged when that capture is discarded", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.lastStaged = { ...STAGED };
    emit("screen:discarded", { base: STAGED.base });
    expect(store.lastStaged).toBeNull();
  });

  // The events are app-wide. Clearing on a base we are not holding would
  // throw away the handle on a DIFFERENT capture's footage — the exact
  // failure the single existing clear site's staleness guard exists to
  // prevent, reintroduced through a second clear that forgot to look.
  it("keeps lastStaged when another capture is exported or discarded", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.lastStaged = { ...STAGED };
    emit("screen:discarded", { base: "something-else" });
    expect(store.lastStaged?.base).toBe(STAGED.base);
    emit("screen:exported", {
      base: "something-else",
      videoPath: "V:/vault/x.mp4",
      notePath: null,
      vaultId: "v1",
      warning: null,
    });
    expect(store.lastStaged?.base).toBe(STAGED.base);
  });

  // Neither clear is a capture-lifecycle transition: the export happens long
  // after the capture ended. Bumping `seq` here would make an in-flight
  // `start()` discard its own reply as stale (and an in-flight `resync()`
  // discard a true status), so the guard must be identity, not generation.
  it("does not disturb the capture generation when it forgets a staged capture", async () => {
    mockIPC(() => undefined);
    const store = useScreenCaptureStore();
    await store.init();
    store.lastStaged = { ...STAGED };
    const seq = store.seq;
    emit("screen:discarded", { base: STAGED.base });
    expect(store.seq).toBe(seq);
  });
});
