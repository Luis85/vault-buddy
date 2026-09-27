/**
 * `AudioSection.vue` (Task 27; F-05, F-24; visual-parity Task 15) — the
 * Inspector's Audio category: clip volume (linear stored, percent read
 * out), Mute this clip, Detach source audio and Open audio mixer.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import AudioSection from "../src/components/editor/inspector/AudioSection.vue";
import { EditorPortError } from "../src/editor/port";
import { revealSerial } from "../src/editor/revealBus";
import type { Asset, Clip, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function asset(id: string, overrides: Partial<Asset> = {}): Asset {
  return { id, kind: "video", name: id, duration_ms: 60_000, ...overrides };
}
// Asymmetric: start/in/out are distinct, the volume is not 1.
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1",
    asset_id: "a1",
    track_id: "v1",
    name: "Intro",
    start_ms: 2_000,
    in_ms: 300,
    out_ms: 1_300,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 0.8,
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
    assets: [asset("a1")],
    tracks: [track("v1"), track("au1", { kind: "audio" })],
    clips: [clip(), clip({ id: "c2", start_ms: 5_000, in_ms: 0, out_ms: 500 })],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}
function snapshot(): EditorSnapshot {
  return {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 1,
    persistedRevision: null,
    title: "Tutorial",
    durationMs: 10_000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
  };
}

let executed: unknown[] = [];
let refuse = false;

async function mountSection(clipIds: string[], overrides: Partial<Project> = {}) {
  executed = [];
  const store = useEditorProjectStore();
  const p = project(overrides);
  const s = snapshot();
  store.setPort(
    fakePort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({
          snapshot: s,
          project: p,
          workspace: {},
          missing: [],
          sourceBase: "base",
          recovered: false,
        }),
      execute: (req) => {
        executed.push(req.command);
        if (refuse) {
          return Promise.reject(
            new EditorPortError({ code: "invalidRequest", message: "no", retryable: false, operationId: "op" }),
          );
        }
        return Promise.resolve({ snapshot: { ...s, revision: s.revision + 1 }, project: p });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
  useEditorWorkspaceStore().select(clipIds);
  const w = mount(AudioSection, { props: { clipIds } });
  await flushPromises();
  return w;
}

afterEach(() => {
  refuse = false;
});

describe("AudioSection — Clip audio", () => {
  // Visual-parity Task 15 (concept spec §5 "Audio"): a Volume range in
  // percent, 0–200. A drag is MANY input events and ONE commit: only the
  // release (`change`) sends.
  it("the volume range reads out percent and commits once on release, stored LINEAR", async () => {
    const w = await mountSection(["c1"]);
    expect(w.findAll("h3").map((h) => h.text())).toEqual(["Clip audio", "Mix"]);
    const slider = w.get('[data-testid="audio-section-volume"]');
    expect([slider.attributes("min"), slider.attributes("max")]).toEqual(["0", "200"]);
    expect(w.get('[data-testid="audio-section-volume-value"]').text()).toBe("80%");
    // A real drag: `input` per step, `change` once on release.
    for (const v of ["70", "50", "25"]) {
      (slider.element as HTMLInputElement).value = v;
      await slider.trigger("input");
    }
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="audio-section-volume-value"]').text()).toBe("25%");
    await slider.trigger("change");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1"], volume: 0.25 }]);
  });

  it("a volume Rust refuses puts the slider back", async () => {
    refuse = true;
    const w = await mountSection(["c1"]);
    const slider = w.get('[data-testid="audio-section-volume"]');
    (slider.element as HTMLInputElement).value = "150";
    await slider.trigger("input");
    await slider.trigger("change");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect(w.get('[data-testid="audio-section-volume-value"]').text()).toBe("80%");
  });

  it("Mute this clip is a checkbox over the whole selection", async () => {
    const one = await mountSection(["c1"]);
    const mute = one.get('[data-testid="audio-section-mute"]');
    expect(mute.element.closest("label")?.textContent).toContain("Mute this clip");
    expect((mute.element as HTMLInputElement).checked).toBe(false);
    await mute.setValue(true);
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1"], muted: true }]);

    const both = await mountSection(["c1", "c2"]);
    expect(both.find('[data-testid="audio-section-multi"]').exists()).toBe(true);
    expect(both.find('[data-testid="audio-section-volume"]').exists()).toBe(false);
    expect(both.find('[data-testid="audio-section-detach"]').exists()).toBe(false);
    await both.get('[data-testid="audio-section-mute"]').setValue(true);
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1", "c2"], muted: true }]);
  });

  it("a mute Rust refuses puts the box back", async () => {
    refuse = true;
    const w = await mountSection(["c1"]);
    const mute = w.get('[data-testid="audio-section-mute"]');
    await mute.setValue(true);
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect((mute.element as HTMLInputElement).checked).toBe(false);
  });

  it("a clip on a locked track disables the tab and sends nothing", async () => {
    const w = await mountSection(["c1"], { tracks: [track("v1", { locked: true }), track("au1", { kind: "audio" })] });
    const fieldset = w.get('[data-testid="audio-section"]');
    expect(fieldset.attributes("title")).toBe("Track v1 is locked");
    expect(fieldset.attributes("disabled")).toBeDefined();
    await w.get('[data-testid="audio-section-mute"]').setValue(true);
    expect(executed).toEqual([]);
  });
});

describe("AudioSection — detach and mix", () => {
  it("Detach source audio lands on the free audio track through the shared action registry", async () => {
    const w = await mountSection(["c1"]);
    const detach = w.get('[data-testid="audio-section-detach"]');
    expect(detach.text()).toBe("Detach source audio");
    await detach.trigger("click");
    expect(executed).toEqual([{ kind: "detachAudio", clipId: "c1", audioTrackId: "au1" }]);
  });

  it("a video whose sound is already detached says why Detach is unavailable", async () => {
    const w = await mountSection(["c1"], {
      assets: [asset("a1"), asset("a1-audio", { kind: "audio", name: "a1 · audio", linked_asset: "a1" })],
      clips: [clip(), clip({ id: "d", asset_id: "a1-audio", track_id: "au1" })],
    });
    const detach = w.get('[data-testid="audio-section-detach"]');
    expect(detach.attributes("aria-disabled")).toBe("true");
    expect(detach.attributes("title")).toBe("This clip's audio is already detached");
    await detach.trigger("click");
    expect(executed).toEqual([]);
  });

  it("a still image has no audio controls at all", async () => {
    const w = await mountSection(["c1"], { assets: [asset("a1", { media_type: "image" })] });
    expect(w.get('[data-testid="audio-section-still"]').text()).toBe("A still image has no sound.");
    expect(w.find('[data-testid="audio-section-detach"]').exists()).toBe(false);
  });

  it("a detached audio clip names the video it came from and offers no Detach", async () => {
    const w = await mountSection(["d"], {
      assets: [asset("a1"), asset("a1-audio", { kind: "audio", name: "a1 · audio", linked_asset: "a1" })],
      clips: [clip({ muted: true }), clip({ id: "d", asset_id: "a1-audio", track_id: "au1" })],
    });
    expect(w.get('[data-testid="audio-section-linked"]').text()).toBe("Detached from a1.");
    expect(w.find('[data-testid="audio-section-detach"]').exists()).toBe(false);
  });

  it("Open audio mixer asks the mixer to show itself, as View → Audio mixer does", async () => {
    const w = await mountSection(["c1"]);
    const before = revealSerial("mixer");
    await w.get('[data-testid="audio-section-mixer"]').trigger("click");
    expect(revealSerial("mixer")).toBe(before + 1);
    expect(w.text()).toContain("Monitoring mute does not mute exports. Track mute and solo do affect exports.");
  });
});

describe("AudioSection — stems", () => {
  // Task 53: a capture recorded WITHOUT stems has every input mixed into one
  // track, which no editor command can split. Selecting it must say so and
  // name the setting that records them separately -- rather than leaving the
  // user to hunt for a per-input control that cannot exist here.
  it("explains stems when a capture has none", async () => {
    const w = await mountSection(["c1"], { assets: [asset("a1", { builtin: "screen" })] });
    const note = w.get("[data-testid='audio-section-stems-absent']").text();
    expect(note).toContain("mixed into this one track");
    expect(note).toContain("Keep each audio input as a separate track");
  });

  it("says nothing about stems once the capture has them, or for other media", async () => {
    const withStems = await mountSection(["c1"], {
      assets: [
        asset("a1", { builtin: "screen" }),
        asset("stem-1", { kind: "audio", name: "USB Mic" }),
      ],
    });
    expect(withStems.find("[data-testid='audio-section-stems-absent']").exists()).toBe(false);
    const imported = await mountSection(["c1"]);
    expect(imported.find("[data-testid='audio-section-stems-absent']").exists()).toBe(false);
  });
});
