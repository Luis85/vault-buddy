/**
 * The preview's transport and surface (Task 22; F-04, F-14, F-25):
 * `TransportBar.vue` (play/pause + Space, current/total time, monitoring
 * volume + mute, playback rate) and `PreviewSurface.vue` (the stage that
 * hosts the non-reactive `PreviewController`, asks Rust for every media
 * path through the port, and wires the workspace's monitoring state into
 * the controller).
 *
 * Monitoring is LOCAL: every mute/rate/volume test also asserts that no
 * editor command was sent — muting the preview is not an edit.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import TransportBar from "../src/components/editor/preview/TransportBar.vue";
import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import type { AudioContextLike, GainLike } from "../src/editor/previewController";
import type { EditorOpenResult, MediaRef, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

enableAutoUnmount(afterEach);

function fakePort(overrides: Partial<EditorPort> = {}): EditorPort {
  const unimplemented = (name: string) => (): never => {
    throw new Error(`fakePort.${name} not stubbed for this test`);
  };
  return {
    openStaged: unimplemented("openStaged"),
    openProject: unimplemented("openProject"),
    listProjects: unimplemented("listProjects"),
    getSnapshot: unimplemented("getSnapshot"),
    execute: unimplemented("execute"),
    save: unimplemented("save"),
    closeSession: unimplemented("closeSession"),
    hideWindow: unimplemented("hideWindow"),
    getWorkspace: () => Promise.resolve({}),
    saveWorkspace: () => Promise.resolve(),
    mediaUrl: unimplemented("mediaUrl"),
    ...overrides,
  };
}

let execute: ReturnType<typeof vi.fn<EditorPort["execute"]>>;

beforeEach(() => {
  setActivePinia(createPinia());
  execute = vi.fn<EditorPort["execute"]>(() =>
    Promise.reject(new Error("monitoring must never send an editor command")),
  );
  useEditorWorkspaceStore().setPort(fakePort({ execute }));
});

describe("TransportBar", () => {
  function mountBar(props: Partial<{ playing: boolean; currentMs: number; durationMs: number; volume: number }> = {}) {
    return mount(TransportBar, {
      props: { playing: false, currentMs: 65_000, durationMs: 125_000, volume: 1, ...props },
      attachTo: document.body,
    });
  }

  it("shows the current and total time", () => {
    const w = mountBar();
    expect(w.get('[data-testid="transport-current"]').text()).toBe("1:05");
    expect(w.get('[data-testid="transport-total"]').text()).toBe("2:05");
  });

  it("the play button toggles and names its own action", async () => {
    const w = mountBar();
    const button = w.get('[data-testid="transport-play"]');
    expect(button.attributes("aria-label")).toBe("Play");
    await button.trigger("click");
    expect(w.emitted("toggle-play")).toHaveLength(1);
    await w.setProps({ playing: true });
    expect(w.get('[data-testid="transport-play"]').attributes("aria-label")).toBe("Pause");
  });

  it("Space toggles playback, except where Space already means something", () => {
    const w = mountBar();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    expect(w.emitted("toggle-play")).toHaveLength(1);

    // Typing a space into a field is typing.
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    // A focused button's own Space is its native click — toggling here too
    // would play-then-pause on one keystroke.
    w.get('[data-testid="transport-play"]').element.dispatchEvent(
      new KeyboardEvent("keydown", { key: " ", bubbles: true }),
    );
    // A clip that already handled Space (select) says so.
    const handled = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    handled.preventDefault();
    window.dispatchEvent(handled);
    expect(w.emitted("toggle-play")).toHaveLength(1);
  });

  it("stops listening for Space once unmounted", () => {
    const w = mountBar();
    w.unmount();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    expect(w.emitted("toggle-play")).toBeUndefined();
  });

  it("mute toggles the workspace's monitor mute and sends no editor command", async () => {
    const workspace = useEditorWorkspaceStore();
    const w = mountBar();
    const mute = w.get('[data-testid="transport-mute"]');
    expect(mute.attributes("aria-pressed")).toBe("false");
    await mute.trigger("click");
    expect(workspace.monitorMuted).toBe(true);
    expect(w.get('[data-testid="transport-mute"]').attributes("aria-pressed")).toBe("true");
    expect(execute).not.toHaveBeenCalled();
  });

  it("the rate control sets the workspace playback rate", async () => {
    const workspace = useEditorWorkspaceStore();
    const w = mountBar();
    await w.get('[data-testid="transport-rate"]').setValue("1.5");
    expect(workspace.playbackRate).toBe(1.5);
    expect(execute).not.toHaveBeenCalled();
  });

  it("the volume slider reports a number between 0 and 1", async () => {
    const w = mountBar();
    await w.get('[data-testid="transport-volume"]').setValue("0.25");
    expect(w.emitted("update:volume")).toEqual([[0.25]]);
  });
});

describe("PreviewSurface", () => {
  const PROJECT: Project = {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "cap", kind: "video", name: "cap.mp4", duration_ms: 8_000 }],
    tracks: [{ id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips: [
      {
        id: "c1",
        asset_id: "cap",
        track_id: "v1",
        name: "cap",
        start_ms: 0,
        in_ms: 0,
        out_ms: 8_000,
        fade_in_ms: 0,
        fade_out_ms: 0,
        fade_curve: "linear",
        opacity: 1,
        volume: 1,
        muted: false,
        x: 0,
        y: 0,
        w: 1,
        h: 1,
      },
    ],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };

  function openResult(): EditorOpenResult {
    return {
      snapshot: {
        sessionId: "ses-a",
        projectId: "project-a",
        revision: 1,
        persistedRevision: null,
        title: "Tutorial",
        durationMs: 8_000,
        canUndo: false,
        canRedo: false,
        undoLabel: null,
        redoLabel: null,
      },
      project: PROJECT,
      workspace: {},
      missing: [],
      sourceBase: "base",
      recovered: false,
    };
  }

  function fakeAudio() {
    const gains: GainLike[] = [];
    const ctx: AudioContextLike = {
      destination: {},
      createGain() {
        const g: GainLike = { gain: { value: 1 }, connect: () => undefined };
        gains.push(g);
        return g;
      },
      createMediaElementSource: () => ({ connect: () => undefined }),
      resume: () => Promise.resolve(),
    };
    return { ctx, gains };
  }

  async function mountSurface(mediaUrl: EditorPort["mediaUrl"]) {
    mockConvertFileSrc("windows");
    const store = useEditorProjectStore();
    store.setPort(fakePort({ openStaged: () => Promise.resolve(openResult()), mediaUrl, execute }));
    await store.openStaged("base");
    const audio = fakeAudio();
    const w = mount(PreviewSurface, {
      props: { createAudioContext: () => audio.ctx },
      attachTo: document.body,
    });
    await flushPromises();
    return { w, gains: audio.gains };
  }

  it("asks Rust for the media path by asset id and plays it through the asset protocol", async () => {
    const PATH = "C:\\Users\\me\\AppData\\Local\\com.vaultbuddy.desktop\\screen-captures\\cap.mp4";
    const mediaUrl = vi.fn((_sid: string, _ref: MediaRef) => Promise.resolve(PATH));
    const { w } = await mountSurface(mediaUrl);
    expect(mediaUrl).toHaveBeenCalledWith("ses-a", { assetId: "cap" });
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    expect(video.getAttribute("src")).toBe(`http://asset.localhost/${encodeURIComponent(PATH)}`);
  });

  it("says so when a layer's media cannot be loaded, instead of a silent black frame", async () => {
    const mediaUrl = vi.fn(() =>
      Promise.reject(
        new EditorPortError({ code: "sourceMissing", message: "gone", retryable: false, operationId: "op" }),
      ),
    );
    const { w } = await mountSurface(mediaUrl);
    expect(w.get('[data-testid="preview-unavailable"]').text()).toContain("cap.mp4");
  });

  it("a non-editor failure (a transport error) is named the same way", async () => {
    const { w } = await mountSurface(() => Promise.reject(new Error("ipc down")));
    expect(w.get('[data-testid="preview-unavailable"]').text()).toContain("cap.mp4");
  });

  it("the transport's mute reaches the preview gain, with no editor command", async () => {
    const { w, gains } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    expect(gains).toHaveLength(1);
    expect(gains[0].gain.value).toBe(1);
    await w.get('[data-testid="transport-mute"]').trigger("click");
    expect(gains[0].gain.value).toBe(0);
    await w.get('[data-testid="transport-volume"]').setValue("0.5");
    await w.get('[data-testid="transport-mute"]').trigger("click");
    expect(gains[0].gain.value).toBe(0.5);
    expect(execute).not.toHaveBeenCalled();
  });

  it("the transport plays and pauses the preview, and the rate reaches the media", async () => {
    const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    await w.get('[data-testid="transport-rate"]').setValue("1.5");
    expect(video.playbackRate).toBe(1.5);
    await w.get('[data-testid="transport-play"]').trigger("click");
    expect(w.get('[data-testid="transport-play"]').attributes("aria-label")).toBe("Pause");
    expect(video.paused).toBe(false);
    await w.get('[data-testid="transport-play"]').trigger("click");
    expect(w.get('[data-testid="transport-play"]').attributes("aria-label")).toBe("Play");
    expect(video.paused).toBe(true);
    expect(execute).not.toHaveBeenCalled();
  });

  it("a pointer on the stage is reported in output-canvas pixels, letterbox undone", async () => {
    const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    const stage = w.get('[data-testid="preview-stage"]').element as HTMLElement;
    stage.getBoundingClientRect = () => ({ left: 20, top: 10, width: 800, height: 600, right: 820, bottom: 610, x: 20, y: 10, toJSON: () => ({}) });
    // 1280x720 in 800x600 letterboxes to 800x450 at top 75: the stage's
    // centre is the canvas centre.
    await w.get('[data-testid="preview-stage"]').trigger("pointerdown", { clientX: 420, clientY: 310 });
    const [[point]] = w.emitted("canvas-pointerdown") as [[{ x: number; y: number }]];
    expect(point.x).toBeCloseTo(640, 6);
    expect(point.y).toBeCloseTo(360, 6);
  });

  it("a new session rebuilds the preview and asks for media under the new session id", async () => {
    const mediaUrl = vi.fn((_sid: string, _ref: MediaRef) => Promise.resolve("C:\\x\\cap.mp4"));
    const { w } = await mountSurface(mediaUrl);
    const store = useEditorProjectStore();
    const second = openResult();
    second.snapshot = { ...second.snapshot, sessionId: "ses-b", projectId: "project-b" };
    second.project = { ...PROJECT, id: "project-b" };
    second.sourceBase = "other";
    store.setPort(fakePort({ openStaged: () => Promise.resolve(second), mediaUrl, execute }));
    await store.openStaged("other");
    await flushPromises();
    expect(mediaUrl).toHaveBeenLastCalledWith("ses-b", { assetId: "cap" });
    expect(w.findAll('[data-testid="preview-layers"] video')).toHaveLength(1);
  });

  // Fix round 1: a lookup from the PREVIOUS session that fails late must not
  // put a stale name into the new session's status line.
  it("a late failure from the previous session does not reach the new session's status line", async () => {
    let failOld!: (e: unknown) => void;
    const oldLookup = vi.fn(() => new Promise<string>((_resolve, reject) => (failOld = reject)));
    const { w } = await mountSurface(oldLookup);
    const store = useEditorProjectStore();
    const second = openResult();
    second.snapshot = { ...second.snapshot, sessionId: "ses-b", projectId: "project-b" };
    second.project = { ...PROJECT, id: "project-b" };
    second.sourceBase = "other";
    store.setPort(
      fakePort({ openStaged: () => Promise.resolve(second), mediaUrl: () => Promise.resolve("C:\\x\\cap.mp4"), execute }),
    );
    await store.openStaged("other");
    await flushPromises();
    failOld(new EditorPortError({ code: "sourceMissing", message: "gone", retryable: false, operationId: "op" }));
    await flushPromises();
    expect(w.find('[data-testid="preview-unavailable"]').exists()).toBe(false);
  });

  it("a playhead moved elsewhere (the timeline) seeks the preview", async () => {
    const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    useEditorWorkspaceStore().setPlayhead(3_000);
    await flushPromises();
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    expect(video.currentTime).toBeCloseTo(3, 6);
    expect(w.get('[data-testid="transport-current"]').text()).toBe("0:03");
  });
});
