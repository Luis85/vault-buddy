import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises } from "@vue/test-utils";
import { createPinia,setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
import { MAX_FINISHED, useCaptureStore } from "../src/stores/capture";
import { useNotificationsStore } from "../src/stores/notifications";

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

  it("keeps the saving UI when stop resolves stillSaving (GAP-20)", async () => {
    // A 15s finalize timeout used to resolve as a bare success; the typed
    // result must NOT flip the store out of "saving" — capture:saved/failed
    // events own that transition.
    mockIPC((cmd) => {
      if (cmd === "stop_capture") return { stillSaving: true };
      throw new Error(`unexpected ${cmd}`);
    });
    const store = useCaptureStore();
    store.status = "recording";
    await store.stop();
    expect(store.status).toBe("saving");
    expect(store.error).toBeNull();
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

  it("capture:started broadcast flips a non-initiating window to recording", async () => {
    // Regression (shipped in v0.3.0): the buddy window never calls start() —
    // it mounts, init()s (resync sees no recording), then learns a recording
    // began in the panel window only via the capture:started broadcast.
    // Without that listener the buddy stayed idle, so its red rec-dot and the
    // blink animation (both gated on status==='recording') never appeared.
    mockIPC((cmd) => {
      if (cmd === "capture_status") {
        return { recording: false, vaultId: null, startedAtMs: null };
      }
    });
    const store = useCaptureStore();
    await store.init();
    expect(store.status).toBe("idle");
    state.eventHandlers["capture:started"]!({
      payload: {
        recording: true,
        vaultId: "v1",
        startedAtMs: 123,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
      },
    });
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v1");
    expect(store.startedAtMs).toBe(123);
  });

  it("transcribeFailed moves the job to failed with the error message", async () => {
    mockIPC((cmd) => cmd === "capture_status" ? { recording: false, vaultId: null, startedAtMs: null } : { active: null, queued: [], waitingForRecording: false });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    state.eventHandlers["capture:transcribeFailed"]!({ payload: { mp3: "a.mp3", message: "model unavailable" } });
    expect(store.transcriptions["a.mp3"].phase).toBe("failed");
    expect(store.transcriptions["a.mp3"].error).toBe("model unavailable");
    expect(store.transcriptions["a.mp3"].progress).toBeNull();
    expect(active()).toBeNull();
  });

  it("dismissTranscription removes a terminal job but leaves in-flight work alone", () => {
    const store = useCaptureStore();
    store.transcriptions = {
      "failed.mp3": { mp3: "failed.mp3", vaultId: "v1", name: "Oops", phase: "failed", progress: null, model: null, error: "boom", startedAtMs: 2 },
      "active.mp3": { mp3: "active.mp3", vaultId: "v1", name: "Live", phase: "transcribing", progress: 0.5, model: null, error: null, startedAtMs: 3 },
      "queued.mp3": { mp3: "queued.mp3", vaultId: "v1", name: "Next", phase: "queued", progress: null, model: null, error: null, startedAtMs: 4 },
    };
    // A finished/failed row is history the user should be able to clear.
    store.dismissTranscription("failed.mp3");
    expect(store.transcriptions["failed.mp3"]).toBeUndefined();
    // But a dismiss must never silently drop in-flight or queued work — that
    // is what cancelTranscription is for. Guard against it.
    store.dismissTranscription("active.mp3");
    store.dismissTranscription("queued.mp3");
    expect(store.transcriptions["active.mp3"]).toBeDefined();
    expect(store.transcriptions["queued.mp3"]).toBeDefined();
  });

  it("surfaces a transcription failure reason as a notification", async () => {
    mockIPC((cmd) =>
      cmd === "capture_status"
        ? { recording: false, vaultId: null, startedAtMs: null }
        : { active: null, queued: [], waitingForRecording: false },
    );
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.init();
    state.eventHandlers["capture:transcribeFailed"]!({
      payload: { mp3: "/v/a.mp3", message: "whisper inference: bad model" },
    });
    expect(
      notes.items.some(
        (i) => i.kind === "error" && i.message.includes("whisper inference: bad model"),
      ),
    ).toBe(true);
    expect(capture.transcriptions["/v/a.mp3"].error).toBe("whisper inference: bad model"); // inline reason still set
  });

  it("transcribed event moves the job to done and surfaces in finishedTranscriptions", async () => {
    mockIPC((cmd) => cmd === "capture_status" ? { recording: false, vaultId: null, startedAtMs: null } : { active: null, queued: [], waitingForRecording: false });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    state.eventHandlers["capture:transcribed"]!({ payload: { mp3: "a.mp3", transcript: "a.transcript.md" } });
    expect(store.transcriptions["a.mp3"].phase).toBe("done");
    expect(store.transcriptions["a.mp3"].progress).toBe(1);
    expect(store.finishedTranscriptions.map((j) => j.mp3)).toEqual(["a.mp3"]);
    expect(active()).toBeNull();
  });

  it("cancelTranscription logs a warning on failure", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
      if (cmd === "transcription_queue_status") return { active: null, queued: [], waitingForRecording: false };
      if (cmd === "cancel_transcription") throw "job already finished";
    });
    const store = useCaptureStore();
    await store.init();
    await store.cancelTranscription("a.mp3");
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("cancel transcription rejected"),
    );
  });

  it("notifies when cancel is rejected", async () => {
    mockIPC((cmd) => {
      if (cmd === "cancel_transcription") throw new Error("No such transcription in the queue");
    });
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.cancelTranscription("/v/x.mp3");
    expect(notes.items.some((i) => i.message.includes("No such transcription"))).toBe(true);
  });

  it("retranscribe invokes retranscribe with the path", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    await store.retranscribe("a.mp3");
    expect(calls).toContainEqual({ cmd: "retranscribe", args: { path: "a.mp3" } });
  });

  it("notifies (not throws) when retranscribe is rejected", async () => {
    mockIPC((cmd) => {
      if (cmd === "retranscribe") throw new Error("Recording not found");
    });
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.retranscribe("/v/gone.mp3"); // must NOT reject
    expect(
      notes.items.some((i) => i.kind === "error" && i.message.includes("Recording not found")),
    ).toBe(true);
  });

  it("openTranscript invokes open_transcript with the path", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    await store.openTranscript("a.mp3");
    expect(calls).toContainEqual({ cmd: "open_transcript", args: { path: "a.mp3" } });
  });

  it("openTranscript logs a warning on failure", async () => {
    mockIPC(() => {
      throw "vault gone";
    });
    const store = useCaptureStore();
    await store.openTranscript("a.mp3");
    expect(store.warning).toContain("vault gone");
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("open transcript rejected"),
    );
  });

  it("notifies when open transcript is rejected", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_transcript") throw new Error("launch failed");
    });
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.openTranscript("/v/x.mp3");
    expect(notes.items.some((i) => i.message.includes("launch failed"))).toBe(true);
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

  const active = () => useCaptureStore().activeTranscription;

  it("seeds the job map from transcription_queue_status on init", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
      if (cmd === "transcription_queue_status")
        return { active: { mp3: "a.mp3", vaultId: "v1", phase: "transcribing", progress: 40, received: null, total: null, startedAtMs: 1 }, queued: [{ mp3: "b.mp3", vaultId: "v1" }], waitingForRecording: false };
    });
    const store = useCaptureStore();
    await store.init();
    expect(store.transcriptions["a.mp3"].phase).toBe("transcribing");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(0.4);
    expect(store.transcriptions["b.mp3"].phase).toBe("queued");
    expect(store.queuedTranscriptions.map((j) => j.mp3)).toEqual(["b.mp3"]);
  });

  it("seeds the active job's byte ratio for a downloading job with received/total (activeSeedProgress)", async () => {
    // Distinct from the "transcribing" seed test above: activeSeedProgress's
    // "downloading" branch prefers the received/total byte ratio over the
    // raw percent field, and this is the only test that seeds init from a
    // downloading (rather than transcribing) active job, so it's the only
    // one that actually exercises that branch.
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
      if (cmd === "transcription_queue_status")
        return {
          active: {
            mp3: "a.mp3",
            vaultId: "v1",
            phase: "downloading",
            progress: null,
            received: 30,
            total: 120,
            startedAtMs: 1,
          },
          queued: [],
          waitingForRecording: false,
        };
    });
    const store = useCaptureStore();
    await store.init();
    expect(store.transcriptions["a.mp3"].phase).toBe("downloading");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(30 / 120);
  });

  it("keeps finishedTranscriptions bounded to MAX_FINISHED after many jobs finish in one session", async () => {
    // finishedTranscriptions is a getter over the (now also bounded — see
    // the map-bounding test below) transcriptions map — this locks in that
    // the exposed list stays capped even as far more jobs than the cap
    // finish, so the visible "Finished this session" list never grows
    // without limit.
    mockIPC((cmd) =>
      cmd === "capture_status"
        ? { recording: false, vaultId: null, startedAtMs: null }
        : { active: null, queued: [], waitingForRecording: false },
    );
    const store = useCaptureStore();
    await store.init();
    for (let i = 0; i < MAX_FINISHED + 10; i++) {
      const mp3 = `job-${i}.mp3`;
      state.eventHandlers["capture:transcribing"]!({ payload: { mp3, vaultId: "v1" } });
      state.eventHandlers["capture:transcribed"]!({ payload: { mp3, transcript: `${mp3}.transcript.md` } });
    }
    expect(Object.keys(store.transcriptions)).toHaveLength(MAX_FINISHED);
    expect(store.finishedTranscriptions.length).toBe(MAX_FINISHED);
  });

  it("bounds the transcriptions map itself, never evicting an active or queued job", async () => {
    // Regression: only the finishedTranscriptions GETTER capped the
    // display — the underlying transcriptions map grew one entry per mp3
    // for the whole session. Eviction must only ever touch TERMINAL (done/
    // failed/cancelled) jobs, oldest first by startedAtMs — an active/
    // queued job must survive no matter how much unrelated terminal churn
    // happens around it.
    mockIPC((cmd) =>
      cmd === "capture_status"
        ? { recording: false, vaultId: null, startedAtMs: null }
        : { active: null, queued: [], waitingForRecording: false },
    );
    const store = useCaptureStore();
    await store.init();
    store.upsert("queued.mp3", { phase: "queued", vaultId: "v1" });

    for (let i = 0; i < MAX_FINISHED + 10; i++) {
      const mp3 = `job-${i}.mp3`;
      state.eventHandlers["capture:transcribing"]!({ payload: { mp3, vaultId: "v1" } });
      state.eventHandlers["capture:transcribed"]!({ payload: { mp3, transcript: `${mp3}.transcript.md` } });
    }

    const terminalCount = Object.values(store.transcriptions).filter((j) =>
      ["done", "failed", "cancelled"].includes(j.phase),
    ).length;
    expect(terminalCount).toBe(MAX_FINISHED);
    expect(store.transcriptions["queued.mp3"]?.phase).toBe("queued");
  });

  it("clears waitingForRecording once a job actually starts running, not just at init", async () => {
    // Backend truth (capture_commands.rs): waiting_for_recording = active
    // .is_none() && !pending.is_empty() && is_recording(&app) — recomputed
    // fresh only when queried. The frontend seeds it once at init and must
    // re-sync it itself whenever one of those three conditions could have
    // flipped, or a stale `true` lingers forever after the recording ends
    // or the job starts. Regression: previously only ever set from the
    // one-shot transcription_queue_status seed.
    mockIPC((cmd) =>
      cmd === "capture_status"
        ? { recording: true, vaultId: "v1", startedAtMs: 1 }
        : { active: null, queued: [{ mp3: "a.mp3", vaultId: "v1" }], waitingForRecording: true },
    );
    const store = useCaptureStore();
    await store.init();
    expect(store.waitingForRecording).toBe(true);

    // Trigger 1: the job actually starts running (active.is_none() flips).
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    expect(store.waitingForRecording).toBe(false);

    // Trigger 2 (defensive net): a live progress tick also proves a job is
    // running, in case a window missed the transcribing event itself.
    store.waitingForRecording = true;
    state.eventHandlers["capture:transcribeProgress"]!({ payload: { mp3: "a.mp3", progress: 10 } });
    expect(store.waitingForRecording).toBe(false);

    // Trigger 3: the awaited recording is saved (is_recording flips false).
    store.waitingForRecording = true;
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/a.mp3", note: null, endedEarly: false },
    });
    expect(store.waitingForRecording).toBe(false);
  });

  it.each([
    ["capture:transcribed", (mp3: string) => ({ mp3, transcript: `${mp3}.transcript.md` })],
    ["capture:transcribeFailed", (mp3: string) => ({ mp3, message: "boom" })],
    ["capture:transcribeSkipped", (mp3: string) => ({ mp3, message: "kept your existing transcript" })],
    ["capture:transcribeCancelled", (mp3: string) => ({ mp3 })],
  ])(
    "re-queries transcription_queue_status and re-arms waitingForRecording after %s",
    async (eventName, payloadFor) => {
      // Regression: waitingForRecording was only ever CLEARED after the
      // init-time seed — never re-armed — so it went stale-false once a job
      // finished while OTHER queued work still had no recording to
      // transcribe (e.g. mid a live recording). transcription_queue_status
      // is backend truth, recomputed fresh per query, so every terminal
      // event must re-query it rather than assume the wait is over.
      let waiting = false;
      mockIPC((cmd) =>
        cmd === "capture_status"
          ? { recording: false, vaultId: null, startedAtMs: null }
          : { active: null, queued: [], waitingForRecording: waiting },
      );
      const store = useCaptureStore();
      await store.init();
      store.waitingForRecording = false; // simulate an earlier immediate clear

      waiting = true;
      state.eventHandlers[eventName]!({ payload: payloadFor("z1.mp3") });
      await flushPromises();
      expect(store.waitingForRecording).toBe(true);

      waiting = false;
      state.eventHandlers[eventName]!({ payload: payloadFor("z2.mp3") });
      await flushPromises();
      expect(store.waitingForRecording).toBe(false);
    },
  );

  it("modelReady clears download progress and moves to preparing", async () => {
    mockIPC((cmd) => { if (cmd === "capture_status" || cmd === "transcription_queue_status") return cmd === "capture_status" ? { recording: false, vaultId: null, startedAtMs: null } : { active: null, queued: [], waitingForRecording: false }; });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    state.eventHandlers["capture:modelDownload"]!({ payload: { mp3: "a.mp3", model: "small", received: 5, total: 10 } });
    expect(store.transcriptions["a.mp3"].phase).toBe("downloading");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(0.5);
    state.eventHandlers["capture:modelReady"]!({ payload: { mp3: "a.mp3" } });
    expect(store.transcriptions["a.mp3"].phase).toBe("preparing");
    expect(store.transcriptions["a.mp3"].progress).toBeNull();
    state.eventHandlers["capture:transcribeProgress"]!({ payload: { mp3: "a.mp3", progress: 12 } });
    expect(store.transcriptions["a.mp3"].phase).toBe("transcribing");
    expect(store.transcriptions["a.mp3"].progress).toBeCloseTo(0.12);
  });

  it("cancelled event moves the job to cancelled; transcribingVaultId clears", async () => {
    mockIPC((cmd) => cmd === "capture_status" ? { recording: false, vaultId: null, startedAtMs: null } : { active: null, queued: [], waitingForRecording: false });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "a.mp3", vaultId: "v1" } });
    expect(store.transcribingVaultId).toBe("v1");
    state.eventHandlers["capture:transcribeCancelled"]!({ payload: { mp3: "a.mp3" } });
    expect(store.transcriptions["a.mp3"].phase).toBe("cancelled");
    expect(store.transcribingVaultId).toBeNull();
  });

  it("cancelTranscription invokes cancel_transcription with the path", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => { calls.push({ cmd, args }); if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null }; if (cmd === "transcription_queue_status") return { active: null, queued: [], waitingForRecording: false }; });
    const store = useCaptureStore();
    await store.init();
    await store.cancelTranscription("a.mp3");
    expect(calls).toContainEqual({ cmd: "cancel_transcription", args: { path: "a.mp3" } });
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

  it("a normal save raises no warning notification", async () => {
    // Regression: endedEarly/warning are both optional — a plain
    // { mp3, note, endedEarly: false } save with no warning text must stay
    // silent (endedEarly alone, false, is never a reason to warn).
    mockIPC(() => undefined);
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.init();
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/a.mp3", note: null, endedEarly: false },
    });
    expect(notes.items.some((i) => i.kind === "warning")).toBe(false);
  });

  it("an early-stopped save shows the backend's warning text verbatim", async () => {
    // The backend forms the complete, user-ready sentence; the store must
    // show it as-is. Regression: the old handler always prefixed
    // "Recording ended early: ", so this pins the exact (unprefixed) text.
    mockIPC(() => undefined);
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.init();
    state.eventHandlers["capture:saved"]!({
      payload: {
        mp3: "/v/a.mp3",
        note: null,
        endedEarly: true,
        warning: "recording ended early: disk full",
      },
    });
    const warn = notes.items.find((i) => i.kind === "warning");
    expect(warn?.message).toBe("recording ended early: disk full");
  });

  it("a companion-note-write failure warns verbatim, never claiming 'ended early'", async () => {
    // capture:saved.warning is dual-purpose: besides the early-stop reason
    // above, a post-save issue (e.g. the companion note failed to write)
    // is routed through the same field with endedEarly: false. Showing the
    // backend's text verbatim — instead of unconditionally prefixing
    // "Recording ended early:" — is the whole point of this fix.
    mockIPC(() => undefined);
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.init();
    state.eventHandlers["capture:saved"]!({
      payload: {
        mp3: "/v/a.mp3",
        note: "x",
        endedEarly: false,
        warning:
          "Saved the recording, but the companion note couldn't be written: perms",
      },
    });
    const warn = notes.items.find((i) => i.kind === "warning");
    expect(warn?.message).toContain("companion note couldn't be written");
    expect(warn?.message).not.toContain("ended early");
  });

  it("an early stop with no specific reason still gets a generic warning", async () => {
    mockIPC(() => undefined);
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.init();
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/a.mp3", note: null, endedEarly: true, warning: null },
    });
    const warn = notes.items.find((i) => i.kind === "warning");
    expect(warn?.message).toContain("ended early");
  });

  it("transcribeSkipped keeps the transcript complete and warns", async () => {
    mockIPC((cmd) =>
      cmd === "capture_status"
        ? { recording: false, vaultId: null, startedAtMs: null }
        : { active: null, queued: [], waitingForRecording: false },
    );
    const capture = useCaptureStore();
    const notes = useNotificationsStore();
    await capture.init();
    state.eventHandlers["capture:transcribeSkipped"]!({
      payload: {
        mp3: "/v/a.mp3",
        message: "kept your existing transcript — not overwritten",
      },
    });
    expect(capture.transcriptions["/v/a.mp3"].phase).toBe("done");
    expect(
      notes.items.some(
        (i) =>
          i.kind === "warning" &&
          i.message.includes("kept your existing transcript"),
      ),
    ).toBe(true);
  });

  it("transcribeSkipped marks the job skipped (so the buddy can stay quiet); a real transcription does not", async () => {
    mockIPC((cmd) =>
      cmd === "capture_status"
        ? { recording: false, vaultId: null, startedAtMs: null }
        : { active: null, queued: [], waitingForRecording: false },
    );
    const capture = useCaptureStore();
    await capture.init();
    state.eventHandlers["capture:transcribeSkipped"]!({
      payload: {
        mp3: "/v/a.mp3",
        message: "kept your existing transcript — not overwritten",
      },
    });
    expect(capture.transcriptions["/v/a.mp3"].phase).toBe("done");
    expect(capture.transcriptions["/v/a.mp3"].skipped).toBe(true);

    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/b.mp3", transcript: "/v/b.transcript.md" },
    });
    expect(capture.transcriptions["/v/b.mp3"].phase).toBe("done");
    expect(capture.transcriptions["/v/b.mp3"].skipped).toBeFalsy();
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

  it("rename raises a warning notification when the backend reports one", async () => {
    // Carry-forward from the Task-2 review: `warning` used to only set
    // store.warning, which (post Task 3) renders nowhere once idle — a
    // rename happens after the recording already finished, so the
    // RecordingBar (recording-only) is long gone by then.
    mockIPC((cmd) => {
      if (cmd === "rename_capture") {
        return {
          mp3: "/v/2026-07-04 1405 Standup.mp3",
          note: null,
          warning: "companion note rename failed: locked",
        };
      }
    });
    const store = useCaptureStore();
    const notes = useNotificationsStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.rename("Standup");
    expect(store.warning).toBe("companion note rename failed: locked");
    expect(
      notes.items.some(
        (i) =>
          i.kind === "warning" &&
          i.message.includes("companion note rename failed"),
      ),
    ).toBe(true);
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

  it("noteActiveProgress starts a clock on first observation and holds it through an unchanged re-observation", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(0));
    const store = useCaptureStore();
    const job = { mp3: "a.mp3", vaultId: "v1", name: "Standup", phase: "transcribing" as const, progress: 0.5, model: null, error: null, startedAtMs: 0 };
    store.noteActiveProgress(job);
    expect(store.activeStuckSinceMs).toBe(0);

    vi.setSystemTime(new Date(90_000));
    // Same job, same progress (whisper re-reporting the same %) — must NOT
    // reset the clock, or a slow-but-alive job would never trip the hint.
    store.noteActiveProgress({ ...job });
    expect(store.activeStuckSinceMs).toBe(0);
    expect(store.activeStuckMp3).toBe("a.mp3");
    vi.useRealTimers();
  });

  it("noteActiveProgress resets the clock on a real progress delta and clears when nothing is transcribing", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(0));
    const store = useCaptureStore();
    const job = { mp3: "a.mp3", vaultId: "v1", name: "Standup", phase: "transcribing" as const, progress: 0.5, model: null, error: null, startedAtMs: 0 };
    store.noteActiveProgress(job);

    vi.setSystemTime(new Date(110_000));
    store.noteActiveProgress({ ...job, progress: 0.6 }); // genuine advance
    expect(store.activeStuckSinceMs).toBe(110_000);

    store.noteActiveProgress(null); // job finished/vanished
    expect(store.activeStuckSinceMs).toBeNull();
    expect(store.activeStuckMp3).toBeNull();
    vi.useRealTimers();
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
