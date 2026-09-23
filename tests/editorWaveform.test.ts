/**
 * Task 28 (F-26): waveforms and thumbnails on the timeline. Rust derives
 * both (`editor_media_peaks` / `editor_media_thumbnail`,
 * `src-tauri/src/editor/media_derive.rs`); the timeline asks only for the
 * clips it actually renders (`timelineLayout.visibleClips`' window), draws
 * the peaks as ONE SVG polyline per clip, and turns a missing ffmpeg into
 * the install hint rather than an empty lane.
 */
import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ClipThumbnail from "../src/components/editor/timeline/ClipThumbnail.vue";
import ClipWaveform from "../src/components/editor/timeline/ClipWaveform.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import { decodeMediaPeaks } from "../src/editor/decode";
import { clearMediaDerivedForTest, loadPeaks, loadThumbnail } from "../src/editor/mediaDerived";
import type { EditorPort } from "../src/editor/port";
import { createTauriEditorPort, EditorPortError } from "../src/editor/port";
import { peakBucketsFor, waveformPoints } from "../src/editor/waveform";
import type { Asset, Clip, EditorOpenResult, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  clearMediaDerivedForTest();
});

afterEach(() => clearMocks());

// ---- fixtures ---------------------------------------------------------------
// Two audio assets of DIFFERENT lengths, so a request carrying the wrong
// asset's bucket count cannot pass; one clip inside the viewport and one far
// outside it (the default zoom draws 50 px per second, so 60 s is 3000 px).

function track(id: string, kind: "video" | "audio"): Track {
  return { id, kind, name: id, visible: true, locked: false, muted: false, solo: false, volume: 1 };
}

function asset(id: string, kind: "video" | "audio", durationMs: number): Asset {
  return { id, kind, name: id, duration_ms: durationMs };
}

function clip(id: string, assetId: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: assetId,
    track_id: trackId,
    name: id,
    start_ms: 0,
    in_ms: 0,
    out_ms: 1_000,
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

function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [asset("snd-near", "audio", 8_000), asset("snd-far", "audio", 3_000)],
    tracks: [track("a1", "audio")],
    clips: [
      clip("near", "snd-near", "a1", { start_ms: 0, in_ms: 2_000, out_ms: 6_000 }),
      clip("far", "snd-far", "a1", { start_ms: 60_000, in_ms: 0, out_ms: 3_000 }),
    ],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

async function openWith(overrides: Partial<EditorPort>, projectOverrides: Partial<Project> = {}) {
  const store = useEditorProjectStore();
  const p = project(projectOverrides);
  store.setPort(
    fakeEditorPort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({
          snapshot: {
            sessionId: "ses-a",
            projectId: "project-a",
            revision: 1,
            persistedRevision: null,
            title: "Tutorial",
            durationMs: 63_000,
            canUndo: false,
            canRedo: false,
            undoLabel: null,
            redoLabel: null,
          },
          project: p,
          workspace: {},
          missing: [],
          sourceBase: "base",
          recovered: false,
        }),
      ...overrides,
    }),
  );
  await store.openStaged("base");
}

async function mountTimeline(overrides: Partial<EditorPort>, projectOverrides: Partial<Project> = {}) {
  await openWith(overrides, projectOverrides);
  const w = mount(TimelineView, { props: { viewportWidth: 400 } });
  await flushPromises();
  return w;
}

// ---- the brief's named cases -------------------------------------------------

describe("the timeline's waveforms", () => {
  it("waveform renders only for visible clips", async () => {
    const mediaPeaks = vi.fn((_sid: string, assetId: string, buckets: number) =>
      Promise.resolve(Array.from({ length: buckets }, (_, i) => (assetId === "snd-near" ? (i % 4) / 4 : 1))),
    );
    const w = await mountTimeline({ mediaPeaks });

    const near = w.find('[data-testid="clip-near-waveform"] polyline');
    expect(near.exists()).toBe(true);
    expect((near.attributes("points") ?? "").split(" ").length).toBeGreaterThan(2);
    // The far clip is outside the virtualization window: no ClipItem, so no
    // waveform — and no decode was ever asked for.
    expect(w.find('[data-testid="clip-far"]').exists()).toBe(false);
    expect(w.find('[data-testid="clip-far-waveform"]').exists()).toBe(false);
    expect(mediaPeaks).toHaveBeenCalledTimes(1);
    expect(mediaPeaks).toHaveBeenCalledWith("ses-a", "snd-near", peakBucketsFor(8_000));
  });

  it("missing ffmpeg shows the install hint", async () => {
    const mediaPeaks = vi.fn(() =>
      Promise.reject(
        new EditorPortError({
          code: "encoderUnavailable",
          message: "Install ffmpeg to see waveforms.",
          retryable: false,
          operationId: "op-1",
        }),
      ),
    );
    const w = await mountTimeline({ mediaPeaks });

    const lane = w.find('[data-testid="clip-near-waveform"]');
    expect(lane.text()).toBe("Install ffmpeg to see waveforms");
    expect(lane.find("polyline").exists()).toBe(false);
  });

  // Any OTHER refusal is logged, never drawn as the ffmpeg hint (which
  // would send the user to install something they already have).
  it("another failure draws nothing and does not claim ffmpeg is missing", async () => {
    const mediaPeaks = vi.fn(() =>
      Promise.reject(
        new EditorPortError({ code: "sourceMissing", message: "gone", retryable: false, operationId: "op-2" }),
      ),
    );
    const w = await mountTimeline({ mediaPeaks });
    const lane = w.find('[data-testid="clip-near-waveform"]');
    expect(lane.exists()).toBe(true);
    expect(lane.text()).toBe("");
    expect(lane.find("polyline").exists()).toBe(false);
  });
});

// ---- the pure geometry ------------------------------------------------------

describe("waveformPoints", () => {
  // Asymmetric peaks over an 8-bucket asset; the clip uses source 2..6 s of
  // 8 s, i.e. buckets 2..5 — a clip reading from the wrong end would draw
  // the zeros instead.
  const peaks = [0, 0, 1, 0.5, 0.25, 1, 0, 0];

  it("draws the clip's own source range, top envelope then bottom", () => {
    const points = waveformPoints(peaks, 8_000, 2_000, 6_000, 40, 10);
    expect(points).toBe("5,0 15,2.5 25,3.8 35,0 35,10 25,6.3 15,7.5 5,10");
  });

  it("never emits more columns than pixels", () => {
    const many = Array.from({ length: 4_000 }, (_, i) => (i % 7) / 7);
    const points = waveformPoints(many, 200_000, 0, 200_000, 50, 12).split(" ");
    expect(points.length).toBe(100);
  });

  it("is empty when there is nothing to draw", () => {
    expect(waveformPoints([], 8_000, 0, 8_000, 40, 10)).toBe("");
    expect(waveformPoints(peaks, 8_000, 3_000, 3_000, 40, 10)).toBe("");
    expect(waveformPoints(peaks, 8_000, 0, 8_000, 0, 10)).toBe("");
    expect(waveformPoints(peaks, 0, 0, 0, 40, 10)).toBe("");
  });

  it("asks for a bounded bucket count", () => {
    expect(peakBucketsFor(8_000)).toBe(160);
    expect(peakBucketsFor(1)).toBe(1);
    expect(peakBucketsFor(0)).toBe(1);
    expect(peakBucketsFor(7_200_000)).toBe(4_000);
  });
});

// ---- the wire ---------------------------------------------------------------

describe("the derived-media IPC", () => {
  it("decodes { peaks } and refuses anything else", () => {
    expect(decodeMediaPeaks({ peaks: [0, 0.5, 1] })).toEqual([0, 0.5, 1]);
    for (const bad of [null, [0.5], { peaks: "x" }, { peaks: [1.5] }, { peaks: [-0.1] }, { peaks: ["0.5"] }]) {
      expect(() => decodeMediaPeaks(bad)).toThrow();
    }
  });

  it("sends each command's own parameter names", async () => {
    const PATH = "C:\\Users\\me\\AppData\\Local\\com.vaultbuddy.desktop\\editor-projects\\p1\\cache\\a1-500.jpg";
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_media_peaks") return { peaks: [0.25, 1] };
      if (cmd === "editor_media_thumbnail") return PATH;
      throw new Error(`unexpected command ${cmd}`);
    });
    const port = createTauriEditorPort();
    await expect(port.mediaPeaks("ses-1", "a1", 2)).resolves.toEqual([0.25, 1]);
    await expect(port.mediaThumbnail("ses-1", "a1", 740)).resolves.toBe(PATH);
    expect(calls).toEqual([
      { cmd: "editor_media_peaks", args: { sessionId: "ses-1", assetId: "a1", buckets: 2 } },
      { cmd: "editor_media_thumbnail", args: { sessionId: "ses-1", assetId: "a1", atMs: 740 } },
    ]);
  });
});

// ---- a lane whose asset changes, and a lane with no session -------------------

describe("ClipWaveform's own lifecycle", () => {
  function deferred<T>() {
    let resolve!: (v: T) => void;
    let reject!: (e: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  }

  const props = { assetDurationMs: 8_000, inMs: 0, outMs: 8_000, widthPx: 4 };

  // A slow answer for the asset the lane USED to show must not overwrite
  // the newer asset's waveform — nor, when it fails, blank it.
  it("a stale answer for a previous asset is ignored", async () => {
    const slow = deferred<number[]>();
    const slower = deferred<number[]>();
    const answers: Record<string, Promise<number[]>> = {
      old: slow.promise,
      older: slower.promise,
      new: Promise.resolve([1, 1, 1, 1]),
    };
    await openWith({ mediaPeaks: (_s, assetId) => answers[assetId] });
    const w = mount(ClipWaveform, { props: { ...props, assetId: "old" } });
    await w.setProps({ assetId: "new" });
    await flushPromises();
    const drawn = w.find("polyline").attributes("points");
    expect(drawn).toContain(",0 "); // full-height peaks

    slow.resolve([0, 0, 0, 0]);
    await flushPromises();
    expect(w.find("polyline").attributes("points")).toBe(drawn);

    await w.setProps({ assetId: "older" });
    await w.setProps({ assetId: "new" });
    slower.reject(new EditorPortError({ code: "encoderUnavailable", message: "x", retryable: false, operationId: "o" }));
    await flushPromises();
    expect(w.text()).toBe("");
    expect(w.find("polyline").attributes("points")).toBe(drawn);
  });

  it("asks nothing without an open session", async () => {
    const mediaPeaks = vi.fn(() => Promise.resolve([1]));
    useEditorProjectStore().setPort(fakeEditorPort({ mediaPeaks }));
    const w = mount(ClipWaveform, { props: { ...props, assetId: "snd-near" } });
    await flushPromises();
    expect(mediaPeaks).not.toHaveBeenCalled();
    expect(w.find("svg").exists()).toBe(false);
  });
});

// ---- poster frames -------------------------------------------------------------

describe("the timeline's poster frames", () => {
  const PATH = "C:\\data\\editor-projects\\project-a\\cache\\vid-0.jpg";
  // A 4 s video clip (200 px at the default zoom), a 0.5 s one (25 px, too
  // narrow for a poster) and a synthesized title card with no file at all.
  const withVideo: Partial<Project> = {
    assets: [asset("vid", "video", 9_000), { ...asset("card-1", "video", 3_000), builtin: "card" }],
    tracks: [track("v1", "video")],
    clips: [
      clip("wide", "vid", "v1", { start_ms: 0, in_ms: 1_000, out_ms: 5_000 }),
      clip("narrow", "vid", "v1", { start_ms: 5_000, in_ms: 6_000, out_ms: 6_500 }),
      clip("card", "card-1", "v1", { start_ms: 6_000, in_ms: 0, out_ms: 3_000 }),
    ],
  };

  it("shows one frame for a wide clip with a real file, through the asset protocol", async () => {
    mockConvertFileSrc("windows");
    const mediaThumbnail = vi.fn(() => Promise.resolve(PATH));
    const w = await mountTimeline({ mediaThumbnail }, withVideo);

    const img = w.find('[data-testid="clip-wide-thumbnail"] img');
    expect(img.exists()).toBe(true);
    expect(img.attributes("src")).toContain("asset.localhost");
    expect(mediaThumbnail).toHaveBeenCalledTimes(1);
    expect(mediaThumbnail).toHaveBeenCalledWith("ses-a", "vid", 1_000);
    expect(w.find('[data-testid="clip-narrow-thumbnail"]').exists()).toBe(false);
    expect(w.find('[data-testid="clip-card-thumbnail"]').exists()).toBe(false);
    expect(w.find('[data-testid="clip-wide-waveform"]').exists()).toBe(false);
  });

  // Review Minor 7: a synthesized audio builtin (no file) gets no waveform
  // lane, so it never asks Rust for peaks it must refuse on every mount.
  it("a file-less audio builtin asks for no waveform", async () => {
    const mediaPeaks = vi.fn(() => Promise.resolve([1]));
    const w = await mountTimeline(
      { mediaPeaks },
      {
        assets: [{ ...asset("amb", "audio", 4_000), builtin: "ambient" }],
        clips: [clip("ambience", "amb", "a1", { start_ms: 0, in_ms: 0, out_ms: 4_000 })],
      },
    );
    expect(w.find('[data-testid="clip-ambience"]').exists()).toBe(true);
    expect(w.find('[data-testid="clip-ambience-waveform"]').exists()).toBe(false);
    expect(mediaPeaks).not.toHaveBeenCalled();
  });

  // Fix round 1: a clip scrolled out of the virtualization window and back
  // asks Rust again (a hit there refreshes the LRU; an evicted file is
  // re-rendered) instead of reusing a path that may no longer exist.
  it("a remounted clip asks for its frame again", async () => {
    mockConvertFileSrc("windows");
    const mediaThumbnail = vi.fn(() => Promise.resolve(PATH));
    const first = await mountTimeline({ mediaThumbnail }, withVideo);
    first.unmount();
    const second = mount(TimelineView, { props: { viewportWidth: 400 } });
    await flushPromises();
    expect(second.find('[data-testid="clip-wide-thumbnail"] img').exists()).toBe(true);
    expect(mediaThumbnail).toHaveBeenCalledTimes(2);
  });

  // Review Minor 8: a late REJECTION for the frame the clip used to show
  // must not blank the newer frame.
  it("a stale refusal does not blank a newer frame", async () => {
    mockConvertFileSrc("windows");
    let rejectOld!: (e: unknown) => void;
    const mediaThumbnail = vi.fn((_s: string, _a: string, atMs: number) =>
      atMs === 1_000
        ? new Promise<string>((_res, rej) => {
          rejectOld = rej;
        })
        : Promise.resolve(PATH),
    );
    await openWith({ mediaThumbnail }, withVideo);
    const w = mount(ClipThumbnail, { props: { assetId: "vid", atMs: 1_000 } });
    await w.setProps({ atMs: 2_000 });
    await flushPromises();
    expect(w.find("img").exists()).toBe(true);
    rejectOld(new EditorPortError({ code: "sourceMissing", message: "x", retryable: false, operationId: "o" }));
    await flushPromises();
    expect(w.find("img").exists()).toBe(true);
  });

  it("draws no frame when Rust cannot make one", async () => {
    for (const code of ["encoderUnavailable", "unsupportedMedia"] as const) {
      clearMediaDerivedForTest();
      const mediaThumbnail = vi.fn(() =>
        Promise.reject(new EditorPortError({ code, message: code, retryable: false, operationId: "o" })),
      );
      const w = await mountTimeline({ mediaThumbnail }, withVideo);
      expect(w.find('[data-testid="clip-wide-thumbnail"]').exists()).toBe(true);
      expect(w.find('[data-testid="clip-wide-thumbnail"] img').exists()).toBe(false);
      w.unmount();
    }
  });
});

// ---- the webview's memo ------------------------------------------------------

describe("mediaDerived's memo", () => {
  // Task 28 fix round 1 (review Important 1): a thumbnail PATH must not be
  // memoized once settled. Rust refreshes a hit's mtime (its LRU key) only
  // when asked, and past 200 thumbnails it evicts the least recently used
  // — so a memoized path goes stale into a deleted file, and a remounted
  // clip would show nothing with no retry. In-flight requests still share.
  it("asks for a settled thumbnail again, sharing only an in-flight request", async () => {
    let n = 0;
    const mediaThumbnail = vi.fn(() => Promise.resolve(`C:/cache/t-${(n += 1)}.jpg`));
    const port = fakeEditorPort({ mediaThumbnail });
    const [a, b] = await Promise.all([loadThumbnail(port, "ses-a", "v", 0), loadThumbnail(port, "ses-a", "v", 0)]);
    expect(b).toBe(a);
    expect(mediaThumbnail).toHaveBeenCalledTimes(1);
    await loadThumbnail(port, "ses-a", "v", 0);
    expect(mediaThumbnail).toHaveBeenCalledTimes(2);
  });

  it("shares one request, forgets a refusal, and stays bounded", async () => {
    let fail = true;
    const mediaPeaks = vi.fn((_s: string, _a: string, buckets: number) =>
      fail ? Promise.reject(new Error("no ffmpeg yet")) : Promise.resolve([buckets / 1_000]),
    );
    const port = fakeEditorPort({ mediaPeaks });
    await expect(loadPeaks(port, "ses-a", "a1", 10)).rejects.toThrow("no ffmpeg yet");
    fail = false;
    const [first, second] = await Promise.all([loadPeaks(port, "ses-a", "a1", 10), loadPeaks(port, "ses-a", "a1", 10)]);
    expect(first).toEqual([0.01]);
    expect(second).toBe(first);
    expect(mediaPeaks).toHaveBeenCalledTimes(2); // the refusal, then ONE retry

    for (let b = 1; b <= 256; b += 1) await loadPeaks(port, "ses-a", "a2", b);
    const before = mediaPeaks.mock.calls.length;
    await loadPeaks(port, "ses-a", "a1", 10); // the oldest entry was evicted
    expect(mediaPeaks.mock.calls.length).toBe(before + 1);
  });
});
