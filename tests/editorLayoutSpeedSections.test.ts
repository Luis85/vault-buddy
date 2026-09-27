/**
 * The Inspector's Speed category (Task 31; F-16; visual-parity Task 15):
 * `SpeedSection.vue`. Every control sends one `setSpeed` for one clip —
 * preceded or followed by one `moveClips` when the following clips move. (The Layout category's
 * tests moved to `editorLayoutSection.test.ts` with visual-parity Task 14.)
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import SpeedSection from "../src/components/editor/inspector/SpeedSection.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { GROUPED_WITH_CLIP, speedRipple } from "../src/editor/speedRipple";
import type { Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);
// The ripple choice is window-local state: never leak one test's into the next.
afterEach(() => {
  speedRipple.value = "ripple";
});

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

/** `refuse` names the command kind Rust turns down, if any. */
async function open(p: Project, refuse: EditorCommand["kind"] | null = null): Promise<void> {
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
        if (req.command.kind === refuse) return Promise.reject(new Error("refused"));
        // Every accepted command is a new revision, so each reply installs.
        return Promise.resolve({ snapshot: { ...snapshot, revision: 1 + executed.length }, project: p });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("SpeedSection", () => {
  // Visual-parity Task 15 (concept spec §5 "Speed"): "Keep the useful
  // pace", the readout, the Clip speed select, "When duration changes" and
  // "Preserve voice pitch where supported".
  it("reads out the speed, the timeline length and the source length", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    expect(w.get("h3").text()).toBe("Keep the useful pace");
    expect(w.get('[data-testid="speed-section-readout-speed"]').text()).toBe("1.5×");
    // 4000 ms of source at 1.5× plays for 2667 ms.
    expect(w.get('[data-testid="speed-section-readout"]').text()).toContain("00:02.6 on the timeline");
    expect(w.get('[data-testid="speed-section-readout"]').text()).toContain("00:04.0 source duration");
  });

  it("the Clip speed select sends setSpeed with the clip's pitch setting", async () => {
    await open(project([clip("c1", { preserve_pitch: false })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const select = w.get('[data-testid="speed-section-speed"]');
    expect((select.element as HTMLSelectElement).value).toBe("1");
    expect(select.findAll("option").map((o) => o.text())).toEqual([
      "0.25×", "0.5×", "0.75×", "1× · original", "1.25×", "1.5×", "2×", "3×", "4×",
    ]);
    await select.setValue("2");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 2, preservePitch: false }]);
  });

  it("a speed set elsewhere is offered as itself, never shown as another", async () => {
    await open(project([clip("c1", { speed: 1.1 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const select = w.get('[data-testid="speed-section-speed"]');
    expect((select.element as HTMLSelectElement).value).toBe("1.1");
    const el = select.element as HTMLSelectElement;
    expect(el.options[el.selectedIndex].text).toBe("1.1×");
  });

  it("moves the following clips on this track by default, and keeps them in place when asked", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 4_700 }), clip("x1", { start_ms: 5_000, track_id: "v2" })],
      [track("v1"), track("v2"), track("a1", { kind: "audio" })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const ripple = w.get('[data-testid="speed-section-ripple"]');
    expect((ripple.element as HTMLSelectElement).value).toBe("ripple");
    expect(ripple.findAll("option").map((o) => o.text())).toEqual([
      "Move following clips on this track", "Keep following clips in place",
    ]);
    await w.get('[data-testid="speed-section-speed"]').setValue("0.5");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "moveClips", clipIds: ["c2"], deltaMs: 4_000, trackId: null },
      { kind: "setSpeed", clipId: "c1", speed: 0.5, preservePitch: true },
    ]);
    executed.length = 0;
    await ripple.setValue("leave");
    await w.get('[data-testid="speed-section-speed"]').setValue("2");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 2, preservePitch: true }]);
  });

  // Fix round 1 (ruling T15-2): a clip grouped with a clip after it cannot
  // move the following clips without moving itself.
  it("grouped clips that would move wrongly leave only Keep following clips in place, and say why", async () => {
    await open(project([clip("c1", { group_id: "g" }), clip("c2", { start_ms: 4_700, group_id: "g" })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const ripple = w.get('[data-testid="speed-section-ripple"]');
    expect((ripple.element as HTMLSelectElement).value).toBe("leave");
    const move = ripple.get('option[value="ripple"]');
    expect(move.attributes("disabled")).toBeDefined();
    expect(move.attributes("title")).toBe(GROUPED_WITH_CLIP);
    expect(w.get('[data-testid="speed-section-ripple-reason"]').text()).toBe(GROUPED_WITH_CLIP);
    await w.get('[data-testid="speed-section-speed"]').setValue("0.5");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 0.5, preservePitch: true }]);
  });

  it("a speed Rust refuses after the following clips moved puts them back", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 4_700 })]), "setSpeed");
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const select = w.get('[data-testid="speed-section-speed"]');
    await select.setValue("0.5");
    await flushPromises();
    expect(executed.map((c) => c.kind)).toEqual(["moveClips", "setSpeed", "undo"]);
    // The select shows the speed the clip still has, and the refusal is
    // still what the editor says — the undo's own reply does not erase it.
    expect((select.element as HTMLSelectElement).value).toBe("1");
    expect(useEditorProjectStore().lastError?.message).toBe("refused");
  });

  it("a locked track disables speed and says why", async () => {
    await open(project([clip("c1")], [track("v1", { locked: true, name: "Screen" })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const fieldset = w.get('[data-testid="speed-section"]');
    expect(fieldset.attributes("title")).toContain("Track Screen is locked");
    expect(fieldset.attributes("disabled")).toBeDefined();
    await w.get('[data-testid="speed-section-pitch"]').setValue(false);
    expect(executed).toEqual([]);
  });

  it("a pitch change Rust refuses puts the box back", async () => {
    await open(project([clip("c1")]), "setSpeed");
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const box = w.get('[data-testid="speed-section-pitch"]');
    await box.setValue(false);
    await flushPromises();
    expect((box.element as HTMLInputElement).checked).toBe(true);
  });

  it("preserve voice pitch toggles with the current speed", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="speed-section-pitch"]').element.closest("label")?.textContent).toContain(
      "Preserve voice pitch where supported",
    );
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
