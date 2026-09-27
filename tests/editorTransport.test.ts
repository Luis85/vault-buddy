/**
 * The preview's transport and surface (Task 22, restyled to the concept by
 * visual-parity Task 12; F-04, F-14, F-25): `TransportBar.vue` (monitor
 * mute + peak meter + playback rate on the left; go to start/end, Play/Pause
 * and the mono timecode centred; the D10 canvas badge on the right) and
 * `PreviewSurface.vue` (the stage that hosts the non-reactive
 * `PreviewController`, asks Rust for every media path through the port, and
 * wires the workspace's monitoring state into the controller).
 *
 * Monitoring is LOCAL: every mute/rate test also asserts that no editor
 * command was sent — muting the preview is not an edit. The concept's own
 * transport carries no monitoring-volume slider (only a mute toggle), so
 * Task 12 dropped the one this app had added — monitoring plays at full
 * volume except when muted.
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
import { requestPlaybackFrom, requestReveal } from "../src/editor/revealBus";
import type { EditorOpenResult, MediaRef, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

function fakePort(overrides: Partial<EditorPort> = {}): EditorPort {
  return fakeEditorPort({
    getWorkspace: () => Promise.resolve({}),
    saveWorkspace: () => Promise.resolve(),
    ...overrides,
  });
}

let execute: ReturnType<typeof vi.fn<EditorPort["execute"]>>;

beforeEach(() => {
  setActivePinia(createPinia());
  execute = vi.fn<EditorPort["execute"]>(() =>
    Promise.reject(new Error("monitoring must never send an editor command")),
  );
  useEditorWorkspaceStore().setPort(fakePort({ execute }));
});

const SAMPLE_CANVAS = { width: 1280, height: 720, fps: 30 };

describe("TransportBar", () => {
  function mountBar(
    props: Partial<{
      playing: boolean;
      currentMs: number;
      durationMs: number;
      canvas: typeof SAMPLE_CANVAS | null;
      readPeak: () => number | null;
    }> = {},
  ) {
    return mount(TransportBar, {
      props: { playing: false, currentMs: 65_400, durationMs: 125_000, canvas: SAMPLE_CANVAS, ...props },
      attachTo: document.body,
    });
  }

  // §4.3: `fmt(ms, true)` = `MM:SS.d`, both minutes and seconds padded.
  it("shows the current and total time as MM:SS.d", () => {
    const w = mountBar();
    expect(w.get('[data-testid="transport-current"]').text()).toBe("01:05.4");
    expect(w.get('[data-testid="transport-total"]').text()).toBe("02:05.0");
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

  // `setPlayhead` itself clamps to `[0, editorProject.durationMs]` (0 with
  // no project open here, as in this standalone mount) -- a spy proves what
  // the buttons ask for regardless, leaving the clamp to the store's own
  // suite and the wiring end to end to `PreviewSurface`'s seek test below.
  it("Go to start and Go to end ask the workspace to seek to 0 and the duration", () => {
    const workspace = useEditorWorkspaceStore();
    const setPlayhead = vi.spyOn(workspace, "setPlayhead");
    const w = mountBar();
    w.get('[data-testid="transport-end"]').trigger("click");
    expect(setPlayhead).toHaveBeenLastCalledWith(125_000);
    w.get('[data-testid="transport-start"]').trigger("click");
    expect(setPlayhead).toHaveBeenLastCalledWith(0);
    expect(execute).not.toHaveBeenCalled();
  });

  it("mute toggles the workspace's monitor mute, names the toggle and sends no editor command", async () => {
    const workspace = useEditorWorkspaceStore();
    const w = mountBar();
    const mute = w.get('[data-testid="transport-mute"]');
    expect(mute.attributes("aria-pressed")).toBe("false");
    expect(mute.attributes("aria-label")).toBe("Mute monitoring");
    await mute.trigger("click");
    expect(workspace.monitorMuted).toBe(true);
    const same = w.get('[data-testid="transport-mute"]');
    expect(same.attributes("aria-pressed")).toBe("true");
    expect(same.attributes("aria-label")).toBe("Unmute monitoring");
    expect(execute).not.toHaveBeenCalled();
  });

  // §4.3's own four speeds, plus 0.25× (fix round 1, finding 2): the
  // workspace's stored rate still clamps to [0.25, 2.0], so a project saved
  // (or restored from a portable file) at 0.25x must find a matching option.
  it("the rate control offers 0.25x through 2x and sets the workspace rate", async () => {
    const workspace = useEditorWorkspaceStore();
    const w = mountBar();
    const options = w.findAll('[data-testid="transport-rate"] option').map((o) => o.text());
    expect(options).toEqual(["0.25×", "0.5×", "1×", "1.5×", "2×"]);
    await w.get('[data-testid="transport-rate"]').setValue("1.5");
    expect(workspace.playbackRate).toBe(1.5);
    expect(execute).not.toHaveBeenCalled();
  });

  // A rate the select has no matching option for renders as nothing
  // selected -- the exact D14 failure mode finding 2 exists to prevent.
  it("a stored 0.25x rate is a real, selected option, not a mismatch", () => {
    useEditorWorkspaceStore().setPlaybackRate(0.25);
    const w = mountBar();
    const select = w.get('[data-testid="transport-rate"]').element as HTMLSelectElement;
    expect(select.value).toBe("0.25");
  });

  it("the peak meter is silent (0% and never hot) while paused, whatever the controller last read", () => {
    const w = mountBar({ playing: false });
    const fill = w.get('[data-testid="transport-peak"] > *');
    expect(fill.attributes("style")).toContain("width: 0%");
    expect(fill.classes()).toContain("bg-audio");
  });

  it("the peak meter fills from the live peak while playing, and turns danger when hot", async () => {
    vi.useFakeTimers();
    try {
      let sample = 1; // 0 dBFS -> full width, and over the .98 hot threshold
      const w = mountBar({ playing: true, readPeak: () => sample });
      await vi.advanceTimersByTimeAsync(100);
      let fill = w.get('[data-testid="transport-peak"] > *');
      expect(fill.attributes("style")).toContain("width: 100%");
      expect(fill.classes()).toContain("bg-danger");

      sample = 0; // silence -> 0%, back to the ordinary audio colour
      await vi.advanceTimersByTimeAsync(100);
      fill = w.get('[data-testid="transport-peak"] > *');
      expect(fill.attributes("style")).toContain("width: 0%");
      expect(fill.classes()).toContain("bg-audio");
    } finally {
      vi.useRealTimers();
    }
  });

  // D10 §4.3: "{W} × {H} · {fps} fps · PREVIEW" from the project's own canvas.
  it("shows the D10 canvas badge, and hides it below the concept's 620px break", async () => {
    const w = mountBar();
    expect(w.get('[data-testid="transport-badge"]').text()).toBe("1280 × 720 · 30 fps · PREVIEW");

    useEditorWorkspaceStore().setViewport(600, 800);
    await w.vm.$nextTick();
    expect(w.find('[data-testid="transport-badge"]').exists()).toBe(false);
  });

  it("the badge reads PREVIEW alone when no project canvas is known yet", () => {
    const w = mountBar({ canvas: null });
    expect(w.get('[data-testid="transport-badge"]').text()).toBe("PREVIEW");
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

  async function mountSurface(mediaUrl: EditorPort["mediaUrl"], project: Project = PROJECT) {
    mockConvertFileSrc("windows");
    const store = useEditorProjectStore();
    const opened = { ...openResult(), project };
    store.setPort(fakePort({ openStaged: () => Promise.resolve(opened), mediaUrl, execute }));
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

  // docs/Gaps.md GAP-175: `migrate::from_staged` (core::editor::migrate)
  // marks a staged capture's own asset `builtin: Some(Builtin::Screen)` for
  // the reference format's sake, but — unlike every OTHER builtin — it is
  // backed by a real file in `sources.json` (`validate_media.rs`'s own
  // module doc). `computeLayers` must still lay this one out and the
  // controller must still ask the port for its media, or the one clip
  // every migrated project starts with shows no picture and no sound.
  it("lays out and requests media for a migrated capture's own builtin-screen asset", async () => {
    const migrated: Project = {
      ...PROJECT,
      assets: [{ id: "cap", kind: "video", name: "cap.mp4", duration_ms: 8_000, builtin: "screen" }],
    };
    const mediaUrl = vi.fn((_sid: string, _ref: MediaRef) => Promise.resolve("C:\\x\\cap.mp4"));
    const { w } = await mountSurface(mediaUrl, migrated);
    expect(mediaUrl).toHaveBeenCalledWith("ses-a", { assetId: "cap" });
    expect(w.find('[data-testid="preview-layers"] video').exists()).toBe(true);
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
    await w.get('[data-testid="transport-mute"]').trigger("click");
    expect(gains[0].gain.value).toBe(1);
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

  // Visual-parity Task 11 (design D15, no-op audit finding 5): the stage
  // click used to emit a `canvas-pointerdown` nothing listened to. It now
  // selects the TOPMOST visible clip under the pointer — mapped through the
  // letterbox — and an empty spot clears the selection.
  describe("clicking the picture selects (D15)", () => {
    const LAYERED: Project = {
      ...PROJECT,
      tracks: [
        { id: "v1", kind: "video", name: "Webcam", visible: true, locked: false, muted: false, solo: false, volume: 1 },
        { id: "v2", kind: "video", name: "Screen", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      ],
      clips: [
        { ...PROJECT.clips[0], id: "c1", track_id: "v2" },
        { ...PROJECT.clips[0], id: "c2", track_id: "v1", x: 0.75, y: 0.05, w: 0.2, h: 0.2 },
      ],
    };

    /** 1280x720 in an 800x600 stage at (20, 10) letterboxes to 800x450 at
     * top 75: canvas fraction (fx, fy) is client (20 + 800fx, 85 + 450fy). */
    async function surface() {
      const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"), LAYERED);
      const stage = w.get('[data-testid="preview-stage"]').element as HTMLElement;
      stage.getBoundingClientRect = () => ({ left: 20, top: 10, width: 800, height: 600, right: 820, bottom: 610, x: 20, y: 10, toJSON: () => ({}) });
      const click = (fx: number, fy: number) =>
        w.get('[data-testid="preview-stage"]').trigger("pointerdown", { button: 0, clientX: 20 + 800 * fx, clientY: 85 + 450 * fy });
      return { w, click, workspace: useEditorWorkspaceStore() };
    }

    it("a click over c1 alone selects c1; over the picture-in-picture selects the clip on top", async () => {
      const { click, workspace } = await surface();
      await click(0.5, 0.5);
      expect(workspace.selectionClipIds).toEqual(["c1"]);
      await click(0.85, 0.15);
      expect(workspace.selectionClipIds).toEqual(["c2"]);
    });

    it("a click outside every layer clears the selection and any selected cue", async () => {
      const { click, workspace } = await surface();
      workspace.select(["c1"]);
      workspace.setSelected({ type: "effect", id: "fx" });
      await click(0.5, -0.1); // the letterbox bar above the picture
      expect(workspace.selectionClipIds).toEqual([]);
      expect(workspace.selected).toBeNull();
    });

    // Fix round 1: a selected full-frame clip's layout box covers the whole
    // canvas and sits ABOVE the stage, so the press reaches the box, never
    // the stage. The PiP above it must still be clickable.
    it("with the full-frame clip selected, a press on its box over the PiP selects the PiP", async () => {
      const { w, workspace } = await surface();
      const rect = () => ({ left: 20, top: 10, width: 800, height: 600, right: 820, bottom: 610, x: 20, y: 10, toJSON: () => ({}) });
      (w.get('[data-testid="layout-handles"]').element as HTMLElement).getBoundingClientRect = rect;
      workspace.select(["c1"]);
      await flushPromises();
      const box = w.get('[data-testid="layout-box"]');
      await box.trigger("pointerdown", { button: 0, pointerId: 1, clientX: 20 + 800 * 0.85, clientY: 85 + 450 * 0.15 });
      expect(workspace.selectionClipIds).toEqual(["c2"]);

      // A press on the box away from any other picture is still a move of
      // the selected clip: it keeps its selection.
      workspace.select(["c1"]);
      await flushPromises();
      await w.get('[data-testid="layout-box"]').trigger("pointerdown", { button: 0, pointerId: 1, clientX: 420, clientY: 310 });
      expect(workspace.selectionClipIds).toEqual(["c1"]);
    });

    it("a hidden track's clip is not under the pointer", async () => {
      const hidden: Project = { ...LAYERED, tracks: [{ ...LAYERED.tracks[0], visible: false }, LAYERED.tracks[1]] };
      const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"), hidden);
      const stage = w.get('[data-testid="preview-stage"]').element as HTMLElement;
      stage.getBoundingClientRect = () => ({ left: 20, top: 10, width: 800, height: 600, right: 820, bottom: 610, x: 20, y: 10, toJSON: () => ({}) });
      await w.get('[data-testid="preview-stage"]').trigger("pointerdown", { button: 0, clientX: 20 + 800 * 0.85, clientY: 85 + 450 * 0.15 });
      expect(useEditorWorkspaceStore().selectionClipIds).toEqual(["c1"]);
    });

    it("a secondary-button press selects nothing", async () => {
      const { w, workspace } = await surface();
      await w.get('[data-testid="preview-stage"]').trigger("pointerdown", { button: 2, clientX: 420, clientY: 310 });
      expect(workspace.selectionClipIds).toEqual([]);
    });
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

  // Visual-parity Task 15 fix round 1: the Fades tab's "Preview entrance"
  // asks for playback from a clip's start; the surface seeks and plays.
  it("a playback request moves the playhead there and plays the preview", async () => {
    const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    requestPlaybackFrom(2_500);
    await flushPromises();
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    expect(useEditorWorkspaceStore().playheadMs).toBe(2_500);
    expect(video.currentTime).toBeCloseTo(2.5, 6);
    expect(video.paused).toBe(false);
    expect(w.get('[data-testid="transport-play"]').attributes("aria-label")).toBe("Pause");
    expect(execute).not.toHaveBeenCalled();
  });

  // Task 22 fix round 1: the mixer's "Play / pause preview" asks for the
  // transport's own toggle — play, then pause — never a second path.
  it("a play/pause request toggles the preview like the transport's Play", async () => {
    const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    requestReveal("playPause");
    await flushPromises();
    expect(video.paused).toBe(false);
    expect(w.get('[data-testid="transport-play"]').attributes("aria-label")).toBe("Pause");
    requestReveal("playPause");
    await flushPromises();
    expect(video.paused).toBe(true);
    expect(w.get('[data-testid="transport-play"]').attributes("aria-label")).toBe("Play");
  });

  it("a playhead moved elsewhere (the timeline) seeks the preview", async () => {
    const { w } = await mountSurface(() => Promise.resolve("C:\\x\\cap.mp4"));
    useEditorWorkspaceStore().setPlayhead(3_000);
    await flushPromises();
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    expect(video.currentTime).toBeCloseTo(3, 6);
    expect(w.get('[data-testid="transport-current"]').text()).toBe("00:03.0");
  });
});
