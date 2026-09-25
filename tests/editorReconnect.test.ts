/**
 * Reconnecting missing originals (Task 40; F-03; A19; SCREENS 07). Rust
 * decides every match (`core::editor::relink`); these tests pin what the
 * webview does with the verdict: the wire shape, the dialog's per-asset
 * results — above all that an AMBIGUOUS asset is never shown as reconnected
 * and its only way forward is an explicit single-file choice — the
 * confirmed-replacement path, and that the preview asks for a relinked
 * asset's media again instead of remembering the failed lookup (Task 22's
 * deferred minor).
 */
import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ReconnectDialog from "../src/components/editor/dialogs/ReconnectDialog.vue";
import MediaLibrary from "../src/components/editor/library/MediaLibrary.vue";
import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import ClipThumbnail from "../src/components/editor/timeline/ClipThumbnail.vue";
import ClipWaveform from "../src/components/editor/timeline/ClipWaveform.vue";
import { useMediaReconnect } from "../src/composables/useMediaReconnect";
import { decodeRelinkReport } from "../src/editor/decodeRelink";
import { clearMediaDerivedForTest, mediaVersion } from "../src/editor/mediaDerived";
import { createTauriEditorPort, EditorPortError } from "../src/editor/port";
import type { AudioContextLike } from "../src/editor/previewController";
import { PreviewController } from "../src/editor/previewController";
import type { EditorOpenResult, MissingMedia, Project, RelinkReport } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  clearMediaDerivedForTest();
});

afterEach(() => {
  clearMocks();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

const TALK: MissingMedia = { assetId: "a-talk", name: "talk.mp4", expectedSize: 7_340_000, expectedDurationMs: 62_500 };
const LOGO: MissingMedia = { assetId: "a-logo", name: "logo.png", expectedSize: 48_000, expectedDurationMs: 5_000 };

function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [
      { id: "a-talk", kind: "video", name: "talk.mp4", duration_ms: 62_500 },
      { id: "a-logo", kind: "video", name: "logo.png", duration_ms: 5_000, media_type: "image" },
    ],
    tracks: [{ id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips: [
      {
        id: "c1", asset_id: "a-talk", track_id: "v1", name: "Talk", start_ms: 0, in_ms: 1_000, out_ms: 9_000,
        fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1,
      },
    ],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "", folder: "", dated: false },
  };
}

function snapshot(revision: number) {
  return {
    sessionId: "ses-a", projectId: "project-a", revision, persistedRevision: 4, title: "Tutorial",
    durationMs: 8_000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  };
}

function opened(missing: MissingMedia[]): EditorOpenResult {
  return { snapshot: snapshot(4), project: project(), workspace: {}, missing, sourceBase: null, recovered: false };
}

function report(overrides: Partial<RelinkReport> = {}): RelinkReport {
  return {
    projection: { snapshot: snapshot(4), project: project() },
    missing: [TALK, LOGO],
    matched: [],
    replaced: [],
    ambiguous: [],
    unmatched: [],
    mismatched: [],
    failed: [],
    unused: [],
    excluded: [],
    perFile: [],
    ...overrides,
  };
}

type Relink = (sessionId: string, assetIds: string[], confirmReplace: boolean) => Promise<RelinkReport | null>;

async function openStore(relinkMedia: Relink, missing: MissingMedia[] = [TALK, LOGO], extra = {}) {
  const store = useEditorProjectStore();
  store.setPort(fakeEditorPort({ openStaged: () => Promise.resolve(opened(missing)), relinkMedia, ...extra }));
  await store.openStaged("base");
  return store;
}

function mountDialog() {
  return mount(ReconnectDialog, { props: { open: true }, attachTo: document.body });
}

const row = (w: ReturnType<typeof mount>, id: string) => w.get(`[data-testid="reconnect-row-${id}"]`);

describe("the relink wire", () => {
  it("decodes a report literally and refuses a malformed one", () => {
    const raw = {
      projection: { snapshot: snapshot(5), project: project() },
      missing: [{ assetId: "a-logo", name: "logo.png", expectedSize: 48_000, expectedDurationMs: 5_000 }],
      matched: [{ assetId: "a-talk", file: "talk (backup).mp4" }],
      replaced: [],
      ambiguous: [{ assetId: "a-x", files: ["x.mp4", "x (1).mp4"] }],
      unmatched: ["a-y"],
      mismatched: [{ assetId: "a-z", file: "z.mp4", reason: "different duration: 12.4 s vs 31.0 s" }],
      failed: [{ assetId: "a-f", file: "f.mp4", error: "disk full" }],
      unused: ["stray.mp4"],
      excluded: [{ assetId: "a-c", reason: "kept with your captures" }],
      perFile: [{ name: "broken.mp4", error: "damaged" }],
    };
    const decoded = decodeRelinkReport(raw);
    expect(decoded?.projection.snapshot.revision).toBe(5);
    expect(decoded?.matched).toEqual([{ assetId: "a-talk", file: "talk (backup).mp4" }]);
    expect(decoded?.ambiguous[0].files).toEqual(["x.mp4", "x (1).mp4"]);
    expect(decoded?.mismatched[0].reason).toBe("different duration: 12.4 s vs 31.0 s");
    expect(decoded?.perFile).toEqual([{ name: "broken.mp4", error: "damaged" }]);
    expect(decoded?.failed).toEqual([{ assetId: "a-f", file: "f.mp4", error: "disk full" }]);
    expect(decoded?.unused).toEqual(["stray.mp4"]);
    expect(decoded?.excluded).toEqual([{ assetId: "a-c", reason: "kept with your captures" }]);
    const { excluded: _gone, ...withoutExcluded } = raw;
    expect(() => decodeRelinkReport(withoutExcluded)).toThrow();
    expect(decodeRelinkReport(null)).toBeNull();
    expect(() => decodeRelinkReport({ ...raw, ambiguous: [{ assetId: "a-x", files: "x.mp4" }] })).toThrow();
    const { perFile: _omitted, ...withoutPerFile } = raw;
    expect(() => decodeRelinkReport(withoutPerFile)).toThrow();
  });

  it("sends sessionId, assetIds and confirmReplace to editor_relink_media", async () => {
    let captured: unknown = null;
    mockIPC((cmd, args) => {
      captured = { cmd, args };
      return null;
    });
    const result = await createTauriEditorPort().relinkMedia("ses-a", ["a-talk"], true);
    expect(result).toBeNull();
    expect(captured).toEqual({
      cmd: "editor_relink_media",
      args: { sessionId: "ses-a", assetIds: ["a-talk"], confirmReplace: true },
    });
  });
});

describe("ReconnectDialog", () => {
  it("shows what each missing original was", async () => {
    await openStore(vi.fn());
    const w = mountDialog();
    const talk = row(w, "a-talk").text();
    expect(talk).toContain("talk.mp4");
    expect(talk).toContain("7 MB");
    expect(talk).toContain("1:02");
    expect(row(w, "a-logo").text()).toContain("47 KB");
  });

  // Named case (A19). Two picked files match the talk equally well: the
  // dialog says so, names both, and does NOT show the talk as reconnected —
  // it stays in the store's missing list. The only way forward is an
  // explicit choice: ONE asset, no replacement confirmation.
  it("ambiguous assets require an explicit choice", async () => {
    const relink = vi.fn<Relink>().mockResolvedValueOnce(
      report({ ambiguous: [{ assetId: "a-talk", files: ["talk.mp4", "talk (1).mp4"] }], unmatched: ["a-logo"] }),
    );
    const store = await openStore(relink);
    const w = mountDialog();

    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();

    expect(relink).toHaveBeenCalledWith("ses-a", ["a-talk", "a-logo"], false);
    const talk = row(w, "a-talk");
    expect(talk.text()).toContain("talk.mp4");
    expect(talk.text()).toContain("talk (1).mp4");
    expect(talk.text()).toMatch(/choose/i);
    expect(talk.text()).not.toMatch(/reconnected/i);
    expect(talk.find('[data-testid="reconnect-replace"]').exists()).toBe(false);
    expect(store.missing.map((m) => m.assetId)).toContain("a-talk");

    relink.mockResolvedValueOnce(report({ matched: [{ assetId: "a-talk", file: "talk (1).mp4" }], missing: [LOGO] }));
    await talk.get('[data-testid="reconnect-choose"]').trigger("click");
    await flushPromises();

    expect(relink).toHaveBeenLastCalledWith("ses-a", ["a-talk"], false);
    expect(row(w, "a-talk").text()).toMatch(/reconnected to “talk \(1\)\.mp4”/i);
    expect(store.missing.map((m) => m.assetId)).toEqual(["a-logo"]);
  });

  it("a mismatched file is explained, and replacing it asks for the file again with confirmation", async () => {
    const relink = vi.fn<Relink>().mockResolvedValueOnce(
      report({ mismatched: [{ assetId: "a-talk", file: "talk-short.mp4", reason: "different duration: 12.4 s vs 62.5 s" }] }),
    );
    await openStore(relink);
    const w = mountDialog();
    await row(w, "a-talk").get('[data-testid="reconnect-choose"]').trigger("click");
    await flushPromises();
    expect(relink).toHaveBeenLastCalledWith("ses-a", ["a-talk"], false);
    expect(row(w, "a-talk").text()).toContain("different duration: 12.4 s vs 62.5 s");

    relink.mockResolvedValueOnce(report({ replaced: [{ assetId: "a-talk", file: "talk-long.mp4" }], missing: [LOGO] }));
    await row(w, "a-talk").get('[data-testid="reconnect-replace"]').trigger("click");
    await flushPromises();
    expect(relink).toHaveBeenLastCalledWith("ses-a", ["a-talk"], true);
    expect(row(w, "a-talk").text()).toMatch(/replaced with “talk-long\.mp4”/i);
  });

  it("a refusal and a dismissed dialog change nothing and say so", async () => {
    const relink = vi
      .fn<Relink>()
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(
        new EditorPortError({ code: "invalidRequest", message: "“talk.mp4” is not missing.", retryable: false, operationId: "op" }),
      );
    const store = await openStore(relink);
    const w = mountDialog();
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="reconnect-status"]').text()).toMatch(/nothing was reconnected/i);
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="reconnect-status"]').text()).toContain("“talk.mp4” is not missing.");
    expect(store.missing).toHaveLength(2);
  });

  it("installs the reconnect's projection and names files it could not read", async () => {
    const relink = vi.fn<Relink>().mockResolvedValueOnce(
      report({
        projection: { snapshot: snapshot(5), project: project() },
        matched: [{ assetId: "a-talk", file: "talk.mp4" }],
        missing: [LOGO],
        perFile: [{ name: "broken.mp4", error: "ffprobe could not read the file." }],
      }),
    );
    const store = await openStore(relink);
    const w = mountDialog();
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    expect(store.snapshot?.revision).toBe(5);
    expect(store.missing).toEqual([LOGO]);
    expect(w.text()).toContain("broken.mp4");
    expect(w.text()).toContain("ffprobe could not read the file.");
  });
});

describe("ReconnectDialog, fix round 1", () => {
  // Review Minor 3: an original Rust left out of a batch (a staged capture,
  // one only a snapshot uses) says why, offers no file choice, and is not
  // sent again by the next "Find all".
  it("a left-out original says why and is not asked for again", async () => {
    const relink = vi
      .fn<Relink>()
      .mockResolvedValueOnce(report({ excluded: [{ assetId: "a-logo", reason: "“logo.png” is kept with your captures." }], unmatched: ["a-talk"] }))
      .mockResolvedValueOnce(report({ unmatched: ["a-talk"] }));
    await openStore(relink);
    const w = mountDialog();
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    const logo = row(w, "a-logo");
    expect(logo.text()).toContain("“logo.png” is kept with your captures.");
    expect(logo.find('[data-testid="reconnect-choose"]').exists()).toBe(false);
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    expect(relink).toHaveBeenLastCalledWith("ses-a", ["a-talk"], false);
  });

  // Review Minor 6/5: a failed copy reads as its cause, and a picked file
  // nothing claimed is named rather than silently ignored.
  it("a failed copy reads as its cause, and unused files are named", async () => {
    const relink = vi.fn<Relink>().mockResolvedValueOnce(
      report({
        failed: [{ assetId: "a-talk", file: "talk.mp4", error: "Not enough disk space to import the file." }],
        unused: ["holiday.mp4"],
        unmatched: ["a-logo"],
      }),
    );
    await openStore(relink);
    const w = mountDialog();
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    const talk = row(w, "a-talk").text();
    expect(talk).toContain("Not enough disk space to import the file.");
    expect(talk).not.toMatch(/none of the chosen files/i);
    expect(w.text()).toContain("holiday.mp4");
  });

  it("one file that fits two originals is shown as such", async () => {
    const relink = vi.fn<Relink>().mockResolvedValueOnce(
      report({ ambiguous: [{ assetId: "a-talk", files: ["take.mp4"] }, { assetId: "a-logo", files: ["take.mp4"] }] }),
    );
    await openStore(relink);
    const w = mountDialog();
    await w.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();
    expect(row(w, "a-talk").text()).toMatch(/“take\.mp4” fits more than one missing original/);
    expect(row(w, "a-talk").text()).not.toMatch(/reconnected/i);
  });
});

// Review Important 1: the timeline's derived media must follow a reconnect.
describe("the timeline after a reconnect", () => {
  const replacedTalk = () =>
    report({
      projection: { snapshot: snapshot(5), project: project() },
      replaced: [{ assetId: "a-talk", file: "talk-long.mp4" }],
      missing: [LOGO],
    });

  it("a mounted waveform asks again and draws the replacement's peaks, and only its own", async () => {
    const mediaPeaks = vi.fn((_s: string, assetId: string) =>
      Promise.resolve(assetId === "a-talk" && mediaPeaks.mock.calls.length > 2 ? [1, 1, 1, 1] : [0.1, 0.1, 0.1, 0.1]),
    );
    await openStore(vi.fn<Relink>().mockResolvedValue(replacedTalk()), [TALK, LOGO], { mediaPeaks });
    const lane = { assetDurationMs: 62_500, inMs: 0, outMs: 62_500, widthPx: 4 };
    const talk = mount(ClipWaveform, { props: { ...lane, assetId: "a-talk" } });
    mount(ClipWaveform, { props: { ...lane, assetId: "a-logo" } });
    await flushPromises();
    const before = talk.get("polyline").attributes("points");
    expect(mediaPeaks).toHaveBeenCalledTimes(2);

    await useMediaReconnect().run(["a-talk"], true);
    await flushPromises();

    expect(mediaPeaks).toHaveBeenCalledTimes(3);
    expect(mediaPeaks.mock.calls[2][1]).toBe("a-talk");
    expect(talk.get("polyline").attributes("points")).not.toBe(before);
  });

  // A detached-audio asset plays its video's sound: its waveform is stale
  // too, although its own id was never reconnected.
  it("a detached audio asset's waveform is forgotten with its video's", async () => {
    const withAudio: Project = {
      ...project(),
      assets: [...project().assets, { id: "a-talk-audio", kind: "audio", name: "talk audio", duration_ms: 62_500, linked_asset: "a-talk" }],
    };
    await openStore(vi.fn<Relink>().mockResolvedValue({ ...replacedTalk(), projection: { snapshot: snapshot(5), project: withAudio } }));
    const before = { talk: mediaVersion("a-talk"), audio: mediaVersion("a-talk-audio"), logo: mediaVersion("a-logo") };
    await useMediaReconnect().run(["a-talk"], true);
    expect(mediaVersion("a-talk")).toBe(before.talk + 1);
    expect(mediaVersion("a-talk-audio")).toBe(before.audio + 1);
    expect(mediaVersion("a-logo")).toBe(before.logo);
  });

  it("a clip whose thumbnail failed while its file was missing asks again", async () => {
    mockConvertFileSrc("windows");
    let found = false;
    const mediaThumbnail = vi.fn(() =>
      found
        ? Promise.resolve("C:\\cache\\a-talk-1000.jpg")
        : Promise.reject(new EditorPortError({ code: "sourceMissing", message: "gone", retryable: false, operationId: "op" })),
    );
    await openStore(vi.fn<Relink>().mockResolvedValue(replacedTalk()), [TALK, LOGO], { mediaThumbnail });
    const w = mount(ClipThumbnail, { props: { assetId: "a-talk", atMs: 1_000 } });
    await flushPromises();
    expect(w.find("img").exists()).toBe(false);

    found = true;
    await useMediaReconnect().run(["a-talk"], true);
    await flushPromises();

    expect(mediaThumbnail).toHaveBeenCalledTimes(2);
    expect(w.find("img").attributes("src")).toContain("a-talk-1000.jpg");
  });
});

describe("the media library", () => {
  it("offers Reconnect only while an original is missing, and opens the dialog", async () => {
    const store = await openStore(vi.fn(), [TALK], { getJobs: () => Promise.resolve([]) });
    const w = mount(MediaLibrary, { attachTo: document.body });
    await flushPromises();
    expect(w.find('[data-testid="reconnect-dialog"]').exists()).toBe(false);
    await w.get('[data-testid="library-reconnect"]').trigger("click");
    await flushPromises();
    expect(document.body.querySelector('[data-testid="reconnect-row-a-talk"]')).not.toBeNull();
    store.$patch({ missing: [] });
    await flushPromises();
    expect(w.find('[data-testid="library-reconnect"]').exists()).toBe(false);
  });
});

describe("the preview after a reconnect", () => {
  it("a forgotten asset is resolved again instead of staying blank", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const resolveUrl = vi.fn<(id: string) => Promise<string | null>>().mockResolvedValue(null);
    const c = new PreviewController({
      container, resolveUrl, createAudioContext: () => null, requestFrame: () => 1, cancelFrame: () => undefined,
    });
    c.setStage({ width: 1000, height: 500 });
    c.layout(project(), 2_000);
    await flushPromises();
    c.layout(project(), 2_100);
    await flushPromises();
    expect(resolveUrl).toHaveBeenCalledTimes(1);
    const video = container.querySelector("video");
    expect(video?.getAttribute("src")).toBeNull();

    resolveUrl.mockResolvedValue("asset://talk");
    c.forgetMedia(["a-talk"]);
    await flushPromises();
    expect(resolveUrl).toHaveBeenCalledTimes(2);
    expect(container.querySelector("video")?.getAttribute("src")).toBe("asset://talk");
    c.destroy();
  });

  it("the preview surface forgets exactly the assets that stopped being missing", async () => {
    mockConvertFileSrc("windows");
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1000);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(500);
    let found = false;
    const mediaUrl = vi.fn(() =>
      found
        ? Promise.resolve("C:\\media\\a-talk.mp4")
        : Promise.reject(new EditorPortError({ code: "sourceMissing", message: "gone", retryable: false, operationId: "op" })),
    );
    const relink = vi.fn<Relink>().mockResolvedValue(
      report({ projection: { snapshot: snapshot(5), project: project() }, matched: [{ assetId: "a-talk", file: "t.mp4" }], missing: [LOGO] }),
    );
    const store = await openStore(relink, [TALK, LOGO], { mediaUrl });
    useEditorWorkspaceStore().setPlayhead(2_000);
    const silent = (): AudioContextLike | null => null;
    const w = mount(PreviewSurface, { props: { createAudioContext: silent }, attachTo: document.body });
    await flushPromises();
    expect(mediaUrl).toHaveBeenCalledTimes(1);
    expect(w.text()).toContain("talk.mp4");

    found = true;
    const dialog = mountDialog();
    await dialog.get('[data-testid="reconnect-find-all"]').trigger("click");
    await flushPromises();

    expect(store.missing).toEqual([LOGO]);
    expect(mediaUrl).toHaveBeenCalledTimes(2);
    expect(w.get("video").attributes("src")).toContain("a-talk.mp4");
    expect(w.text()).not.toContain("talk.mp4");
  });
});
