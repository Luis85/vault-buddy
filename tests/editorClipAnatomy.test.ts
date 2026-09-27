/**
 * Visual-parity Task 18 (concept spec §6.5): a clip's anatomy on the
 * timeline — the 47px body at top 5, the repeating filmstrip, the name,
 * the duration pill, the badges, the gold fade shape and handles, the
 * trim handles, the selection outline and the audio clip's bar waveform —
 * plus the lanes' "Close gap" hints, the playhead with its head and the
 * dashed snap guide a drag shows. Class-level checks here (happy-dom has
 * no layout); `tests/e2e/editorParity.spec.ts` measures the real boxes.
 */
import { clearMocks, mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import { clearMediaDerivedForTest } from "../src/editor/mediaDerived";
import { type EditorPort,EditorPortError } from "../src/editor/port";
import type { Asset, Clip, EditorOpenResult, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

let executed: unknown[] = [];

beforeEach(() => {
  setActivePinia(createPinia());
  useEditorWorkspaceStore().setViewport(1600, 1000);
  clearMediaDerivedForTest();
  mockConvertFileSrc("windows");
  executed = [];
});
afterEach(() => clearMocks());

function track(id: string, kind: "video" | "audio", overrides: Partial<Track> = {}): Track {
  return { id, kind, name: `${id} track`, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}

function asset(id: string, kind: "video" | "audio", overrides: Partial<Asset> = {}): Asset {
  return { id, kind, name: id, duration_ms: 60_000, ...overrides };
}

function clip(id: string, assetId: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: assetId,
    track_id: trackId,
    name: `${id} name`,
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

// At zoom 1 (50 px/s): `wide` is 9.5 s = 475 px, `short` 2 s = 100 px and
// `tiny` 60 ms = 3 px. `sound` is an audio clip; v2 holds one clip at 5 s,
// so it has a 250 px gap before it, and v1 a 1 s (50 px) gap after `wide`.
function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [asset("vid", "video"), asset("snd", "audio"), asset("still", "video", { media_type: "image" })],
    tracks: [track("v2", "video"), track("v1", "video"), track("a1", "audio")],
    clips: [
      clip("wide", "vid", "v1", { start_ms: 0, in_ms: 1_000, out_ms: 10_500, fade_in_ms: 1_900, fade_out_ms: 950 }),
      clip("short", "vid", "v1", { start_ms: 10_500, in_ms: 0, out_ms: 2_000, speed: 2 }),
      clip("tiny", "vid", "v1", { start_ms: 15_000, in_ms: 0, out_ms: 60 }),
      clip("late", "still", "v2", { start_ms: 5_000, in_ms: 0, out_ms: 4_000, group_id: "g1" }),
      clip("sound", "snd", "a1", { start_ms: 0, in_ms: 0, out_ms: 8_000 }),
    ],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

async function openWith(ports: Partial<EditorPort> = {}, overrides: Partial<Project> = {}, durationMs = 20_000) {
  const store = useEditorProjectStore();
  const p = project(overrides);
  const snapshot = {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 1,
    persistedRevision: null,
    title: "Tutorial",
    durationMs,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
  };
  store.setPort(
    fakeEditorPort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({ snapshot, project: p, workspace: {}, missing: [], sourceBase: "base", recovered: false }),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: p });
      },
      mediaThumbnail: () => Promise.resolve("C:\\cache\\vid-1000.jpg"),
      mediaPeaks: (_s, _a, buckets) => Promise.resolve(Array.from({ length: buckets }, (_, i) => (i % 5) / 5)),
      ...ports,
    }),
  );
  await store.openStaged("base");
}

async function mountTimeline(
  ports: Partial<EditorPort> = {},
  overrides: Partial<Project> = {},
  viewportWidth = 1400,
  durationMs = 20_000,
) {
  await openWith(ports, overrides, durationMs);
  const w = mount(TimelineView, { props: { viewportWidth }, attachTo: document.body });
  await flushPromises();
  return w;
}

const byId = (w: Awaited<ReturnType<typeof mountTimeline>>, id: string) => w.get(`[data-testid="${id}"]`);

// ---- the body ------------------------------------------------------------------

describe("a clip's body (§6.5)", () => {
  it("is 47px tall at top 5 with radius 5, and takes its kind's background and border", async () => {
    const w = await mountTimeline();
    const video = byId(w, "clip-wide");
    expect(video.classes()).toEqual(
      expect.arrayContaining(["top-[5px]", "h-[47px]", "rounded-[5px]", "border", "bg-video-bg", "border-clip-video-edge"]),
    );
    const audio = byId(w, "clip-sound");
    expect(audio.classes()).toEqual(expect.arrayContaining(["bg-audio-bg", "border-clip-audio-edge", "text-audio"]));
    // The content clips to the rounded body; the fade handles ride above it.
    expect(byId(w, "clip-wide-content").classes()).toEqual(expect.arrayContaining(["overflow-hidden", "rounded-[4px]"]));
    expect(video.classes()).not.toContain("overflow-hidden");
  });

  it("a selected clip has a 2px accent outline (never a box-shadow ring)", async () => {
    const w = await mountTimeline();
    useEditorWorkspaceStore().select(["wide"]);
    await flushPromises();
    expect(byId(w, "clip-wide").classes()).toEqual(expect.arrayContaining(["outline-2", "outline-accent", "outline-offset-0"]));
    expect(byId(w, "clip-wide").classes()).not.toContain("ring-2");
    expect(byId(w, "clip-short").classes()).not.toContain("outline-accent");
  });
});

// ---- the filmstrip, the name, the pill and the badges -------------------------

describe("a video clip's picture and text", () => {
  it("repeats ONE thumbnail per clip across the body at 32 % as a filmstrip", async () => {
    const mediaThumbnail = vi.fn(() => Promise.resolve("C:\\cache\\vid-1000.jpg"));
    const w = await mountTimeline({ mediaThumbnail });
    const film = byId(w, "clip-wide-film");
    expect(film.classes()).toEqual(expect.arrayContaining(["absolute", "inset-0", "opacity-32", "saturate-70"]));
    const style = film.attributes("style") ?? "";
    expect(style).toContain("asset.localhost");
    expect(style).toContain("background-size: auto 47px");
    expect(style).toContain("repeat-x");
    // wide and short are wide enough for a poster; tiny is not. One request
    // each, at the clip's own first source instant.
    expect(mediaThumbnail).toHaveBeenCalledTimes(3);
    expect(mediaThumbnail).toHaveBeenCalledWith("ses-a", "vid", 1_000);
    expect(w.find('[data-testid="clip-tiny-film"]').exists()).toBe(false);
  });

  it("names the clip bottom-left with a 10px glyph", async () => {
    const w = await mountTimeline();
    const name = byId(w, "clip-wide-name");
    expect(name.classes()).toEqual(expect.arrayContaining(["left-[9px]", "right-[6px]", "bottom-[5px]", "text-[10px]", "truncate", "text-fg"]));
    expect(name.text()).toBe("wide name");
    const glyph = name.get("svg");
    expect([glyph.attributes("width"), glyph.attributes("height")]).toEqual(["10", "10"]);
  });

  it("shows the duration pill only on a video clip wider than 140px", async () => {
    const w = await mountTimeline();
    expect(byId(w, "clip-wide-duration").text()).toBe("9.5s");
    expect(byId(w, "clip-wide-duration").classes()).toEqual(
      expect.arrayContaining(["right-[7px]", "top-1", "text-[8px]", "bg-clip-pill", "text-clip-pill-ink"]),
    );
    expect(w.find('[data-testid="clip-short-duration"]').exists()).toBe(false); // 50 px at 2×
    expect(w.find('[data-testid="clip-sound-duration"]').exists()).toBe(false); // audio
  });

  it("badges a speed that is not 1, a grouped clip and a still", async () => {
    const w = await mountTimeline({}, {}, 1400);
    // short plays at 2× but is 1 s = 50 px wide: too narrow for badges.
    expect(w.find('[data-testid="clip-short-badges"]').exists()).toBe(false);
    const late = byId(w, "clip-late-badges");
    expect(late.text()).toContain("STILL");
    expect(late.find("svg").exists()).toBe(true); // the link glyph
    expect(late.classes()).toEqual(expect.arrayContaining(["left-2", "top-[3px]", "text-[8px]", "bg-clip-badge"]));
    expect(w.find('[data-testid="clip-wide-badges"]').exists()).toBe(false);
  });

  it("badges 2× on a sped-up clip wide enough to hold it", async () => {
    const w = await mountTimeline({}, {
      clips: [clip("fast", "vid", "v1", { start_ms: 0, in_ms: 0, out_ms: 6_000, speed: 2 })],
    });
    expect(byId(w, "clip-fast-badges").text()).toBe("2×");
  });
});

// ---- fades -----------------------------------------------------------------------

describe("the fade shape and handles", () => {
  it("draws gold triangles and a diagonal for each nonzero fade, over a 100×47 box", async () => {
    const w = await mountTimeline();
    const shape = byId(w, "clip-wide-fade-shape");
    expect(shape.attributes("viewBox")).toBe("0 0 100 47");
    expect(shape.attributes("preserveAspectRatio")).toBe("none");
    // wide: 9.5 s, fade in 1.9 s = 20 %, fade out 0.95 s = 10 %.
    const fadeIn = byId(w, "clip-wide-fade-in");
    const [fill, stroke] = fadeIn.findAll("path");
    expect(fill.attributes("d")).toBe("M0 47L20 2V47Z");
    expect(fill.attributes("fill")).toBe("var(--color-gold)");
    expect(fill.attributes("opacity")).toBe(".12");
    expect(stroke.attributes("d")).toBe("M0 47L20 2");
    expect(stroke.attributes("stroke")).toBe("var(--color-gold)");
    expect(stroke.attributes("stroke-width")).toBe("1.2");
    expect(byId(w, "clip-wide-fade-out").findAll("path")[0].attributes("d")).toBe("M90 2L100 47H90Z");
    // No fade, no shape.
    expect(w.find('[data-testid="clip-short-fade-shape"]').exists()).toBe(false);
  });

  it("puts 13px gold circles on the top edge at the knees, shown on hover or selection, keeping vb-handle", async () => {
    const w = await mountTimeline();
    const handle = byId(w, "clip-wide-fade-in-handle");
    expect(handle.classes()).toEqual(
      expect.arrayContaining(["vb-handle", "-top-1", "h-[13px]", "w-[13px]", "rounded-full", "bg-gold", "border-2", "border-panel", "-translate-x-1/2", "opacity-0", "group-hover:opacity-100"]),
    );
    expect(handle.attributes("style")).toContain("left: 20%");
    expect(byId(w, "clip-wide-fade-out-handle").attributes("style")).toContain("left: 90%");
    expect(handle.attributes("title")).toBe("Fade in: 1.9s. Drag, or use Fades properties.");
    useEditorWorkspaceStore().select(["wide"]);
    await flushPromises();
    expect(byId(w, "clip-wide-fade-in-handle").classes()).toContain("opacity-100");
  });

  it("trim handles are 9×33 at top 10 with a 2px ink bar that keeps vb-handle", async () => {
    const w = await mountTimeline();
    const start = byId(w, "clip-wide-trim-start");
    expect(start.classes()).toEqual(expect.arrayContaining(["top-[10px]", "h-[33px]", "w-[9px]", "left-0", "cursor-ew-resize", "opacity-0", "group-hover:opacity-100"]));
    expect(start.classes()).not.toContain("vb-handle");
    const bar = byId(w, "clip-wide-trim-start-bar");
    expect(bar.classes()).toEqual(expect.arrayContaining(["vb-handle", "w-[2px]", "bg-fg", "left-[2px]", "top-1.5", "bottom-1.5"]));
    expect(byId(w, "clip-wide-trim-end").classes()).toContain("right-0");
    expect(byId(w, "clip-wide-trim-end-bar").classes()).toContain("right-[2px]");
  });
});

// ---- audio ---------------------------------------------------------------------

describe("an audio clip (§6.5)", () => {
  it("names itself at the top in the audio colour and draws at most 130 rounded bars", async () => {
    const w = await mountTimeline();
    const name = byId(w, "clip-sound-name");
    expect(name.classes()).toEqual(expect.arrayContaining(["top-[3px]", "text-audio"]));
    expect(name.classes()).not.toContain("bottom-[5px]");
    const svg = byId(w, "clip-sound-waveform").get("svg");
    expect(svg.classes()).toEqual(expect.arrayContaining(["top-[18px]", "h-[27px]", "opacity-65"]));
    const bars = svg.get("path");
    expect(bars.attributes("stroke-linecap")).toBe("round");
    expect(bars.attributes("stroke-width")).toBe("1.5");
    const count = (bars.attributes("d") ?? "").split("M").length - 1;
    // 8 s = 400 px: one bar per ~5 px, capped at 130.
    expect(count).toBe(80);
  });

  it("says the waveform is unavailable and the audio still plays when there are no peaks", async () => {
    const mediaPeaks = vi.fn(() =>
      Promise.reject(new EditorPortError({ code: "encoderUnavailable", message: "x", retryable: false, operationId: "o" })),
    );
    const w = await mountTimeline({ mediaPeaks });
    const lane = byId(w, "clip-sound-waveform");
    expect(lane.text()).toBe("waveform unavailable · audio still plays");
    expect(lane.find("path").exists()).toBe(false);
  });
});

// ---- gap hints -------------------------------------------------------------------

describe("the Close gap hint", () => {
  it("shows a dashed Close gap in a gap wider than 70px, faint until the lane is hovered", async () => {
    const w = await mountTimeline();
    const hint = byId(w, "lane-gap-v2-0");
    expect(hint.text()).toBe("Close gap");
    expect(hint.classes()).toEqual(
      expect.arrayContaining(["border-dashed", "border-line", "bg-panel", "top-4", "h-[25px]", "text-[9px]", "opacity-16", "group-hover/lane:opacity-100", "group-focus-within/lane:opacity-100", "motion-reduce:transition-none"]),
    );
    // 0..5 s = 0..250 px, inset 4 px each side.
    expect(hint.attributes("style")).toContain("left: 4px");
    expect(hint.attributes("style")).toContain("width: 242px");
    expect(hint.attributes("aria-label")).toBe("Close 5.0 second gap on v2 track");
    expect(byId(w, "track-lane-body-v2").classes()).toContain("group/lane");
    // v1's gaps: 9.5..10.5 s (50 px) does not qualify, 11.5..15 s (175 px) does.
    expect(w.findAll('[data-testid^="lane-gap-v1-"]')).toHaveLength(1);
  });

  it("closes that gap through the lane menu's own moveClips", async () => {
    const w = await mountTimeline();
    await byId(w, "lane-gap-v2-0").trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["late"], deltaMs: -5_000, trackId: null }]);
  });

  it("is not offered on a locked track, or where the gap is 70px or less", async () => {
    const w = await mountTimeline({}, {
      tracks: [track("v2", "video", { locked: true }), track("v1", "video"), track("a1", "audio")],
      clips: [
        clip("late", "vid", "v2", { start_ms: 5_000, out_ms: 1_000 }),
        clip("near", "vid", "v1", { start_ms: 1_400, out_ms: 1_000 }),
      ],
    });
    expect(w.find('[data-testid^="lane-gap-v2-"]').exists()).toBe(false);
    expect(w.find('[data-testid^="lane-gap-v1-"]').exists()).toBe(false); // 70 px exactly
  });
});

// ---- the playhead and the snap guide ------------------------------------------------

describe("the playhead (§6.5)", () => {
  it("is a 1px accent line below the ruler, under the pinned label cells, with a pentagon head in the ruler", async () => {
    const w = await mountTimeline();
    const line = byId(w, "timeline-playhead");
    expect(line.classes()).toEqual(expect.arrayContaining(["vb-playhead", "w-px", "bg-accent", "top-8", "bottom-0", "z-[12]", "pointer-events-none"]));
    // Under the sticky label cells (z-15) and the ruler row (z-20).
    expect(byId(w, "track-lane-header-v1").classes()).toContain("z-[15]");
    const head = byId(w, "timeline-playhead-head");
    expect(head.classes()).toEqual(expect.arrayContaining(["vb-playhead", "top-[22px]", "h-[10px]", "w-[11px]", "bg-accent", "z-[1]"]));
    expect(head.classes()).toContain("[clip-path:polygon(0_0,100%_0,100%_60%,50%_100%,0_60%)]");
    // The head sits in the ruler's own layer, below its pinned label cell.
    expect(byId(w, "timeline-ruler").find('[data-testid="timeline-playhead-head"]').exists()).toBe(true);
    expect(byId(w, "timeline-ruler-label").classes()).toContain("z-[3]");
    useEditorWorkspaceStore().setPlayhead(2_000);
    await flushPromises();
    expect(byId(w, "timeline-playhead").attributes("style")).toContain(`left: ${196 + 100}px`);
    expect(byId(w, "timeline-playhead-head").attributes("style")).toContain("left: 95px");
  });
});

describe("the snap guide", () => {
  it("a drag that snaps shows a dashed gold guide at the snapped time; release hides it", async () => {
    const w = await mountTimeline();
    const late = byId(w, "clip-late");
    // late starts at 5 s; 3 s right is 8 s, 4 px from sound's end (8 s):
    // inside the 8 px snap threshold, so its start snaps to 8 s.
    await late.trigger("pointerdown", { clientX: 1_000, clientY: 0, pointerId: 1, button: 0 });
    await late.trigger("pointermove", { clientX: 1_146, clientY: 0, pointerId: 1 });
    const guide = byId(w, "timeline-snap-guide");
    expect(guide.classes()).toEqual(expect.arrayContaining(["border-l", "border-dashed", "border-gold", "top-8", "bottom-0", "pointer-events-none", "z-[12]"]));
    expect(guide.attributes("style")).toContain(`left: ${196 + 400}px`);
    await late.trigger("pointerup", { clientX: 1_146, clientY: 0, pointerId: 1 });
    await flushPromises();
    expect(w.find('[data-testid="timeline-snap-guide"]').exists()).toBe(false);
  });

  it("shows no guide while Snap is off", async () => {
    const w = await mountTimeline();
    useEditorWorkspaceStore().toggleSnap();
    const late = byId(w, "clip-late");
    await late.trigger("pointerdown", { clientX: 1_000, clientY: 0, pointerId: 1, button: 0 });
    await late.trigger("pointermove", { clientX: 1_146, clientY: 0, pointerId: 1 });
    expect(w.find('[data-testid="timeline-snap-guide"]').exists()).toBe(false);
    await late.trigger("keydown", { key: "Escape" });
  });
});

// ---- extreme content ---------------------------------------------------------------

describe("a 3px clip", () => {
  it("renders with no badges, no pill and its name clipped inside the body", async () => {
    const w = await mountTimeline({}, {
      clips: [clip("tiny", "vid", "v1", { start_ms: 1_000, in_ms: 0, out_ms: 60, group_id: "g" })],
    });
    const body = byId(w, "clip-tiny");
    expect(body.attributes("style")).toContain("width: 5px");
    expect(w.find('[data-testid="clip-tiny-badges"]').exists()).toBe(false);
    expect(w.find('[data-testid="clip-tiny-duration"]').exists()).toBe(false);
    expect(byId(w, "clip-tiny-content").classes()).toContain("overflow-hidden");
    expect(byId(w, "clip-tiny-name").classes()).toContain("truncate");
    // Too narrow for two 9px grips inside: they step outside its edges.
    expect(byId(w, "clip-tiny-trim-start").classes()).toContain("right-full");
    expect(byId(w, "clip-tiny-trim-end").classes()).toContain("left-full");
  });

  it("stays reachable from the keyboard: a nudge moves it, Shift+F10 offers its trims", async () => {
    const w = await mountTimeline({}, {
      clips: [clip("tiny", "vid", "v1", { start_ms: 1_000, in_ms: 0, out_ms: 60 })],
    });
    const body = byId(w, "clip-tiny");
    expect(body.attributes("tabindex")).toBe("0");
    await body.trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["tiny"], deltaMs: 33, trackId: null }]);
    // With the playhead inside it, its Trim to pointer acts there.
    useEditorWorkspaceStore().setPlayhead(1_030);
    await body.trigger("keydown", { key: "F10", shiftKey: true });
    await flushPromises();
    const trim = document.querySelector('[data-testid="editor-context-menu-item-trim"]');
    expect(trim).not.toBeNull();
    expect(trim?.getAttribute("aria-disabled")).not.toBe("true");
  });
});

// ---- Fit ---------------------------------------------------------------------------

describe("Fit (§6.3)", () => {
  it.each([
    [1600, 196],
    [960, 174],
  ])("at %ipx fits the edit into the lanes beside the %ipx label column, less 26px", async (width, label) => {
    useEditorWorkspaceStore().setViewport(width, 1000);
    const w = await mountTimeline({}, {}, width);
    const workspace = useEditorWorkspaceStore();
    await byId(w, "timeline-toolbar-fit").trigger("click");
    // 20 s of edit into (width - label - 26) px.
    expect(workspace.timelineZoom).toBeCloseTo((width - label - 26) / (0.05 * 20_000), 5);
  });

  it("fits a short edit as if it were 15 s long, like the concept's ruler", async () => {
    const w = await mountTimeline({}, { clips: [clip("one", "vid", "v1", { out_ms: 3_000 })] }, 1400, 3_000);
    await byId(w, "timeline-toolbar-fit").trigger("click");
    expect(useEditorWorkspaceStore().timelineZoom).toBeCloseTo((1400 - 196 - 26) / (0.05 * 15_000), 5);
  });
});
