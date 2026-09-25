/**
 * `webcamRecorder.ts` (Task 50; F-20; ADR R10): the camera's whole life in
 * the editor webview — nothing is requested before `enable`, a take is
 * streamed chunk by chunk to `editor_webcam_append` strictly in order, a
 * finished take is never deleted (GAP-195), and every track is stopped on
 * dispose. The platform is faked (`helpers/fakeWebcam.ts`); the port is the
 * shared fake.
 */
import { readFileSync } from "node:fs";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import {
  DEVICE_UNAVAILABLE_TEXT,
  ENCODER_UNAVAILABLE_TEXT,
  PERMISSION_DENIED_TEXT,
  TAKE_MIME_TYPES,
  type WebcamDeps,
  WebcamRecorder,
} from "../src/editor/webcamRecorder";
import type { TakeDto } from "../src/editorTypes";
import { logWarning } from "../src/logging";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { deferred, type FakeDevices, fakeMediaDevices, FakeRecorder, resetFakeRecorder } from "./helpers/fakeWebcam";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const TAKE: TakeDto = { takeId: "take-7", assetId: "take-7", durationMs: 4_300, width: 1280, height: 720, hasAudio: true };

/** Lets every queued promise continuation run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
  await new Promise((r) => setTimeout(r, 0));
}

let devices: FakeDevices;

beforeEach(() => {
  resetFakeRecorder();
  devices = fakeMediaDevices();
});

afterEach(() => {
  vi.mocked(logWarning).mockClear();
  FakeRecorder.supported = new Set(["video/webm;codecs=vp9,opus", "video/webm"]);
});

function recorder(port: Partial<EditorPort>, extra: Partial<WebcamDeps> = {}): WebcamRecorder {
  return new WebcamRecorder({
    port: fakeEditorPort({
      webcamBegin: () => Promise.resolve({ takeId: "take-7" }),
      ...port,
    }),
    sessionId: () => "ses-a",
    mediaDevices: devices.mediaDevices,
    Recorder: FakeRecorder,
    wait: () => Promise.resolve(),
    onChange: () => undefined,
    ...extra,
  });
}

function live(): FakeRecorder {
  const r = FakeRecorder.instances[FakeRecorder.instances.length - 1];
  if (!r) throw new Error("no MediaRecorder was created");
  return r;
}

describe("webcamRecorder — the take's wire", () => {
  it("chunks are sent strictly in order even if one append is slow", async () => {
    // The failure mode: two appends in flight at once. Rust admits ONLY the
    // next sequence number, so a later chunk overtaking a slow one fails
    // the whole take (and a lost chunk corrupts every cluster after it).
    const log: string[] = [];
    const slow = deferred();
    const r = recorder({
      webcamAppend: async (_s, _t, seq, bytes) => {
        log.push(`start ${seq}:${bytes[0]}`);
        if (seq === 0) await slow.promise;
        log.push(`end ${seq}`);
      },
    });
    await r.enable();
    await r.start();
    live().emit([10]);
    live().emit([11]);
    live().emit([12]);
    await settle();
    expect(log).toEqual(["start 0:10"]);
    slow.resolve();
    await settle();
    expect(log).toEqual(["start 0:10", "end 0", "start 1:11", "end 1", "start 2:12", "end 2"]);
  });

  it("records in the first SUPPORTED type Rust accepts, at a 1 s timeslice, after the countdown", async () => {
    const begun: string[] = [];
    const counts: (number | null)[] = [];
    const r = recorder(
      { webcamBegin: (_s, mime) => (begun.push(mime), Promise.resolve({ takeId: "take-7" })) },
      { onChange: (v) => counts.push(v.count) },
    );
    await r.enable();
    await r.start();
    expect(counts.filter((c) => c !== null)).toEqual([3, 2, 1]);
    expect(begun).toEqual(["video/webm;codecs=vp9,opus"]);
    expect(live().mimeType).toBe("video/webm;codecs=vp9,opus");
    expect(live().timeslice).toBe(1000);
    expect(r.view.state).toBe("recording");
  });

  it("the TS type list is exactly Rust's TAKE_MIME_TYPES, in order", () => {
    const rust = readFileSync("src-tauri/core/src/editor/take.rs", "utf8");
    const block = /TAKE_MIME_TYPES: \[&str; \d+\] = \[([\s\S]*?)\];/.exec(rust)?.[1] ?? "";
    expect([...block.matchAll(/"([^"]+)"/g)].map((m) => m[1])).toEqual([...TAKE_MIME_TYPES]);
  });

  it("stop finishes the take with the last sequence actually sent, then reviews it", async () => {
    const finished: number[] = [];
    const r = recorder({
      webcamAppend: () => Promise.resolve(),
      webcamFinish: (_s, _t, lastSeq) => (finished.push(lastSeq), Promise.resolve(TAKE)),
    });
    await r.enable();
    await r.start();
    live().emit([1]);
    live().emit([2]);
    await r.stop(); // the platform's final chunk is seq 2
    expect(finished).toEqual([2]);
    expect(r.view.state).toBe("review");
    expect(r.view.take).toEqual(TAKE);
  });

  it("a countdown can be cancelled, and then nothing is begun", async () => {
    const tick = deferred();
    let begun = 0;
    const r = recorder(
      { webcamBegin: () => (begun++, Promise.resolve({ takeId: "take-7" })) },
      { wait: () => tick.promise },
    );
    await r.enable();
    const starting = r.start();
    await settle();
    expect(r.view.state).toBe("countdown");
    await r.cancel();
    tick.resolve();
    await starting;
    expect(r.view.state).toBe("ready");
    expect(begun).toBe(0);
    expect(FakeRecorder.instances).toEqual([]);
  });
});

describe("webcamRecorder — keeping and dropping takes", () => {
  it("cancelling a recording discards the unfinished take", async () => {
    const discarded: string[] = [];
    const r = recorder({
      webcamAppend: () => Promise.resolve(),
      webcamDiscard: (_s, takeId) => (discarded.push(takeId), Promise.resolve()),
    });
    await r.enable();
    await r.start();
    live().emit([1]);
    await r.cancel();
    expect(discarded).toEqual(["take-7"]);
    expect(r.view.state).toBe("ready");
  });

  it("retake after a finished take goes back to ready and never deletes it", async () => {
    // GAP-195: a finished take is a registered asset; its file stays.
    let discards = 0;
    const r = recorder({
      webcamAppend: () => Promise.resolve(),
      webcamFinish: () => Promise.resolve(TAKE),
      webcamDiscard: () => (discards++, Promise.resolve()),
    });
    await r.enable();
    await r.start();
    live().emit([1]);
    await r.stop();
    await r.retake();
    expect(discards).toBe(0);
    expect(r.view.state).toBe("ready");
    expect(r.view.take).toBeNull();
    expect(devices.tracks().every((t) => !t.stopped)).toBe(true);
  });

  it("dispose stops every track and discards a take still recording", async () => {
    const discarded: string[] = [];
    const r = recorder({
      webcamAppend: () => Promise.resolve(),
      webcamDiscard: (_s, takeId) => (discarded.push(takeId), Promise.resolve()),
    });
    await r.enable(undefined, true);
    await r.start();
    r.dispose();
    await settle();
    expect(devices.tracks()).toHaveLength(2);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
    expect(discarded).toEqual(["take-7"]);
    expect(r.view.state).toBe("idle");
  });

  it("switching camera stops the previous stream's tracks", async () => {
    const r = recorder({});
    await r.enable();
    await r.enable("cam-usb", false);
    expect(devices.requests[1]).toEqual({ video: { deviceId: { exact: "cam-usb" } }, audio: false });
    expect(devices.streams[0].tracks.every((t) => t.stopped)).toBe(true);
    expect(devices.streams[1].tracks.every((t) => !t.stopped)).toBe(true);
  });

  it("lists cameras only, naming an unlabelled one", async () => {
    const r = recorder({});
    await r.enable();
    expect(r.view.cameras).toEqual([
      { deviceId: "cam-front", label: "Front camera" },
      { deviceId: "cam-usb", label: "Camera 2" },
    ]);
  });
});

describe("webcamRecorder — errors", () => {
  it.each([
    ["NotAllowedError", "permissionDenied", PERMISSION_DENIED_TEXT],
    ["NotFoundError", "deviceUnavailable", DEVICE_UNAVAILABLE_TEXT],
    ["NotReadableError", "deviceUnavailable", DEVICE_UNAVAILABLE_TEXT],
  ])("%s maps to the %s copy", async (name, kind, text) => {
    devices = fakeMediaDevices(name);
    const r = recorder({});
    await r.enable();
    expect(r.view.problem).toEqual({ kind, message: text });
    expect(r.view.state).toBe("idle");
  });

  it("a missing ffmpeg is the install-ffmpeg copy, not a camera error, and stops the camera", async () => {
    const r = recorder({
      webcamBegin: () =>
        Promise.reject(
          new EditorPortError({ code: "encoderUnavailable", message: "no ffmpeg", retryable: false, operationId: "o" }),
        ),
    });
    await r.enable();
    await r.start();
    expect(r.view.problem).toEqual({ kind: "encoderUnavailable", message: ENCODER_UNAVAILABLE_TEXT });
    expect(ENCODER_UNAVAILABLE_TEXT).toMatch(/install ffmpeg/i);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });
});

describe("webcamRecorder — failure and no-op paths", () => {
  const refusal = (code: "internal" | "invalidRequest") =>
    new EditorPortError({ code, message: `refused: ${code}`, retryable: false, operationId: "o" });

  it.each([
    ["SecurityError", "permissionDenied"],
    ["OverconstrainedError", "deviceUnavailable"],
    ["TypeError", "failed"],
  ])("%s is a %s problem", async (name, kind) => {
    devices = fakeMediaDevices(name);
    const r = recorder({});
    await r.enable();
    expect(r.view.problem?.kind).toBe(kind);
  });

  it("a window with no media devices reports an unavailable camera", async () => {
    const r = recorder({}, { mediaDevices: undefined });
    await r.enable();
    expect(r.view.problem).toEqual({ kind: "deviceUnavailable", message: DEVICE_UNAVAILABLE_TEXT });
  });

  it("a camera list that cannot be read degrades to none", async () => {
    const broken = { ...devices.mediaDevices, enumerateDevices: () => Promise.reject(new Error("no")) };
    const r = recorder({}, { mediaDevices: broken as unknown as MediaDevices });
    await r.enable();
    expect(r.view.state).toBe("ready");
    expect(r.view.cameras).toEqual([]);
  });

  it("no supported recording type (or no MediaRecorder) says so and begins nothing", async () => {
    let begun = 0;
    FakeRecorder.supported = new Set();
    const r = recorder({ webcamBegin: () => (begun++, Promise.resolve({ takeId: "take-7" })) });
    await r.enable();
    await r.start();
    expect(r.view.problem?.message).toMatch(/cannot record video/);
    expect(begun).toBe(0);
    const bare = recorder({}, { Recorder: undefined });
    await bare.enable();
    await bare.start();
    expect(bare.view.problem?.kind).toBe("failed");
  });

  it("a begin that answers after a cancel is discarded, never recorded", async () => {
    const answer = deferred<{ takeId: string }>();
    const discarded: string[] = [];
    const r = recorder({
      webcamBegin: () => answer.promise,
      webcamDiscard: (_s, takeId) => (discarded.push(takeId), Promise.resolve()),
    });
    await r.enable();
    const starting = r.start();
    await settle();
    await r.cancel();
    answer.resolve({ takeId: "take-late" });
    await starting;
    await settle();
    expect(discarded).toEqual(["take-late"]);
    expect(FakeRecorder.instances).toEqual([]);
  });

  it("a refused append fails the take, discards it and shows Rust's message", async () => {
    const discarded: string[] = [];
    const r = recorder({
      webcamAppend: () => Promise.reject(refusal("invalidRequest")),
      webcamDiscard: (_s, takeId) => (discarded.push(takeId), Promise.resolve()),
    });
    await r.enable();
    await r.start();
    live().emit([1]);
    await r.stop();
    expect(r.view.problem).toEqual({
      kind: "failed",
      message: "The take could not be recorded. refused: invalidRequest",
    });
    expect(discarded).toEqual(["take-7"]);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });

  it("a recorder error ends the take as a failure", async () => {
    const r = recorder({ webcamAppend: () => Promise.resolve(), webcamDiscard: () => Promise.resolve() });
    await r.enable();
    await r.start();
    live().onerror?.("boom");
    await r.stop();
    expect(r.view.problem?.message).toMatch(/could not be recorded\. boom/);
  });

  it("a take with no chunks is discarded as nothing recorded", async () => {
    FakeRecorder.finalBytes = [];
    let finished = 0;
    const r = recorder({
      webcamFinish: () => (finished++, Promise.resolve(TAKE)),
      webcamDiscard: () => Promise.resolve(),
    });
    await r.enable();
    await r.start();
    await r.stop();
    expect(finished).toBe(0);
    expect(r.view.problem?.message).toBe("Nothing was recorded.");
  });

  it("a refused finish is a failure, and a failing discard is only logged", async () => {
    const r = recorder({
      webcamAppend: () => Promise.resolve(),
      webcamFinish: () => Promise.reject(refusal("internal")),
      webcamDiscard: () => Promise.reject(new Error("gone")),
    });
    await r.enable();
    await r.start();
    await r.stop();
    expect(r.view.problem?.message).toMatch(/refused: internal/);
    r.dispose();
    await settle();
  });

  it("commit runs the placement once in review and returns to review when it did not land", async () => {
    const r = recorder({ webcamAppend: () => Promise.resolve(), webcamFinish: () => Promise.resolve(TAKE) });
    expect(await r.commit(() => Promise.resolve(true))).toBe(false); // nothing to place yet
    await r.enable();
    await r.start();
    await r.stop();
    const seen: string[] = [];
    const placed = await r.commit((take) => {
      seen.push(r.view.state, take.assetId);
      return Promise.resolve(false);
    });
    expect(placed).toBe(false);
    expect(seen).toEqual(["committing", "take-7"]);
    expect(r.view.state).toBe("review");
  });

  it("verbs out of their state do nothing", async () => {
    let calls = 0;
    const r = recorder({
      webcamBegin: () => (calls++, Promise.resolve({ takeId: "t" })),
      webcamDiscard: () => (calls++, Promise.resolve()),
    });
    await r.start(); // not enabled
    await r.stop(); // not recording
    await r.retake(); // not reviewing
    await r.cancel(); // nothing live
    expect(calls).toBe(0);
    expect(r.view.state).toBe("idle");
    expect(r.stream).toBeNull();
  });
});

describe("webcamRecorder — dispose mid-recording (fix round 1)", () => {
  /** A take mid-recording whose chunk 0 append is still in flight. */
  async function midRecording(append: () => Promise<void>, extra: Partial<WebcamDeps> = {}) {
    const log: string[] = [];
    const r = recorder(
      {
        webcamAppend: async (_s, _t, seq) => {
          log.push(`append ${seq}`);
          await append();
          log.push(`appended ${seq}`);
        },
        webcamDiscard: (_s, takeId) => (log.push(`discard ${takeId}`), Promise.resolve()),
      },
      extra,
    );
    await r.enable(undefined, true);
    await r.start();
    live().emit([1]);
    await settle();
    return { r, log };
  }

  it("stops every track at once, drains the append in flight, drops the final chunk, then discards", async () => {
    // The failure mode: dispose discarded the take while chunk 0 was still
    // being appended and then appended the recorder's final chunk to a take
    // Rust had already removed.
    const slow = deferred();
    const { r, log } = await midRecording(() => slow.promise);
    r.dispose();
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
    expect(r.view.state).toBe("idle");
    await settle();
    expect(log).toEqual(["append 0"]);
    slow.resolve();
    await settle();
    expect(log).toEqual(["append 0", "appended 0", "discard take-7"]);
  });

  it("an append that fails after the take was dropped is logged, never swallowed", async () => {
    const slow = deferred();
    const { r, log } = await midRecording(() => slow.promise);
    r.dispose();
    slow.reject(new Error("take gone"));
    await settle();
    expect(log).toEqual(["append 0", "discard take-7"]);
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(expect.stringMatching(/take-7.*chunk 0.*take gone/));
  });

  it("an append that never answers does not hold the discard forever", async () => {
    const { r, log } = await midRecording(() => new Promise(() => undefined), { drainLimitMs: 5 });
    r.dispose();
    await new Promise((resolve) => setTimeout(resolve, 30));
    await settle();
    expect(log).toEqual(["append 0", "discard take-7"]);
  });
});

describe("webcamRecorder — the ffmpeg pre-flight (fix round 1)", () => {
  it("a known-missing ffmpeg is reported before the countdown, and nothing is begun", async () => {
    let begun = 0;
    const counts: (number | null)[] = [];
    const r = recorder(
      { webcamBegin: () => (begun++, Promise.resolve({ takeId: "take-7" })) },
      {
        preflight: () => Promise.resolve({ kind: "encoderUnavailable", message: ENCODER_UNAVAILABLE_TEXT }),
        onChange: (v) => counts.push(v.count),
      },
    );
    await r.enable();
    await r.start();
    expect(counts.filter((c) => c !== null)).toEqual([]);
    expect(begun).toBe(0);
    expect(r.view.problem).toEqual({ kind: "encoderUnavailable", message: ENCODER_UNAVAILABLE_TEXT });
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });

  it("a pre-flight that finds nothing wrong leaves the native refusal as the authority", async () => {
    let begun = 0;
    const r = recorder(
      { webcamBegin: () => (begun++, Promise.resolve({ takeId: "take-7" })) },
      { preflight: () => Promise.resolve(null) },
    );
    await r.enable();
    await r.start();
    expect(begun).toBe(1);
    expect(r.view.state).toBe("recording");
  });
});
