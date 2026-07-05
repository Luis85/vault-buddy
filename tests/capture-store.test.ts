import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

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

import { useCaptureStore } from "../src/stores/capture";

describe("capture store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    state.eventHandlers = {};
  });

  afterEach(() => {
    clearMocks();
  });

  it("starts recording after a successful start_capture call", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 123 };
      }
    });
    const store = useCaptureStore();
    await store.start("v1");
    expect(calls).toEqual([{ cmd: "start_capture", args: { id: "v1" } }]);
    expect(store.status).toBe("recording");
    expect(store.startedAtMs).toBe(123);
  });

  it("ignores a second start while one is pending or active", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    let resolveFirst!: (v: unknown) => void;
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "start_capture") {
        return new Promise((resolve) => {
          resolveFirst = resolve;
        });
      }
    });
    const store = useCaptureStore();
    const first = store.start("v1");
    expect(store.status).toBe("starting");
    await store.start("v2"); // pending: must be ignored
    expect(calls).toHaveLength(1);
    resolveFirst({ recording: true, vaultId: "v1", startedAtMs: 123 });
    await first;
    expect(store.status).toBe("recording");
    await store.start("v2"); // active: must be ignored
    expect(calls).toHaveLength(1);
    expect(store.startedAtMs).toBe(123);
  });

  it("start failure surfaces the error and stays idle", async () => {
    mockIPC(() => {
      throw "No microphone found";
    });
    const store = useCaptureStore();
    await store.start("v1");
    expect(store.status).toBe("idle");
    expect(store.error).toContain("No microphone");
  });

  it("stop passes through saving and returns to idle on saved event", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") {
        return { recording: false, vaultId: null, startedAtMs: null }; // resync
      }
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1 };
      }
      if (cmd === "stop_capture") return undefined;
    });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    const stopping = store.stop();
    expect(store.status).toBe("saving");
    await stopping;
    // Simulate a prior failed stop attempt that left stale banners up —
    // a fresh save must clear them, not just the status.
    store.error = "boom";
    store.warning = "stale warning";
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/m.mp3", note: null, endedEarly: false },
    });
    expect(store.status).toBe("idle");
    expect(store.lastSavedFile).toBe("/v/m.mp3");
    expect(store.error).toBeNull();
    expect(store.warning).toBeNull();
  });

  it("failed event resets to idle with error", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") {
        return { recording: false, vaultId: null, startedAtMs: null };
      }
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:failed"]!({ payload: { message: "boom" } });
    expect(store.status).toBe("idle");
    expect(store.error).toBe("boom");
  });

  it("warning event is stored without changing status", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") {
        return { recording: false, vaultId: null, startedAtMs: null }; // resync
      }
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1 };
      }
    });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    state.eventHandlers["capture:warning"]!({
      payload: { message: "source lost: mic" },
    });
    expect(store.status).toBe("recording");
    expect(store.warning).toContain("source lost");
  });

  it("init resyncs from capture_status (app restarted mid-recording UI)", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "capture_status") {
        return { recording: true, vaultId: "v9", startedAtMs: 7 };
      }
    });
    const store = useCaptureStore();
    await store.init();
    expect(calls.map((c) => c.cmd)).toContain("capture_status");
    expect(store.status).toBe("recording");
    expect(store.startedAtMs).toBe(7);
  });

  it("transcribing event sets the transcribing flag", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3" } });
    expect(store.transcribing).toBe(true);
  });

  it("model download progress is tracked, then cleared on transcribed", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3" } });
    state.eventHandlers["capture:modelDownload"]!({
      payload: { model: "small", received: 5, total: 10 },
    });
    expect(store.modelDownload).toEqual({ model: "small", received: 5, total: 10 });
    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/m.mp3", transcript: "/v/m.transcript.md" },
    });
    expect(store.transcribing).toBe(false);
    expect(store.modelDownload).toBeNull();
  });

  it("transcribeFailed surfaces an error and the mp3 for retry", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribeFailed"]!({
      payload: { mp3: "/v/m.mp3", message: "model unavailable" },
    });
    expect(store.transcribing).toBe(false);
    expect(store.transcriptError).toBe("model unavailable");
    expect(store.transcriptFailedMp3).toBe("/v/m.mp3");
  });

  it("retryTranscription re-invokes the command for the failed file", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribeFailed"]!({
      payload: { mp3: "/v/m.mp3", message: "boom" },
    });
    await store.retryTranscription();
    expect(calls).toContainEqual({
      cmd: "transcribe_recording_now",
      args: { path: "/v/m.mp3" },
    });
    expect(store.transcriptError).toBeNull();
  });
});
