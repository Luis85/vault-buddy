/**
 * The Inspector's Speed category (Task 31; F-16): `SpeedSection.vue`. Every
 * control sends one `setSpeed` for one clip, and an out-of-range entry stays
 * visible with its correction and sends nothing. (The Layout category's
 * tests moved to `editorLayoutSection.test.ts` with visual-parity Task 14.)
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import SpeedSection from "../src/components/editor/inspector/SpeedSection.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import type { Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "cam",
    track_id: "v1",
    name: id,
    start_ms: 700,
    in_ms: 1_000,
    out_ms: 5_000,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 0.8,
    volume: 1,
    muted: false,
    x: 0.61,
    y: 0.23,
    w: 0.27,
    h: 0.19,
    ...overrides,
  };
}
function project(
  clips: Clip[],
  tracks: Track[] = [track("v1"), track("a1", { kind: "audio" })],
  canvas = { width: 720, height: 1280, fps: 30 },
): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas,
    master_gain: 1,
    assets: [{ id: "cam", kind: "video", name: "cam.mp4", duration_ms: 60_000, width: 1280, height: 720 }],
    tracks,
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

let executed: EditorCommand[];

async function open(p: Project): Promise<void> {
  executed = [];
  const store = useEditorProjectStore();
  const snapshot = {
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
  store.setPort(
    fakeEditorPort({
      openStaged: () =>
        Promise.resolve({ snapshot, project: p, workspace: {}, missing: [], sourceBase: "base", recovered: false }),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: p });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
}

async function type(w: ReturnType<typeof mount>, testid: string, value: string): Promise<void> {
  const input = w.get(`[data-testid="${testid}"]`);
  await input.setValue(value);
  await input.trigger("keydown", { key: "Enter" });
  await flushPromises();
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("SpeedSection", () => {
  it("a preset sends setSpeed with the clip's pitch setting", async () => {
    await open(project([clip("c1", { preserve_pitch: false })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="speed-preset-1"]').attributes("aria-pressed")).toBe("true");
    await w.get('[data-testid="speed-preset-2"]').trigger("click");
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 2, preservePitch: false }]);
    // The current speed's own preset sends nothing.
    await w.get('[data-testid="speed-preset-1"]').trigger("click");
    expect(executed).toHaveLength(1);
  });

  it("numeric speed is bounded to 0.25x..4x and says how long the clip plays", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    // 4000 ms of source at 1.5x.
    expect(w.get('[data-testid="speed-section-duration"]').text()).toContain("2.67 s");
    await type(w, "speed-section-speed", "5");
    expect(w.get('[data-testid="speed-section-speed-error"]').text()).toContain("0.25× and 4×");
    expect(executed).toEqual([]);
    await type(w, "speed-section-speed", "0.75");
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 0.75, preservePitch: true }]);
  });

  it("Escape puts a typed speed back and sends nothing; leaving the field sends it", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const input = w.get('[data-testid="speed-section-speed"]');
    await input.setValue("3");
    await input.trigger("keydown", { key: "Escape" });
    expect((input.element as HTMLInputElement).value).toBe("1.5");
    await input.setValue("1.25");
    await input.trigger("blur");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 1.25, preservePitch: true }]);
  });

  it("a locked track disables speed and says why", async () => {
    await open(project([clip("c1")], [track("v1", { locked: true, name: "Screen" })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    expect(w.find('[data-testid="speed-section-locked"]').exists()).toBe(false);
    expect(w.get('[data-testid="speed-section"]').attributes("title")).toContain("Track Screen is locked");
    expect(w.get('[data-testid="speed-preset-2"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="speed-section-pitch"]').setValue(false);
    expect(executed).toEqual([]);
  });

  it("preserve pitch toggles with the current speed", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="speed-section-pitch"]').setValue(false);
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 1.5, preservePitch: false }]);
  });

  it("a multi-selection gets a note, not a silent edit of the first clip", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 6_000 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1", "c2"] } });
    expect(w.find('[data-testid="speed-section"]').exists()).toBe(false);
    expect(w.get('[data-testid="speed-section-multi"]').text()).toContain("single clip");
  });
});
