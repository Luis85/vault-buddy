/**
 * `AudioSection.vue` (Task 27; F-05, F-24) — the Inspector's Audio category:
 * clip volume (linear stored, dB read out), mute, and Detach audio.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import AudioSection from "../src/components/editor/inspector/AudioSection.vue";
import { EditorPortError } from "../src/editor/port";
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

describe("AudioSection — volume", () => {
  // The named case: a slider drag is MANY input events and ONE commit. The
  // draft moves with every step (so does the dB readout); only the release
  // (`change`) sends, and the blur that follows it is not a second send.
  it("volume draft commits once", async () => {
    const w = await mountSection(["c1"]);
    const slider = w.get('[data-testid="audio-section-volume-slider"]');
    // A real drag: `input` per step, `change` once on release. (VTU's
    // `setValue` fires BOTH, which is a release per step — not a drag.)
    for (const v of ["0.7", "0.5", "0.25"]) {
      (slider.element as HTMLInputElement).value = v;
      await slider.trigger("input");
    }
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="audio-section-db"]').text()).toBe("−12.0 dB");

    await slider.trigger("change");
    await w.get('[data-testid="audio-section-volume"]').trigger("blur");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1"], volume: 0.25 }]);
  });

  it("typed volume commits on Enter once, and stores LINEAR while reading out dB", async () => {
    const w = await mountSection(["c1"]);
    expect(w.get('[data-testid="audio-section-db"]').text()).toBe("−1.9 dB");
    const field = w.get('[data-testid="audio-section-volume"]');
    await field.setValue("2");
    expect(w.get('[data-testid="audio-section-db"]').text()).toBe("+6.0 dB");
    await field.trigger("keydown", { key: "Enter" });
    await field.trigger("blur");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1"], volume: 2 }]);

    await field.setValue("0");
    expect(w.get('[data-testid="audio-section-db"]').text()).toBe("−∞ dB");
  });

  it("an out-of-range volume stays visible with a correction and sends nothing", async () => {
    const w = await mountSection(["c1"]);
    const field = w.get('[data-testid="audio-section-volume"]');
    await field.setValue("2.5");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="audio-section-volume-error"]').text()).toContain("between 0 and 2");
    expect((field.element as HTMLInputElement).value).toBe("2.5");
  });

  it("a volume Rust refuses falls back to the committed value", async () => {
    refuse = true;
    const w = await mountSection(["c1"]);
    const field = w.get('[data-testid="audio-section-volume"]');
    await field.setValue("1.5");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect((field.element as HTMLInputElement).value).toBe("0.8");
  });
});

describe("AudioSection — mute and detach", () => {
  it("mute acts on the whole selection; volume and detach need one clip", async () => {
    const w = await mountSection(["c1", "c2"]);
    expect(w.find('[data-testid="audio-section-multi"]').exists()).toBe(true);
    expect(w.find('[data-testid="audio-section-volume"]').exists()).toBe(false);
    expect(w.find('[data-testid="audio-section-detach"]').exists()).toBe(false);
    await w.get('[data-testid="audio-section-mute"]').trigger("click");
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1", "c2"], muted: true }]);
  });

  it("a clip on a locked track sends nothing and says why", async () => {
    const w = await mountSection(["c1"], { tracks: [track("v1", { locked: true })] });
    const mute = w.get('[data-testid="audio-section-mute"]');
    expect(mute.attributes("title")).toBe("Track v1 is locked");
    await mute.trigger("click");
    expect(executed).toEqual([]);
  });

  it("Detach audio lands on the free audio track through the shared action registry", async () => {
    const w = await mountSection(["c1"]);
    await w.get('[data-testid="audio-section-detach"]').trigger("click");
    expect(executed).toEqual([{ kind: "detachAudio", clipId: "c1", audioTrackId: "au1" }]);
  });

  it("a still image has no audio controls at all", async () => {
    const w = await mountSection(["c1"], { assets: [asset("a1", { media_type: "image" })] });
    expect(w.get('[data-testid="audio-section-still"]').text()).toBe("A still image has no sound.");
    expect(w.find('[data-testid="audio-section-detach"]').exists()).toBe(false);
  });

  it("a detached audio clip names the video it came from, and cannot be detached again", async () => {
    const w = await mountSection(["d"], {
      assets: [asset("a1"), asset("a1-audio", { kind: "audio", name: "a1 · audio", linked_asset: "a1" })],
      clips: [clip({ muted: true }), clip({ id: "d", asset_id: "a1-audio", track_id: "au1" })],
    });
    expect(w.get('[data-testid="audio-section-linked"]').text()).toBe("Detached from a1.");
    const detach = w.get('[data-testid="audio-section-detach"]');
    expect(detach.attributes("aria-disabled")).toBe("true");
    await detach.trigger("click");
    expect(executed).toEqual([]);
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
