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
import { errorToasts, mountFeedback } from "./helpers/feedbackHost";

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

/** The dialog footer's buttons, in order. */
function footer(w: VueWrapper): string[] {
  return w.findAll("footer button").map((b) => b.text());
}

// Visual-parity Task 22 (concept spec §9.7, screen 05): the concept's
// anatomy — 960 wide, the 16:9 view with its "Camera is off" empty state
// and a status row on the left, "Set up your take" on the right, the
// privacy strip, and a footer per phase. No "Try demo overlay" (D10).
describe("WebcamDialog — the concept anatomy (§9.7)", () => {
  it("is 960 wide with the concept's title and subtitle", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    expect(w.get('[data-testid="dialog-host-content"]').attributes("style")).toContain("width: 960px");
    expect(w.get("h2").text()).toBe("Bring yourself into the tutorial");
    expect(w.get('[data-testid="dialog-host-content"] header p').text()).toBe("Your camera. A separate, editable layer.");
  });

  it("idle: the view says the camera is off, the settings column and the privacy strip sit beside it", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    expect(w.get('[data-testid="webcam-body"]').classes()).toContain("grid-cols-[minmax(0,1fr)_230px]");
    const view = w.get('[data-testid="webcam-view"]');
    expect(view.classes()).toContain("aspect-video");
    const empty = w.get('[data-testid="webcam-empty"]');
    expect(empty.get("b").text()).toBe("Camera is off");
    expect(empty.text()).toContain("Nothing is accessed until you do.");
    expect(w.get('[data-testid="webcam-status"]').text()).toContain("Camera off");
    expect(w.get('[data-testid="webcam-status"]').text()).toContain("Local recording · saved into this project");
    const settings = w.get('[data-testid="webcam-settings"]');
    expect(settings.get("h3").text()).toBe("Set up your take");
    expect(settings.text()).toContain("Include microphone");
    expect(w.get('[data-testid="webcam-privacy"]').text()).toMatch(/^Permission is explicit\./);
    expect(w.get('[data-testid="webcam-dialog"]').text()).not.toMatch(/demo/i);
    expect(footer(w)).toEqual(["Enable camera"]);
    expect(devices.requests).toEqual([]);
  });

  it("the camera choice waits for the camera and says so on screen", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    const select = w.get('[data-testid="webcam-device"]');
    expect((select.element as HTMLSelectElement).disabled).toBe(true);
    expect(select.text()).toContain("System default");
    expect(w.get('[data-testid="webcam-device-reason"]').text()).toBe("Choose a camera once it is on.");
    await click(w, "webcam-enable");
    expect((w.get('[data-testid="webcam-device"]').element as HTMLSelectElement).disabled).toBe(false);
    expect(w.get('[data-testid="webcam-device-reason"]').text()).toBe("");
  });

  it("says where the take will land: at the playhead, above the other video tracks", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    expect(w.get('[data-testid="webcam-placement"]').text()).toContain("0:04.2");
  });

  // Fix round 1 (finding 7): the concept's requesting footer — a note and
  // Cancel request — and the ✕ says what it is waiting for.
  it("while the camera is being asked for, the footer says so and offers Cancel request", async () => {
    await openStore();
    devices = fakeMediaDevices(null, { hold: true });
    Object.defineProperty(navigator, "mediaDevices", { value: devices.mediaDevices, configurable: true });
    const w = mountDialog();
    await click(w, "webcam-enable");
    expect(footer(w)).toEqual(["Cancel request"]);
    expect(w.get('[data-testid="webcam-footer-reason"]').text()).toBe("Waiting for the camera…");
    expect(w.get('[data-testid="webcam-footer-reason"]').attributes("aria-live")).toBe("polite");
    expect(w.get('[data-testid="webcam-close"]').attributes("title")).toBe("Waiting for the camera…");
  });

  it("Cancel request closes the dialog, and a late answer turns the camera straight off", async () => {
    await openStore();
    devices = fakeMediaDevices(null, { hold: true });
    Object.defineProperty(navigator, "mediaDevices", { value: devices.mediaDevices, configurable: true });
    const w = mountDialog();
    await w.get('[data-testid="webcam-mic"]').setValue(true);
    await click(w, "webcam-enable");
    await click(w, "webcam-cancel-request");
    expect(w.emitted("close")).toHaveLength(1);
    devices.release();
    await flushPromises();
    expect(devices.tracks()).toHaveLength(2);
    expect(devices.tracks().every((t) => t.stopped)).toBe(true);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("each phase has its own footer, and the status row follows", async () => {
    await openStore();
    const w = mountDialog();
    await click(w, "webcam-enable");
    expect(footer(w)).toEqual(["Start recording"]);
    expect(w.get('[data-testid="webcam-status"]').text()).toContain("Camera on");
    await click(w, "webcam-record");
    expect(footer(w)).toEqual(["Cancel countdown"]);
    await vi.advanceTimersByTimeAsync(3_000);
    await flushPromises();
    expect(footer(w)).toEqual(["Discard recording", "Stop & review"]);
    expect(w.get('[data-testid="webcam-status"]').text()).toContain("Recording");
    FakeRecorder.instances[0].emit([1, 2, 3]);
    await click(w, "webcam-stop");
    expect(footer(w)).toEqual(["Retake", "Add to timeline"]);
  });

  it("the close question sits in the footer", async () => {
    await openStore();
    const w = mountDialog();
    await recordTake(w);
    await click(w, "webcam-close");
    expect(footer(w)).toEqual(["Back to the take", "Keep in library and close"]);
    expect(w.get('footer [data-testid="webcam-confirm"]').text()).toMatch(/not on the timeline/i);
  });
});

// Ruling T7-1: a refused Add to timeline is said in the dialog, not toasted
// as well; an error from anything else while the dialog sits open still
// toasts.
describe("WebcamDialog — its own refusal is inline", () => {
  const refusal = () =>
    Promise.reject(new EditorPortError({ code: "invalidRequest", message: "Too many tracks.", retryable: false, operationId: "o" }));

  it("a refused placement is shown in the dialog and not toasted", async () => {
    await openStore({ execute: refusal });
    mountFeedback();
    const w = mountDialog();
    await recordTake(w);
    await click(w, "webcam-add");
    expect(w.get('[data-testid="webcam-place-error"]').text()).toBe(
      "The take could not be added to the timeline. Too many tracks.",
    );
    expect(errorToasts()).toEqual([]);
  });

  it("an unrelated refusal while the dialog is open still toasts", async () => {
    const store = await openStore({ execute: refusal });
    mountFeedback();
    const w = mountDialog();
    await flushPromises();
    await store.execute({ kind: "rename", title: "elsewhere" });
    await flushPromises();
    expect(errorToasts()).toEqual(["Too many tracks."]);
    expect(w.find('[data-testid="webcam-place-error"]').exists()).toBe(false);
  });
});

// D16: the close question takes focus on its safe answer, and leaving it
// puts focus on the footer's first live button, never on the page.
describe("WebcamDialog — focus around the close question", () => {
  it("focus moves to Back to the take, then to the footer when it goes", async () => {
    await openStore();
    const w = mountDialog();
    await recordTake(w);
    await click(w, "webcam-close");
    expect(document.activeElement?.getAttribute("data-testid")).toBe("webcam-confirm-back");
    await click(w, "webcam-confirm-back");
    expect(document.activeElement?.getAttribute("data-testid")).toBe("webcam-retake");
  });
});

// Task 22 fix round 1 (Ruling T22-1): the concept's microphone picker, mic
// level meter, "Insert at timeline time" and mirror are built on the
// native pieces that already exist.
describe("WebcamDialog — the microphone", () => {
  it("the microphone picker waits for the checkbox and says why on screen", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    const picker = w.get('[data-testid="webcam-mic-device"]');
    expect((picker.element as HTMLSelectElement).disabled).toBe(true);
    expect(w.get('[data-testid="webcam-mic-reason"]').text()).toBe("Turn on Include microphone to choose one.");
    await w.get('[data-testid="webcam-mic"]').setValue(true);
    expect(w.get('[data-testid="webcam-mic-reason"]').text()).toBe("Choose a microphone once the camera is on.");
    // Nothing asked for a device yet.
    expect(devices.requests).toEqual([]);
    // The checkbox is not described by the camera's reason.
    expect(w.get('[data-testid="webcam-mic"]').attributes("aria-describedby")).toBeUndefined();
  });

  it("lists the microphones once the camera is on, and a chosen one is asked for", async () => {
    await openStore();
    const w = mountDialog();
    await w.get('[data-testid="webcam-mic"]').setValue(true);
    await click(w, "webcam-enable");
    const picker = w.get('[data-testid="webcam-mic-device"]');
    expect((picker.element as HTMLSelectElement).disabled).toBe(false);
    expect(picker.text()).toContain("Microphone");
    expect(w.get('[data-testid="webcam-mic-reason"]').text()).toBe("");
    await picker.setValue("mic-1");
    await flushPromises();
    expect(devices.requests).toEqual([
      { video: true, audio: true },
      { video: true, audio: { deviceId: { exact: "mic-1" } } },
    ]);
    expect(devices.streams[0].tracks.every((t) => t.stopped)).toBe(true);
  });
});

/** A Web Audio stand-in: an analyser whose samples peak at `peak`. */
function fakeAudioContext(peak: number) {
  const contexts: { closed: boolean; disconnected: number }[] = [];
  class FakeAudioContext {
    readonly state = { closed: false, disconnected: 0 };
    constructor() {
      contexts.push(this.state);
    }
    createMediaStreamSource() {
      return { connect: () => undefined, disconnect: () => (this.state.disconnected += 1) };
    }
    createAnalyser() {
      return {
        fftSize: 1024,
        getFloatTimeDomainData: (a: Float32Array) => a.fill(0).fill(-peak, 3, 4),
        disconnect: () => (this.state.disconnected += 1),
      };
    }
    close() {
      this.state.closed = true;
      return Promise.resolve();
    }
  }
  return { FakeAudioContext, contexts };
}

/** Lets the meter's 100 ms poll run once (setInterval is not faked here). */
function poll(): Promise<void> {
  return new Promise((resolve) => {
    const id = setInterval(() => {
      clearInterval(id);
      resolve();
    }, 150);
  });
}

describe("WebcamDialog — the mic level preview", () => {
  it("reads the live microphone's peak and is torn down when the dialog closes", async () => {
    const audio = fakeAudioContext(0.5);
    vi.stubGlobal("AudioContext", audio.FakeAudioContext);
    await openStore();
    const w = mountDialog();
    await w.get('[data-testid="webcam-mic"]').setValue(true);
    await click(w, "webcam-enable");
    await poll();
    const meter = w.get('[data-testid="webcam-mic-meter"]');
    expect(Number(meter.attributes("aria-valuenow"))).toBeCloseTo(0.5, 5);
    expect(audio.contexts).toHaveLength(1);
    await click(w, "webcam-close");
    expect(audio.contexts[0]).toEqual({ closed: true, disconnected: 2 });
  });

  it("is torn down when the dialog unmounts", async () => {
    const audio = fakeAudioContext(0.25);
    vi.stubGlobal("AudioContext", audio.FakeAudioContext);
    await openStore();
    const w = mountDialog();
    await w.get('[data-testid="webcam-mic"]').setValue(true);
    await click(w, "webcam-enable");
    w.unmount();
    expect(audio.contexts[0]).toEqual({ closed: true, disconnected: 2 });
  });

  it("with the microphone off nothing is measured", async () => {
    const audio = fakeAudioContext(0.5);
    vi.stubGlobal("AudioContext", audio.FakeAudioContext);
    await openStore();
    const w = mountDialog();
    await click(w, "webcam-enable");
    expect(audio.contexts).toHaveLength(0);
    expect(w.get('[data-testid="webcam-mic-meter"]').attributes("aria-valuenow")).toBe("0");
  });
});

describe("WebcamDialog — where and how the take lands", () => {
  it("Insert at timeline time starts at the playhead and a chosen time reaches the insert", async () => {
    await openStore();
    const w = mountDialog();
    await flushPromises();
    const field = w.get('[data-testid="webcam-insert-at"]');
    expect((field.element as HTMLInputElement).value).toBe("4.20");
    await field.setValue("1.5");
    expect(w.get('[data-testid="webcam-placement"]').text()).toContain("0:01.5");
    await recordTake(w);
    await click(w, "webcam-add");
    expect(executed[1]).toEqual({
      kind: "insertClip", assetId: "take-7", trackId: "trk-new", startMs: 1_500, inMs: 0, outMs: 4_300,
    });
  });

  it("a time outside the project cannot be used, and says why on screen", async () => {
    await openStore();
    const w = mountDialog();
    await w.get('[data-testid="webcam-insert-at"]').setValue("20");
    await recordTake(w);
    const add = w.get('[data-testid="webcam-add"]');
    expect((add.element as HTMLButtonElement).disabled).toBe(true);
    expect(add.attributes("title")).toBe("Choose a time between 0 and 9.00 seconds.");
    expect(w.get('[data-testid="webcam-footer-reason"]').text()).toBe("Choose a time between 0 and 9.00 seconds.");
    expect((w.get('[data-testid="webcam-retake"]').element as HTMLButtonElement).disabled).toBe(false);
  });

  it("Mirror mirrors the live preview only, and the placed clip through its own flag", async () => {
    await openStore();
    const w = mountDialog();
    await w.get('[data-testid="webcam-mirror"]').setValue(true);
    await click(w, "webcam-enable");
    expect(w.get('[data-testid="webcam-live"]').classes()).toContain("scale-x-[-1]");
    await click(w, "webcam-record");
    await vi.advanceTimersByTimeAsync(3_000);
    await flushPromises();
    FakeRecorder.instances[0].emit([1, 2, 3]);
    await click(w, "webcam-stop");
    await click(w, "webcam-add");
    expect(executed[2]).toMatchObject({ kind: "setLayout", mirror: true });
  });
});
