/**
 * `WebcamDialog.vue` (Task 50; F-20, F-21; SCREENS 05; A08): opening it
 * touches no device, Enable camera asks exactly once, every way out stops
 * the camera, a refusal leaves the project alone, a take closes only after
 * asking, and Add to timeline places the take as its OWN clip on a new TOP
 * video track at the ADR's presenter placement — never baked into the
 * screen capture. The platform is faked (`helpers/fakeWebcam.ts`).
 */
import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import WebcamDialog from "../src/components/editor/dialogs/WebcamDialog.vue";
import MediaLibrary from "../src/components/editor/library/MediaLibrary.vue";
import { cornerPreset, PRESENTER_CORNER, presenterBox } from "../src/editor/layoutGeometry";
import { type EditorPort, EditorPortError } from "../src/editor/port";
import { PERMISSION_DENIED_TEXT } from "../src/editor/webcamRecorder";
import type { Clip, EditorCommand, EditorOpenResult, Project, TakeDto } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import placement from "./fixtures/editor-presenter-placement.json";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { type FakeDevices, fakeMediaDevices, FakeRecorder, resetFakeRecorder } from "./helpers/fakeWebcam";

enableAutoUnmount(afterEach);

const TAKE: TakeDto = { takeId: "take-7", assetId: "take-7", durationMs: 4_300, width: 1280, height: 720, hasAudio: false };

const SCREEN_CLIP: Clip = {
  id: "c-screen", asset_id: "src", track_id: "v1", name: "Screen", start_ms: 0, in_ms: 0, out_ms: 9_000,
  fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1,
};

function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    // Asymmetric canvas: a circle 0.19 wide is 0.3378 tall here, so a
    // swapped axis or a missing aspect correction shows.
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "src", kind: "video", name: "Screen.mp4", duration_ms: 9_000 }],
    tracks: [
      { id: "v1", kind: "video", name: "Screen", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      { id: "a1", kind: "audio", name: "Audio", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    ],
    clips: [{ ...SCREEN_CLIP }],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "", folder: "", dated: false },
  };
}

let devices: FakeDevices;
/** What `detect_ffmpeg` (the `useFfmpegStore` pre-flight) reports. */
let ffmpegInstalled: boolean;
let executed: EditorCommand[];
let begun: number;
let live: Project;

/** A minimal Rust: applies the three commands this dialog sends. */
function apply(command: EditorCommand): void {
  if (command.kind === "addTrack") {
    live.tracks.splice(command.index, 0, {
      id: "trk-new", kind: command.trackKind, name: command.name, visible: true, locked: false, muted: false, solo: false, volume: 1,
    });
  } else if (command.kind === "insertClip") {
    live.clips.push({
      ...SCREEN_CLIP, id: "clip-new", asset_id: command.assetId, track_id: command.trackId, name: "Take",
      start_ms: command.startMs, in_ms: command.inMs, out_ms: command.outMs,
    });
  } else if (command.kind === "setLayout") {
    const clip = live.clips.find((c) => command.clipIds.includes(c.id));
    if (clip) Object.assign(clip, { x: command.x, y: command.y, w: command.w, h: command.h });
  }
}

function snapshot(revision: number) {
  return {
    sessionId: "ses-a", projectId: "project-a", revision, persistedRevision: 5, title: "Tutorial",
    durationMs: 9_000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  };
}

async function openStore(extra: Partial<EditorPort> = {}) {
  live = project();
  let revision = 5;
  const opened: EditorOpenResult = {
    snapshot: snapshot(5), project: project(), workspace: {}, missing: [], sourceBase: null, recovered: false,
  };
  const port = fakeEditorPort({
    openStaged: () => Promise.resolve(opened),
    execute: (req) => {
      executed.push(req.command);
      apply(req.command);
      revision += 1;
      return Promise.resolve({ snapshot: snapshot(revision), project: structuredClone(live) });
    },
    getSnapshot: () => {
      live.assets.push({ id: "take-7", kind: "video", name: "Webcam take 1", duration_ms: 4_300 });
      return Promise.resolve({ snapshot: snapshot(revision), project: structuredClone(live) });
    },
    getJobs: () => Promise.resolve([]),
    webcamBegin: () => (begun++, Promise.resolve({ takeId: "take-7" })),
    webcamAppend: () => Promise.resolve(),
    webcamFinish: () => Promise.resolve(TAKE),
    webcamDiscard: () => Promise.resolve(),
    mediaUrl: () => Promise.resolve("C:\\p\\takes\\take-7.webm"),
    ...extra,
  });
  const store = useEditorProjectStore();
  store.setPort(port);
  await store.openStaged("base");
  useEditorWorkspaceStore().playheadMs = 4_200;
  return store;
}

function mountDialog(): VueWrapper {
  return mount(WebcamDialog, { props: { open: true }, attachTo: document.body });
}

async function click(w: VueWrapper, id: string): Promise<void> {
  await w.get(`[data-testid="${id}"]`).trigger("click");
  await flushPromises();
}

/** Enable, count down, record one chunk, stop: a finished take in review. */
async function recordTake(w: VueWrapper): Promise<void> {
  await click(w, "webcam-enable");
  await click(w, "webcam-record");
  await vi.advanceTimersByTimeAsync(3_000);
  await flushPromises();
  FakeRecorder.instances[0].emit([1, 2, 3]);
  await click(w, "webcam-stop");
}

beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  resetFakeRecorder();
  ffmpegInstalled = true;
  mockIPC((cmd) =>
    cmd === "detect_ffmpeg"
      ? { installed: ffmpegInstalled, version: null, path: null, ffprobePath: null, h264Encoder: null, configuredPath: null }
      : undefined,
  );
  mockConvertFileSrc("windows");
  devices = fakeMediaDevices();
  Object.defineProperty(navigator, "mediaDevices", { value: devices.mediaDevices, configurable: true });
  vi.stubGlobal("MediaRecorder", FakeRecorder);
  executed = [];
  begun = 0;
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  clearMocks();
  document.body.innerHTML = "";
});

describe("WebcamDialog — the camera", () => {
  it("opening the dialog requests no device", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    expect(w.get('[data-testid="webcam-dialog"]').text()).toMatch(/Enable camera/);
    expect(devices.requests).toEqual([]);
  });

  it("enable requests exactly once", async () => {
    await openStore();
    const w = mountDialog();
    await click(w, "webcam-enable");
    expect(devices.requests).toEqual([{ video: true, audio: false }]);
    expect(w.find('[data-testid="webcam-record"]').exists()).toBe(true);
  });

  it("close stops every track", async () => {
    await openStore();
    const w = mountDialog();
    await w.get('[data-testid="webcam-mic"]').setValue(true);
    await click(w, "webcam-enable");
    expect(devices.tracks()).toHaveLength(2);
    await click(w, "webcam-close");
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("pagehide stops every track", async () => {
    await openStore();
    const w = mountDialog();
    await click(w, "webcam-enable");
    window.dispatchEvent(new Event("pagehide"));
    await flushPromises();
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });

  it("denial leaves the project untouched and shows the permission copy", async () => {
    devices = fakeMediaDevices("NotAllowedError");
    Object.defineProperty(navigator, "mediaDevices", { value: devices.mediaDevices, configurable: true });
    const store = await openStore();
    const before = JSON.parse(JSON.stringify(store.project)) as Project;
    const w = mountDialog();
    await click(w, "webcam-enable");
    expect(w.get('[data-testid="webcam-problem"]').text()).toBe(PERMISSION_DENIED_TEXT);
    expect(executed).toEqual([]);
    expect(begun).toBe(0);
    expect(store.snapshot?.revision).toBe(5);
    expect(store.project).toEqual(before);
  });

  it("a known-missing ffmpeg is reported at Record, before any countdown (fix round 1)", async () => {
    ffmpegInstalled = false;
    await openStore();
    const w = mountDialog();
    await click(w, "webcam-enable");
    await click(w, "webcam-record");
    expect(w.find('[data-testid="webcam-countdown"]').exists()).toBe(false);
    expect(w.get('[data-testid="webcam-problem"]').text()).toMatch(/install ffmpeg/i);
    expect(begun).toBe(0);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });

  it("a missing ffmpeg says to install it, not that the camera failed", async () => {
    await openStore({
      webcamBegin: () =>
        // What the real port throws (`port.ts` wraps every refusal).
        Promise.reject(
          new EditorPortError({ code: "encoderUnavailable", message: "ffmpeg not found", retryable: false, operationId: "o" }),
        ),
    });
    const w = mountDialog();
    await click(w, "webcam-enable");
    await click(w, "webcam-record");
    await vi.advanceTimersByTimeAsync(3_000);
    await flushPromises();
    expect(w.get('[data-testid="webcam-problem"]').text()).toMatch(/install ffmpeg/i);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });
});

describe("WebcamDialog — the take", () => {
  it("add to timeline inserts a separate clip on a new top track with the presenter layout at the ADR placement constant", async () => {
    const store = await openStore();
    const w = mountDialog();
    await recordTake(w);
    await click(w, "webcam-add");
    expect(executed).toEqual([
      { kind: "addTrack", trackKind: "video", name: "Presenter", index: 0 },
      { kind: "insertClip", assetId: "take-7", trackId: "trk-new", startMs: 4_200, inMs: 0, outMs: 4_300 },
      {
        kind: "setLayout", clipIds: ["clip-new"], x: 0.775, y: 0.06, w: 0.19, h: 0.3378,
        frameShape: "circle", fit: "cover",
      },
    ]);
    // F35: the ADR's own placement, NOT cornerPreset's generic margin.
    const layout = executed[2] as { x: number; y: number };
    expect(layout.x === 0.775 && layout.y === 0.06).toBe(true);
    const generic = cornerPreset("tr", { width: 1280, height: 720 }, 0.19, { circle: true });
    expect([generic.x, generic.y]).not.toEqual([layout.x, layout.y]);
    expect(PRESENTER_CORNER).toEqual({ x: 0.775, y: 0.06, w: 0.19, frameShape: "circle", fit: "cover" });
    // A separate clip on the TOP track; the screen clip is untouched.
    expect(store.project?.tracks[0].id).toBe("trk-new");
    expect(store.project?.clips.find((c) => c.id === "c-screen")).toEqual(SCREEN_CLIP);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("review plays the finished take through editor_media_url", async () => {
    const asked: unknown[] = [];
    await openStore({ mediaUrl: (_s, ref) => (asked.push(ref), Promise.resolve("C:\\p\\takes\\take-7.webm")) });
    const w = mountDialog();
    await recordTake(w);
    expect(asked).toEqual([{ assetId: "take-7" }]);
    expect(w.find('[data-testid="product-player-video"]').exists()).toBe(true);
  });

  it("an unsaved take asks before closing", async () => {
    await openStore();
    const w = mountDialog();
    await recordTake(w);
    await click(w, "webcam-close");
    // Asked, not closed — and the camera is still live behind the question.
    expect(w.emitted("close")).toBeUndefined();
    const confirm = w.get('[data-testid="webcam-confirm"]');
    expect(confirm.text()).toMatch(/not on the timeline/i);
    expect(confirm.text()).toMatch(/stays in the media library/i);
    expect(devices.tracks().every((t) => !t.stopped)).toBe(true);
    await click(w, "webcam-confirm-back");
    expect(w.find('[data-testid="webcam-confirm"]').exists()).toBe(false);
    await click(w, "webcam-close");
    await click(w, "webcam-confirm-keep");
    expect(w.emitted("close")).toHaveLength(1);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
  });

  it("retake after a finished take says the earlier one stays in the library", async () => {
    let discards = 0;
    await openStore({ webcamDiscard: () => (discards++, Promise.resolve()) });
    const w = mountDialog();
    await recordTake(w);
    expect(w.get('[data-testid="webcam-dialog"]').text()).toMatch(/Retake keeps this take in the media library/);
    await click(w, "webcam-retake");
    expect(discards).toBe(0);
    expect(w.find('[data-testid="webcam-record"]').exists()).toBe(true);
  });

  it("choosing another camera asks for that camera, once enabled", async () => {
    await openStore();
    const w = mountDialog();
    await click(w, "webcam-enable");
    await w.get('[data-testid="webcam-device"]').setValue("cam-usb");
    await flushPromises();
    expect(devices.requests).toEqual([
      { video: true, audio: false },
      { video: { deviceId: { exact: "cam-usb" } }, audio: false },
    ]);
    expect(devices.streams[0].tracks.every((t) => t.stopped)).toBe(true);
  });

  it("closing while recording asks, and Discard recording removes the unfinished take", async () => {
    const discarded: string[] = [];
    await openStore({ webcamDiscard: (_s, takeId) => (discarded.push(takeId), Promise.resolve()) });
    const w = mountDialog();
    await click(w, "webcam-enable");
    await click(w, "webcam-record");
    await vi.advanceTimersByTimeAsync(3_000);
    await flushPromises();
    await click(w, "webcam-close");
    expect(w.get('[data-testid="webcam-confirm"]').text()).toMatch(/still recording/);
    await click(w, "webcam-confirm-keep");
    expect(discarded).toEqual(["take-7"]);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("Discard recording goes back to the live camera", async () => {
    const discarded: string[] = [];
    await openStore({ webcamDiscard: (_s, takeId) => (discarded.push(takeId), Promise.resolve()) });
    const w = mountDialog();
    await click(w, "webcam-enable");
    await click(w, "webcam-record");
    await vi.advanceTimersByTimeAsync(3_000);
    await flushPromises();
    await click(w, "webcam-cancel");
    expect(discarded).toEqual(["take-7"]);
    expect(w.find('[data-testid="webcam-record"]').exists()).toBe(true);
  });

  it("a refused placement keeps the dialog open with Rust's reason", async () => {
    await openStore({
      execute: () =>
        Promise.reject(
          new EditorPortError({ code: "invalidRequest", message: "Too many tracks.", retryable: false, operationId: "o" }),
        ),
    });
    const w = mountDialog();
    await recordTake(w);
    await click(w, "webcam-add");
    expect(w.text()).toContain("The take could not be added to the timeline. Too many tracks.");
    expect(w.emitted("close")).toBeUndefined();
    expect(w.find('[data-testid="webcam-add"]').exists()).toBe(true);
  });
});

describe("MediaLibrary — the Webcam entry", () => {
  it("opens the webcam dialog without touching the camera", async () => {
    await openStore();
    const w = mount(MediaLibrary, { attachTo: document.body });
    await flushPromises();
    expect(w.find('[data-testid="webcam-dialog"]').exists()).toBe(false);
    await click(w, "library-webcam");
    expect(w.find('[data-testid="webcam-dialog"]').exists()).toBe(true);
    expect(devices.requests).toEqual([]);
  });
});

// Task 51 (F35): the synchronized-webcam migration places the presenter in
// RUST (`core::editor::migrate`), the webcam dialog in TS. Both read ONE
// table (`core/src/editor/migrate_webcam_tests.rs` is the other reader), so
// neither language's copy can drift from the other.
describe("the presenter placement matches core::editor::migrate's", () => {
  it("PRESENTER_CORNER and presenterBox agree with the shared table", () => {
    const { x, y, w, frameShape, fit } = placement;
    expect(PRESENTER_CORNER).toEqual({ x, y, w, frameShape, fit });
    expect(placement.heights).toHaveLength(4);
    for (const { canvas, h } of placement.heights) {
      const box = presenterBox({ width: canvas[0], height: canvas[1] });
      expect({ canvas, box }).toEqual({ canvas, box: { x, y, w, h } });
    }
  });
});
