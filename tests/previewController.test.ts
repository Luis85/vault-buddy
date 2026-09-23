/**
 * The layered preview (Task 22; F-04, F-14, F-25): the pure geometry
 * (`previewGeometry.ts`), the pure layer computation (`previewLayers.ts`)
 * and the non-reactive `PreviewController` that applies it to pooled media
 * elements. happy-dom has no decoder and no Web Audio, so media elements
 * are real happy-dom elements whose `seeked` a test dispatches by hand, and
 * the `AudioContext` is an injected fake whose gain values a test reads.
 *
 * Every fixture is asymmetric on purpose (a non-square canvas, distinct
 * x/y/w/h, a non-unit speed) so a swapped axis or a dropped scale fails.
 */
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AudioContextLike, GainLike } from "../src/editor/previewController";
import { MAX_ELEMENTS, PreviewController } from "../src/editor/previewController";
import { clientToCanvas, containRect } from "../src/editor/previewGeometry";
import { computeLayers } from "../src/editor/previewLayers";
import type { Asset, Clip, Project, Track } from "../src/editorTypes";
import * as logging from "../src/logging";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}

function asset(id: string, overrides: Partial<Asset> = {}): Asset {
  return { id, kind: "video", name: `${id}.mp4`, duration_ms: 60_000, ...overrides };
}

function clip(id: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: `a-${id}`,
    track_id: trackId,
    name: id,
    start_ms: 0,
    in_ms: 0,
    out_ms: 10_000,
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
    ...overrides,
  };
}

function project(tracks: Track[], clips: Clip[], overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "p1",
    title: "Preview",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: clips.map((c) => asset(c.asset_id)),
    tracks,
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "", folder: "", dated: false },
    ...overrides,
  };
}

const STAGE = { width: 1000, height: 500 };
const LOUD = { muted: false, volume: 1 };

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

function controller(extra: Partial<ConstructorParameters<typeof PreviewController>[0]> = {}) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const c = new PreviewController({
    container,
    resolveUrl: (id) => Promise.resolve(`asset://${id}`),
    createAudioContext: () => null,
    requestFrame: () => 1,
    cancelFrame: () => undefined,
    ...extra,
  });
  c.setStage(STAGE);
  return { c, container };
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("previewGeometry", () => {
  // This task's named case: a PORTRAIT canvas pillarboxed inside a
  // LANDSCAPE stage. The stage itself sits away from the window origin, so
  // forgetting to subtract its client rect fails too.
  it("clientToCanvas undoes letterboxing on a portrait canvas in a landscape stage", () => {
    const stageRect = { left: 100, top: 40, width: 1000, height: 500 };
    const canvas = { width: 720, height: 1280 };
    const centre = clientToCanvas({ clientX: 100 + 500, clientY: 40 + 250 }, stageRect, canvas);
    expect(centre.x).toBeCloseTo(360, 6);
    expect(centre.y).toBeCloseTo(640, 6);
    // The canvas's own top-left corner, past the 359.375px pillarbox bar.
    const corner = clientToCanvas({ clientX: 100 + 359.375, clientY: 40 }, stageRect, canvas);
    expect(corner.x).toBeCloseTo(0, 6);
    expect(corner.y).toBeCloseTo(0, 6);
    // A click in the bar maps OUTSIDE the canvas rather than being clamped.
    expect(clientToCanvas({ clientX: 110, clientY: 290 }, stageRect, canvas).x).toBeLessThan(0);
  });

  it("containRect letterboxes a landscape canvas in a squarer stage", () => {
    expect(containRect({ width: 1280, height: 720 }, { width: 800, height: 600 })).toEqual({
      left: 0,
      top: 75,
      width: 800,
      height: 450,
    });
  });
});

describe("computeLayers / PreviewController.layout", () => {
  it("layout stacks upper tracks above lower tracks", () => {
    const p = project(
      [track("upper"), track("lower")],
      [
        clip("under", "lower"),
        clip("over", "upper", { x: 0.1, y: 0.2, w: 0.5, h: 0.25, opacity: 0.6 }),
      ],
    );
    const { c, container } = controller();
    const layers = c.layout(p, 1_000);

    expect(layers.map((l) => l.clipId)).toEqual(["over", "under"]);
    const [over, under] = layers;
    expect(over.z).toBeGreaterThan(under.z);
    // 1280x720 in 1000x500: scale 500/720, canvas box 888.9 x 500 at x=55.6.
    const scale = 500 / 720;
    const left = (1000 - 1280 * scale) / 2;
    expect(over.box!.left).toBeCloseTo(left + 0.1 * 1280 * scale, 6);
    expect(over.box!.top).toBeCloseTo(0.2 * 500, 6);
    expect(over.box!.width).toBeCloseTo(0.5 * 1280 * scale, 6);
    expect(over.box!.height).toBeCloseTo(0.25 * 500, 6);
    expect(over.opacity).toBe(0.6);

    // And the DOM agrees: the upper track's element paints above.
    const els = [...container.querySelectorAll<HTMLVideoElement>("video")];
    expect(els).toHaveLength(2);
    const byZ = els.map((e) => Number(e.style.zIndex)).sort((a, b) => b - a);
    expect(byZ[0]).toBe(over.z);
    expect(els.find((e) => Number(e.style.zIndex) === over.z)!.style.opacity).toBe("0.6");
  });

  it("hidden tracks are not laid out", () => {
    const p = project(
      [track("shown"), track("hidden", { visible: false })],
      [clip("a", "shown"), clip("b", "hidden")],
    );
    const { c, container } = controller();
    expect(c.layout(p, 500).map((l) => l.clipId)).toEqual(["a"]);
    expect(container.querySelectorAll("video")).toHaveLength(1);
  });

  it("an inactive clip is not laid out, and an active one reports its speed-mapped source time", () => {
    const p = project(
      [track("v")],
      [
        clip("later", "v", { start_ms: 5_000 }),
        clip("fast", "v", { start_ms: 1_000, in_ms: 2_000, out_ms: 12_000, speed: 2 }),
      ],
    );
    const [only] = computeLayers(p, 3_000, STAGE, LOUD);
    expect(only.clipId).toBe("fast");
    // (3000 - 1000) * 2 + 2000
    expect(only.sourceMs).toBe(6_000);
    expect(only.speed).toBe(2);
  });

  it("images get an <img>, audio tracks a hidden <audio>, builtins nothing", () => {
    const tracks = [track("v"), track("mus", { kind: "audio" })];
    const clips = [clip("pic", "v"), clip("song", "mus"), clip("card", "v")];
    const p = project(tracks, clips, {
      assets: [
        asset("a-pic", { media_type: "image" }),
        asset("a-song", { kind: "audio" }),
        asset("a-card", { builtin: "card" }),
      ],
    });
    const { c, container } = controller();
    const kinds = c.layout(p, 0).map((l) => [l.clipId, l.kind]);
    expect(kinds).toEqual(expect.arrayContaining([["pic", "image"], ["song", "audio"]]));
    expect(kinds).toHaveLength(2);
    expect(container.querySelectorAll("img")).toHaveLength(1);
    const audio = container.querySelector("audio")!;
    expect(audio.style.display).toBe("none");
  });

  it("pools at most MAX_ELEMENTS media elements and keeps the top-most", () => {
    const tracks = Array.from({ length: MAX_ELEMENTS + 3 }, (_, i) => track(`t${i}`));
    const clips = tracks.map((t) => clip(`c-${t.id}`, t.id));
    const { c, container } = controller();
    const shown = c.layout(project(tracks, clips), 0);
    expect(shown).toHaveLength(MAX_ELEMENTS);
    expect(shown[0].clipId).toBe("c-t0");
    expect(container.querySelectorAll("video")).toHaveLength(MAX_ELEMENTS);
  });

  it("resolves each asset's URL through the injected resolver, never a built path", async () => {
    const resolveUrl = vi.fn((id: string) => Promise.resolve(`http://asset.localhost/${id}`));
    const { c, container } = controller({ resolveUrl });
    c.layout(project([track("v")], [clip("a", "v")]), 0);
    await Promise.resolve();
    await Promise.resolve();
    expect(resolveUrl).toHaveBeenCalledWith("a-a");
    expect(container.querySelector("video")!.getAttribute("src")).toBe("http://asset.localhost/a-a");
  });
});

describe("monitoring", () => {

  beforeEach(() => setActivePinia(createPinia()));

  // Muting the MONITOR is local preview state: the gain the user hears goes
  // to zero, the project's own mix (clip/track volume, master gain) is
  // untouched, and no editor command is ever sent for it.
  it("monitor mute silences preview gain without an editor command", () => {
    const execute = vi.fn(() => Promise.reject(new Error("must not be called")));
    useEditorProjectStore().setPort(fakePort({ execute }));
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ execute }));
    const { ctx, gains } = fakeAudio();
    const { c } = controller({ createAudioContext: () => ctx });
    const p = project([track("v", { volume: 0.8 })], [clip("a", "v", { volume: 0.5 })], { master_gain: 0.9 });

    c.setMonitor({ muted: workspace.monitorMuted, volume: 1 });
    const [loud] = c.layout(p, 0);
    expect(gains).toHaveLength(1);
    expect(loud.gain).toBeCloseTo(0.5 * 0.8 * 0.9, 9);
    expect(gains[0].gain.value).toBeCloseTo(0.36, 9);

    workspace.toggleMonitorMute();
    c.setMonitor({ muted: workspace.monitorMuted, volume: 1 });
    expect(gains[0].gain.value).toBe(0);
    expect(p.clips[0].volume).toBe(0.5);
    expect(execute).not.toHaveBeenCalled();
  });

  it("a muted track, a muted clip and another track's solo each silence a layer", () => {
    const tracks = [track("solo", { solo: true }), track("plain"), track("m", { muted: true })];
    const clips = [clip("s", "solo"), clip("p", "plain"), clip("mm", "m"), clip("cm", "solo", { muted: true })];
    const gainOf = Object.fromEntries(computeLayers(project(tracks, clips), 0, STAGE, LOUD).map((l) => [l.clipId, l.gain]));
    expect(gainOf).toEqual({ s: 1, p: 0, mm: 0, cm: 0 });
  });
});

describe("pooling and fallbacks", () => {
  it("an element released when its clip ends is reused for a later clip, and a known URL is not re-resolved", async () => {
    const resolveUrl = vi.fn((id: string) => Promise.resolve(`asset://${id}`));
    const { c, container } = controller({ resolveUrl });
    const p = project(
      [track("v")],
      [clip("first", "v", { out_ms: 1_000 }), clip("second", "v", { start_ms: 2_000, asset_id: "a-first" })],
      { assets: [asset("a-first")] },
    );
    c.layout(p, 500);
    const el = container.querySelector("video");
    c.layout(p, 1_500);
    expect(container.querySelectorAll("video")).toHaveLength(0);
    await Promise.resolve();
    c.layout(p, 2_500);
    expect(container.querySelector("video")).toBe(el);
    expect(resolveUrl).toHaveBeenCalledTimes(1);
  });

  it("an AudioContext that throws falls back to element volume and mute", () => {
    const warn = vi.spyOn(logging, "logWarning").mockImplementation(() => undefined);
    const { c, container } = controller({
      createAudioContext: () => {
        throw new Error("no audio device");
      },
    });
    const p = project([track("v")], [clip("a", "v", { volume: 0.4 })]);
    c.layout(p, 0);
    const video = container.querySelector("video")!;
    expect(video.volume).toBeCloseTo(0.4, 9);
    c.setMonitor({ muted: true, volume: 1 });
    expect(video.muted).toBe(true);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("no AudioContext"));
    warn.mockRestore();
  });

  it("a layer Web Audio refuses to route still plays, on element volume", () => {
    const warn = vi.spyOn(logging, "logWarning").mockImplementation(() => undefined);
    const { ctx } = fakeAudio();
    ctx.createMediaElementSource = () => {
      throw new Error("already connected");
    };
    const { c, container } = controller({ createAudioContext: () => ctx });
    c.layout(project([track("v")], [clip("a", "v", { volume: 0.3 })]), 0);
    expect(container.querySelector("video")!.volume).toBeCloseTo(0.3, 9);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("Web Audio"));
    warn.mockRestore();
  });

  it("logs a layer overflow once per episode, not once per frame", () => {
    const warn = vi.spyOn(logging, "logWarning").mockImplementation(() => undefined);
    const tracks = Array.from({ length: MAX_ELEMENTS + 1 }, (_, i) => track(`t${i}`));
    const p = project(tracks, tracks.map((t) => clip(`c-${t.id}`, t.id, { out_ms: 1_000 })));
    const { c } = controller();
    c.layout(p, 0);
    c.layout(p, 10);
    expect(warn).toHaveBeenCalledTimes(1);
    c.layout(p, 2_000); // nothing active: the episode ends
    c.layout(p, 20);
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });

  it("a clip whose asset turns into an image swaps its <video> for an <img>", () => {
    const { c, container } = controller();
    const p = project([track("v")], [clip("a", "v")]);
    c.layout(p, 0);
    c.layout({ ...p, assets: [asset("a-a", { media_type: "image" })] }, 0);
    expect(container.querySelectorAll("video")).toHaveLength(0);
    expect(container.querySelectorAll("img")).toHaveLength(1);
  });

  it("playback stops at the project's end, and play() from the end starts over", () => {
    let now = 0;
    const frames: (() => void)[] = [];
    const playing: boolean[] = [];
    const { c } = controller({
      now: () => now,
      requestFrame: (cb) => frames.push(cb),
      onPlayingChange: (p) => playing.push(p),
    });
    c.layout(project([track("v")], [clip("a", "v", { out_ms: 100 })]), 50);
    c.setRate(2);
    c.play();
    now += 40;
    frames.shift()!();
    expect(c.playing).toBe(false);
    expect(c.timeMs).toBe(100);
    c.play();
    expect(c.timeMs).toBe(0);
    expect(playing).toEqual([true, false, true]);
    c.destroy();
    expect(c.playing).toBe(false);
  });
});

describe("teardown and failure paths (fix round 1)", () => {
  // A lookup still in flight when the controller is destroyed (a session
  // switch) must not point the now-detached, preload="auto" element at
  // media: it would start loading a file nobody can see.
  it("a URL lookup that settles after destroy() does not arm the detached element", async () => {
    let settle!: (url: string) => void;
    const resolveUrl = () => new Promise<string | null>((r) => (settle = r));
    const { c, container } = controller({ resolveUrl });
    c.layout(project([track("v")], [clip("a", "v")]), 0);
    const video = container.querySelector("video")!;
    c.destroy();
    settle("asset://late");
    await Promise.resolve();
    await Promise.resolve();
    expect(video.getAttribute("src")).toBeNull();
  });

  // A resolver that REJECTS (rather than answering null) is logged, never
  // an unhandled rejection — vitest fails the run on one.
  it("a rejecting resolver is logged, not left unhandled", async () => {
    const warn = vi.spyOn(logging, "logWarning").mockImplementation(() => undefined);
    const { c, container } = controller({ resolveUrl: () => Promise.reject(new Error("ipc gone")) });
    c.layout(project([track("v")], [clip("a", "v")]), 0);
    await Promise.resolve();
    await Promise.resolve();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("ipc gone"));
    expect(container.querySelector("video")!.getAttribute("src")).toBeNull();
    warn.mockRestore();
  });

  it("an AudioContext whose close() rejects is logged on destroy, not left unhandled", async () => {
    const warn = vi.spyOn(logging, "logWarning").mockImplementation(() => undefined);
    const { ctx } = fakeAudio();
    ctx.close = () => Promise.reject(new Error("InvalidStateError"));
    const { c } = controller({ createAudioContext: () => ctx });
    c.layout(project([track("v")], [clip("a", "v")]), 0);
    c.destroy();
    c.destroy();
    await Promise.resolve();
    await Promise.resolve();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("InvalidStateError"));
    warn.mockRestore();
  });
});

describe("seeking", () => {
  it("seek cancels an older pending seek", async () => {
    const { c, container } = controller();
    const p = project([track("v")], [clip("a", "v", { in_ms: 1_000, out_ms: 20_000 })]);
    c.layout(p, 0);
    const video = container.querySelector("video")!;

    const older = c.seek(2_000);
    const newer = c.seek(4_000);
    // The OLDER seek is already settled as cancelled, before any media
    // reports back — a scrub must never wait on a frame it moved past.
    await expect(older).resolves.toBe(false);
    video.dispatchEvent(new Event("seeked"));
    await expect(newer).resolves.toBe(true);
    expect(video.currentTime).toBeCloseTo(5, 6); // source = 1000 + 4000
    expect(c.timeMs).toBe(4_000);
  });

  // A seek is requested BY the playhead's owner; echoing the controller's
  // own (possibly clamped) time back would overwrite what the user just set.
  it("a seek does not echo its time back as a playhead report", () => {
    const onTime = vi.fn();
    const { c } = controller({ onTime });
    c.layout(project([track("v")], [clip("a", "v")]), 0);
    void c.seek(60_000);
    expect(c.timeMs).toBe(10_000);
    expect(onTime).not.toHaveBeenCalled();
  });

  it("reports the playhead at no more than 10 Hz while playing, and the final time on pause", () => {
    let now = 0;
    const frames: (() => void)[] = [];
    const onTime = vi.fn();
    const { c } = controller({
      now: () => now,
      onTime,
      requestFrame: (cb) => frames.push(cb),
    });
    c.layout(project([track("v")], [clip("a", "v")]), 0);
    c.play();
    for (let i = 0; i < 12; i += 1) {
      now += 16;
      frames.shift()!();
    }
    // 192 ms of frames at 60 Hz: one emit at ~112 ms, not twelve.
    expect(onTime.mock.calls.length).toBeLessThanOrEqual(2);
    expect(c.timeMs).toBeCloseTo(192, 6);
    onTime.mockClear();
    now += 16;
    c.pause();
    expect(c.playing).toBe(false);
    expect(onTime).toHaveBeenCalledWith(208);
  });
});
