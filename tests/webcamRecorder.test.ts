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
    // The camera went off at Stop & review; Retake turned a new one on.
    expect(devices.streams).toHaveLength(2);
    expect(devices.streams[1].tracks.every((t) => !t.stopped)).toBe(true);
  });

  // Final review, Important 3 (Ruling F-1): the dialog's privacy line says
  // "Camera and microphone stop after recording or closing", but Stop &
  // review kept every track for Retake, the light stayed on and the review
  // status read "Camera on". The claim is now made true.
  it("Stop & review stops every camera and microphone track before the take is reviewed", async () => {
    const r = recorder({ webcamAppend: () => Promise.resolve(), webcamFinish: () => Promise.resolve(TAKE) });
    await r.enable("cam-usb", true, "mic-1");
    await r.start();
    live().emit([1]);
    await r.stop();
    expect(devices.tracks()).toHaveLength(2);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
    expect(r.stream).toBeNull();
    expect(r.view.state).toBe("review");
    expect(r.view.take).toEqual(TAKE);
  });

  it("the tracks stop only once the recorder has stopped, so its final chunk still lands", async () => {
    const appended: number[] = [];
    const r = recorder({
      webcamAppend: (_s, _t, seq) => (appended.push(seq), Promise.resolve()),
      webcamFinish: () => Promise.resolve(TAKE),
    });
    await r.enable();
    await r.start();
    const recorderStop = live().stop.bind(live());
    let liveAtStop: boolean | null = null;
    live().stop = () => {
      liveAtStop = devices.tracks().every((t) => !t.stopped);
      recorderStop();
    };
    live().emit([1]);
    await r.stop();
    expect(liveAtStop).toBe(true);
    expect(appended).toEqual([0, 1]);
  });

  it("Retake asks again for the same camera and microphone, through enable", async () => {
    const r = recorder({ webcamAppend: () => Promise.resolve(), webcamFinish: () => Promise.resolve(TAKE) });
    await r.enable("cam-usb", true, "mic-1");
    await r.start();
    live().emit([1]);
    await r.stop();
    await r.retake();
    expect(devices.requests).toHaveLength(2);
    expect(devices.requests[1]).toEqual(devices.requests[0]);
    expect(devices.requests[1]).toEqual({ video: { deviceId: { exact: "cam-usb" } }, audio: { deviceId: { exact: "mic-1" } } });
    expect(r.view.state).toBe("ready");
    expect(r.stream).toBe(devices.streams[1]);
  });

  it("a Retake the user leaves before the camera answers never turns it on", async () => {
    const r = recorder({ webcamAppend: () => Promise.resolve(), webcamFinish: () => Promise.resolve(TAKE) });
    await r.enable();
    await r.start();
    live().emit([1]);
    await r.stop();
    const held = fakeMediaDevices(null, { hold: true });
    Object.assign(devices.mediaDevices, held.mediaDevices);
    const retaking = r.retake();
    await settle();
    expect(r.view.state).toBe("requesting");
    r.dispose();
    held.release();
    await retaking;
    expect(held.tracks().every((t) => t.stopped)).toBe(true);
    expect(r.view.state).toBe("idle");
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

  // Fix round 1 (review Important): the discard-failure log carries the
  // error's stable code and operationId, never its message — which can
  // carry a capture's own name in plain text, not only a `<path:#hash8>`
  // handle the redaction scan can catch.
  it("logs a failed discard by code and operationId, never by message", async () => {
    const answer = deferred<{ takeId: string }>();
    const r = recorder({
      webcamBegin: () => answer.promise,
      webcamDiscard: () =>
        Promise.reject(
          new EditorPortError({
            code: "internal",
            message: "no staged capture named Secret Window",
            retryable: false,
            operationId: "op-discard",
          }),
        ),
    });
    await r.enable();
    const starting = r.start();
    await settle();
    await r.cancel();
    answer.resolve({ takeId: "take-late" });
    await starting;
    await settle();
    const line = vi
      .mocked(logWarning)
      .mock.calls.map((c) => c[0] as string)
      .find((l) => l.includes("discarding take"));
    expect(line).toBeDefined();
    expect(line).not.toContain("Secret");
    expect(line).toContain("internal");
    expect(line).toContain("op-discard");
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

  // Fix round 1 (review Important): the chunk-append-failure log carries
  // the error's stable code and operationId, never its message.
  it("logs a chunk-append failure by code and operationId, never by message", async () => {
    const r = recorder({
      webcamAppend: () =>
        Promise.reject(
          new EditorPortError({
            code: "internal",
            message: "no staged capture named Secret Window",
            retryable: false,
            operationId: "op-append",
          }),
        ),
      webcamDiscard: () => Promise.resolve(),
    });
    await r.enable();
    await r.start();
    live().emit([1]);
    await r.stop();
    const line = vi
      .mocked(logWarning)
      .mock.calls.map((c) => c[0] as string)
      .find((l) => l.includes("was not appended"));
    expect(line).toBeDefined();
    expect(line).not.toContain("Secret");
    expect(line).toContain("internal");
    expect(line).toContain("op-append");
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
    // Fix round 1: by code and operationId, not the raw message — a plain
    // `Error` (not an `EditorPortError`) falls back to `internal`/
    // `store-local` (`toEditorError`'s own guard), never "undefined".
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringMatching(/take-7.*chunk 0.*internal.*store-local/),
    );
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

// Task 22 fix round 1 (Ruling T22-1): the microphone is chosen like the
// camera, and a camera request answered after the dialog went away never
// leaves the camera on.
describe("webcamRecorder — the microphone and a late answer", () => {
  it("lists the microphones beside the cameras", async () => {
    const r = recorder({});
    await r.enable();
    expect(r.view.microphones).toEqual([{ deviceId: "mic-1", label: "Microphone" }]);
  });

  it("a chosen microphone reaches the request, the camera's own shape", async () => {
    const r = recorder({});
    await r.enable(undefined, true, "mic-1");
    expect(devices.requests).toEqual([{ video: true, audio: { deviceId: { exact: "mic-1" } } }]);
  });

  it("the microphone choice is ignored while the microphone is off", async () => {
    const r = recorder({});
    await r.enable("cam-usb", false, "mic-1");
    expect(devices.requests).toEqual([{ video: { deviceId: { exact: "cam-usb" } }, audio: false }]);
  });

  // Final review, minor 8: `enable` did not look at `epoch` again after
  // the device listing, so a close while it was pending let the listing's
  // end write "ready" over the closed view.
  it("a close while the devices are being listed leaves the camera off and the view idle", async () => {
    devices = fakeMediaDevices(null, { holdList: true });
    const states: string[] = [];
    const r = recorder({}, { onChange: (v) => states.push(v.state) });
    const enabling = r.enable();
    await settle();
    r.dispose();
    devices.releaseList();
    await enabling;
    expect(r.view.state).toBe("idle");
    expect(states.at(-1)).toBe("idle");
    expect(states).not.toContain("ready");
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });

  it("a request answered after dispose stops its tracks at once and changes nothing", async () => {
    devices = fakeMediaDevices(null, { hold: true });
    const r = recorder({});
    const pending = r.enable(undefined, true);
    r.dispose();
    devices.release();
    await pending;
    await settle();
    expect(devices.tracks()).toHaveLength(2);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
    expect(r.stream).toBeNull();
    expect(r.view.state).toBe("idle");
  });
});
