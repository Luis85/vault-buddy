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

vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { logWarning } from "../src/logging";
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
    expect(calls).toEqual([
      { cmd: "start_capture", args: { id: "v1", mode: null } },
    ]);
    expect(store.status).toBe("recording");
    expect(store.startedAtMs).toBe(123);
  });

  it("passes an explicit mode override through to start_capture", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 123 };
      }
    });
    const store = useCaptureStore();
    await store.start("v1", "voice-note");
    expect(calls).toEqual([
      { cmd: "start_capture", args: { id: "v1", mode: "voice-note" } },
    ]);
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

  it("transcribed event records the file for the Open action", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/m.mp3", transcript: "/v/m.transcript.md" },
    });
    expect(store.lastTranscribed).toEqual({ mp3: "/v/m.mp3" });
  });

  it("a new recording clears the last transcribed marker", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") return { recording: true, vaultId: "v2", startedAtMs: 9 };
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/old.mp3" };
    await store.start("v2");
    expect(store.lastTranscribed).toBeNull();
  });

  it("openTranscript invokes open_transcript with the recording path", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    await store.openTranscript();
    expect(calls).toContainEqual({ cmd: "open_transcript", args: { path: "/v/m.mp3" } });
  });

  it("openTranscript clears the row on success", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_transcript") return undefined;
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    await store.openTranscript();
    expect(store.lastTranscribed).toBeNull();
  });

  it("openTranscript keeps the row and warns on failure", async () => {
    mockIPC(() => {
      throw "vault gone";
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    await store.openTranscript();
    expect(store.lastTranscribed).toEqual({ mp3: "/v/m.mp3" });
    expect(store.warning).toContain("vault gone");
  });

  it("dismissTranscribed clears the row without opening", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    store.dismissTranscribed();
    expect(store.lastTranscribed).toBeNull();
    expect(calls).not.toContain("open_transcript");
  });

  it("tracks which vault is transcribing, then clears it", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3", vaultId: "v7" } });
    expect(store.transcribingVaultId).toBe("v7");
    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/m.mp3", transcript: "/v/m.transcript.md" },
    });
    expect(store.transcribingVaultId).toBeNull();
  });

  it("clears the transcribing vault on failure too", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3", vaultId: "v7" } });
    state.eventHandlers["capture:transcribeFailed"]!({ payload: { mp3: "/v/m.mp3", message: "x" } });
    expect(store.transcribingVaultId).toBeNull();
  });

  it("pause and resume flow through IPC and mirror events", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1_000 };
      }
    });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    await store.pause();
    expect(calls).toContain("pause_capture");
    // Rust confirms via event — the store mirrors it, not the invoke
    expect(store.paused).toBe(false);
    state.eventHandlers["capture:level"]!({ payload: { peak: 0.8 } });
    expect(store.level).toBeCloseTo(0.8);
    state.eventHandlers["capture:paused"]!({ payload: { atMs: 5_000 } });
    expect(store.paused).toBe(true);
    expect(store.pausedSinceMs).toBe(5_000);
    // the meter must not freeze at the pre-pause peak while paused
    expect(store.level).toBe(0);
    await store.pause(); // already paused: no second IPC call
    expect(calls.filter((c) => c === "pause_capture")).toHaveLength(1);
    await store.resume();
    expect(calls).toContain("resume_capture");
    state.eventHandlers["capture:resumed"]!({
      payload: { pausedTotalMs: 2_500 },
    });
    expect(store.paused).toBe(false);
    expect(store.pausedSinceMs).toBeNull();
    expect(store.pausedTotalMs).toBe(2_500);
  });

  it("level events update the meter value, clamped to 0..1", async () => {
    mockIPC(() => undefined);
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:level"]!({ payload: { peak: 0.42 } });
    expect(store.level).toBeCloseTo(0.42);
    state.eventHandlers["capture:level"]!({ payload: { peak: 7 } });
    expect(store.level).toBe(1);
  });

  it("saved event opens the rename window and clears recording state", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1 };
      }
    });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    expect(store.vaultId).toBe("v1");
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/M/2026/07/2026-07-04 1405 Meeting.mp3", note: "/v/M/2026/07/2026-07-04 1405 Meeting.md", endedEarly: false },
    });
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.paused).toBe(false);
    expect(store.level).toBe(0);
    expect(store.lastSaved).toEqual({
      mp3: "/v/M/2026/07/2026-07-04 1405 Meeting.mp3",
      note: "/v/M/2026/07/2026-07-04 1405 Meeting.md",
    });
  });

  it("rename window expires after 30s", async () => {
    vi.useFakeTimers();
    mockIPC(() => undefined);
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/m.mp3", note: null, endedEarly: false },
    });
    expect(store.lastSaved).not.toBeNull();
    vi.advanceTimersByTime(29_000);
    expect(store.lastSaved).not.toBeNull();
    vi.advanceTimersByTime(2_000);
    expect(store.lastSaved).toBeNull();
    vi.useRealTimers();
  });

  it("rename calls rename_capture and updates the saved file", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "rename_capture") {
        return { mp3: "/v/2026-07-04 1405 Standup.mp3", note: null, warning: null };
      }
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.rename("Standup");
    expect(calls).toEqual([
      {
        cmd: "rename_capture",
        args: { mp3: "/v/2026-07-04 1405 Meeting.mp3", title: "Standup" },
      },
    ]);
    expect(store.lastSavedFile).toBe("/v/2026-07-04 1405 Standup.mp3");
    expect(store.lastSaved).toBeNull();
    expect(store.renameError).toBeNull();
  });

  it("rename failure keeps the prompt up with the error", async () => {
    mockIPC(() => {
      throw "Title is too long";
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.rename("x");
    expect(store.lastSaved).not.toBeNull();
    expect(store.renameError).toContain("Title is too long");
  });

  it("rename failure logs a warning through the log bridge", async () => {
    mockIPC(() => {
      throw "Title is too long";
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.rename("x");
    expect(store.renameError).not.toBeNull();
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("rename rejected"),
    );
  });

  it("pause failure logs a warning through the log bridge", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1 };
      }
      if (cmd === "pause_capture") {
        throw "Recording is still starting.";
      }
    });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    await store.pause();
    expect(store.error).not.toBeNull();
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("pause rejected"),
    );
  });

  it("a new recording dismisses the rename window", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v2", startedAtMs: 9 };
      }
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/old.mp3", note: null };
    await store.start("v2");
    expect(store.lastSaved).toBeNull();
  });

  it("init resyncs paused state from capture_status", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") {
        return {
          recording: true,
          vaultId: "v9",
          startedAtMs: 7,
          paused: true,
          pausedTotalMs: 1_500,
          pausedSinceMs: 9_000,
        };
      }
    });
    const store = useCaptureStore();
    await store.init();
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v9");
    expect(store.paused).toBe(true);
    expect(store.pausedTotalMs).toBe(1_500);
    expect(store.pausedSinceMs).toBe(9_000);
  });

  it("acceptRename with unchanged title dismisses without calling rename_capture", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.acceptRename("2026-07-04 1405 Meeting");
    expect(calls).toHaveLength(0);
    expect(store.lastSaved).toBeNull();
  });

  it("acceptRename with empty/whitespace title dismisses without calling rename_capture", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.acceptRename("   ");
    expect(calls).toHaveLength(0);
    expect(store.lastSaved).toBeNull();
  });

  it("acceptRename with edited title calls rename_capture and updates lastSavedFile", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "rename_capture") {
        return { mp3: "/v/2026-07-04 1405 Standup.mp3", note: null, warning: null };
      }
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.acceptRename("Standup");
    expect(calls).toEqual([
      {
        cmd: "rename_capture",
        args: { mp3: "/v/2026-07-04 1405 Meeting.mp3", title: "Standup" },
      },
    ]);
    expect(store.lastSavedFile).toBe("/v/2026-07-04 1405 Standup.mp3");
    expect(store.lastSaved).toBeNull();
  });
});
