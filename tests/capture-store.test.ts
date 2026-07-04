import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
const listeners = new Map<string, (event: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

import { useCaptureStore } from "../src/stores/capture";

describe("capture store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    invoke.mockReset();
    listeners.clear();
  });

  it("starts recording and tracks the vault", async () => {
    invoke.mockResolvedValueOnce({
      recording: true,
      vaultId: "v1",
      startedAtMs: 123,
    });
    const store = useCaptureStore();
    await store.start("v1");
    expect(invoke).toHaveBeenCalledWith("start_capture", { id: "v1" });
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v1");
    expect(store.startedAtMs).toBe(123);
  });

  it("ignores a second start while one is pending or active", async () => {
    let resolveFirst!: (v: unknown) => void;
    invoke.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFirst = resolve;
      }),
    );
    const store = useCaptureStore();
    const first = store.start("v1");
    expect(store.status).toBe("starting");
    await store.start("v2"); // pending: must be ignored
    expect(invoke).toHaveBeenCalledTimes(1);
    resolveFirst({ recording: true, vaultId: "v1", startedAtMs: 123 });
    await first;
    expect(store.status).toBe("recording");
    await store.start("v2"); // active: must be ignored
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(store.vaultId).toBe("v1");
  });

  it("start failure surfaces the error and stays idle", async () => {
    invoke.mockRejectedValueOnce("No microphone found");
    const store = useCaptureStore();
    await store.start("v1");
    expect(store.status).toBe("idle");
    expect(store.error).toContain("No microphone");
  });

  it("stop passes through saving and returns to idle on saved event", async () => {
    invoke.mockResolvedValueOnce({ recording: false, vaultId: null, startedAtMs: null }); // capture_status resync
    invoke.mockResolvedValueOnce({ recording: true, vaultId: "v1", startedAtMs: 1 });
    invoke.mockResolvedValueOnce(undefined); // stop_capture
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    const stopping = store.stop();
    expect(store.status).toBe("saving");
    await stopping;
    listeners.get("capture:saved")!({
      payload: { mp3: "/v/m.mp3", note: null, endedEarly: false },
    });
    expect(store.status).toBe("idle");
    expect(store.lastSavedFile).toBe("/v/m.mp3");
  });

  it("failed event resets to idle with error", async () => {
    const store = useCaptureStore();
    await store.init();
    listeners.get("capture:failed")!({ payload: { message: "boom" } });
    expect(store.status).toBe("idle");
    expect(store.error).toBe("boom");
  });

  it("warning event is stored without changing status", async () => {
    invoke.mockResolvedValueOnce({ recording: false, vaultId: null, startedAtMs: null }); // capture_status resync
    invoke.mockResolvedValueOnce({ recording: true, vaultId: "v1", startedAtMs: 1 });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    listeners.get("capture:warning")!({ payload: { message: "source lost: mic" } });
    expect(store.status).toBe("recording");
    expect(store.warning).toContain("source lost");
  });

  it("init resyncs from capture_status (app restarted mid-recording UI)", async () => {
    invoke.mockResolvedValueOnce({ recording: true, vaultId: "v9", startedAtMs: 7 });
    const store = useCaptureStore();
    await store.init();
    expect(invoke).toHaveBeenCalledWith("capture_status");
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v9");
  });
});
