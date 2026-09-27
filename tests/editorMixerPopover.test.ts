/**
 * `MixerPopover.vue` (Task 27; F-05, F-25) — per-track volume/mute/solo,
 * master gain, the monitoring-only mute and the preview peak meter.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import MixerPopover from "../src/components/editor/shell/MixerPopover.vue";
import { EditorPortError } from "../src/editor/port";
import { computeLayers } from "../src/editor/previewLayers";
import type { Clip, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

let executed: unknown[] = [];
let refuse = false;

beforeEach(() => {
  setActivePinia(createPinia());
  executed = [];
  refuse = false;
});

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "audio", name: `Track ${id}`, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function clip(id: string, trackId: string): Clip {
  return {
    id,
    asset_id: "a1",
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
  };
}
function project(tracks: Track[]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 0.7,
    assets: [{ id: "a1", kind: "audio", name: "a1", duration_ms: 60_000 }],
    tracks,
    clips: tracks.map((t) => clip(`c-${t.id}`, t.id)),
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}
const SNAPSHOT: EditorSnapshot = {
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

async function mountMixer(tracks: Track[], readPeak?: () => number | null) {
  const p = project(tracks);
  const port = fakePort({
    openStaged: () =>
      Promise.resolve<EditorOpenResult>({
        snapshot: SNAPSHOT,
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
      return Promise.resolve({ snapshot: { ...SNAPSHOT, revision: 2 }, project: p });
    },
    saveWorkspace: () => Promise.resolve(),
  });
  useEditorProjectStore().setPort(port);
  useEditorWorkspaceStore().setPort(port);
  await useEditorProjectStore().openStaged("base");
  const w = mount(MixerPopover, { props: { readPeak }, attachTo: document.body });
  await w.get('[data-testid="mixer-toggle"]').trigger("click");
  await flushPromises();
  return { w, p };
}

describe("MixerPopover", () => {
  // A06's UI half: the monitoring mute is the WORKSPACE flag the preview
  // reads, and it never becomes an editor command — the rendered video's
  // mix cannot change through it.
  it("monitoring mute never sends a command", async () => {
    const { w } = await mountMixer([track("t1")]);
    const box = w.get('[data-testid="mixer-monitor-mute"]');
    expect(box.element.parentElement?.textContent).toContain("Mute preview (does not affect the video)");
    await box.setValue(true);
    await flushPromises();
    expect(useEditorWorkspaceStore().monitorMuted).toBe(true);
    expect(executed).toEqual([]);
  });

  it("track mute, solo and a released volume drag are one render-affecting command each", async () => {
    const { w } = await mountMixer([track("t1"), track("t2", { volume: 1.2 })]);
    await w.get('[data-testid="mixer-muted-t1"]').trigger("click");
    await w.get('[data-testid="mixer-solo-t2"]').trigger("click");
    const slider = w.get('[data-testid="mixer-volume-t2"]');
    for (const v of ["1.5", "0.5"]) {
      (slider.element as HTMLInputElement).value = v;
      await slider.trigger("input");
    }
    expect(w.get('[data-testid="mixer-track-t2"]').text()).toContain("−6.0 dB");
    await slider.trigger("change");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "setTrackFlags", trackId: "t1", muted: true },
      { kind: "setTrackFlags", trackId: "t2", solo: true },
      { kind: "setTrackFlags", trackId: "t2", volume: 0.5 },
    ]);
  });

  it("master gain commits once on release, and a refusal snaps it back", async () => {
    refuse = true;
    const { w } = await mountMixer([track("t1")]);
    const master = w.get('[data-testid="mixer-master"]');
    (master.element as HTMLInputElement).value = "0.4";
    await master.trigger("input");
    await master.trigger("change");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setMasterGain", gain: 0.4 }]);
    expect((master.element as HTMLInputElement).value).toBe("0.7");
  });

  // The ONE solo rule: what the mixer calls "Silenced by solo" is exactly
  // what the preview's monitoring silences (`computeLayers`), and a soloed
  // track's own mute still wins.
  it("labels solo exactly as the preview applies it", async () => {
    const tracks = [track("s", { solo: true }), track("plain"), track("sm", { solo: true, muted: true })];
    const { w, p } = await mountMixer(tracks);
    const status = (id: string) => w.get(`[data-testid="mixer-status-${id}"]`).text();
    expect([status("s"), status("plain"), status("sm")]).toEqual(["", "Silenced by solo", "Muted"]);
    const silent = computeLayers(p, 0, { width: 100, height: 100 }, { muted: false, volume: 1 })
      .filter((l) => l.muted)
      .map((l) => l.clipId)
      .sort();
    expect(silent).toEqual(["c-plain", "c-sm"]);
  });

  it("a locked track's controls send nothing and say why", async () => {
    const { w } = await mountMixer([track("t1", { locked: true })]);
    expect(w.get('[data-testid="mixer-track-t1"]').attributes("title")).toBe("Track Track t1 is locked");
    await w.get('[data-testid="mixer-solo-t1"]').trigger("click");
    expect(executed).toEqual([]);
  });

  // F-25's acceptance: a sample peak is labelled a peak, never loudness.
  it("shows the preview's sample peak as a peak in dBFS, polled while open", async () => {
    vi.useFakeTimers();
    try {
      let sample: number | null = 0.5;
      const { w } = await mountMixer([track("t1")], () => sample);
      const meter = w.get('[data-testid="mixer-peak"]');
      expect(meter.text()).toContain("Preview peak");
      expect(meter.text()).toContain("−6.0 dBFS");
      expect(w.get('[data-testid="mixer-popover"]').text().toLowerCase()).not.toContain("loudness");
      sample = null;
      vi.advanceTimersByTime(150);
      await flushPromises();
      expect(meter.text()).toContain("No preview audio");
    } finally {
      vi.useRealTimers();
    }
  });

  it("Escape closes the popover without letting the key reach the panel", async () => {
    const { w } = await mountMixer([track("t1")]);
    const outer = vi.fn();
    window.addEventListener("keydown", outer);
    await w.get('[data-testid="mixer-popover"]').trigger("keydown", { key: "Escape" });
    window.removeEventListener("keydown", outer);
    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(false);
    expect(outer).not.toHaveBeenCalled();
  });

  // F-M8: the popover used to close on Escape only when focus was already
  // inside it (a `@keydown` bound on the popover's own div). A window-level
  // listener is what closes it when focus sits elsewhere -- e.g. the
  // timeline, which is where a user opening the mixer while editing is
  // most likely to have focus.
  it("Escape closes the mixer even when focus is elsewhere on the timeline", async () => {
    const { w } = await mountMixer([track("t1")]);
    const timelineStandIn = document.createElement("button");
    document.body.appendChild(timelineStandIn);
    timelineStandIn.focus();

    timelineStandIn.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await flushPromises();

    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(false);
    timelineStandIn.remove();
  });

  // F-M8: no outside-pointerdown closer existed at all -- clicking anywhere
  // else on the editor left the popover open.
  it("a pointerdown outside the popover closes it", async () => {
    const { w } = await mountMixer([track("t1")]);
    const outside = document.createElement("div");
    document.body.appendChild(outside);

    outside.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();

    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(false);
    outside.remove();
  });

  it("a pointerdown on the mixer's own trigger button does not count as outside", async () => {
    const { w } = await mountMixer([track("t1")]);
    w.get('[data-testid="mixer-toggle"]').element.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true }),
    );
    await flushPromises();

    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(true);
  });
});

// Visual-parity Task 22 (concept spec §9.8, `shell.html: id="mixerDialog"`):
// the concept's look — a header with the title and a ✕, one
// `125px 1fr 52px` row per track (name, M/S gold when on, the kind; the
// range; a mono dB readout), and "Master output" with a mono percent.
describe("MixerPopover — the concept look (§9.8)", () => {
  it("has the concept's header and a labelled close", async () => {
    const { w } = await mountMixer([track("t1")]);
    const popover = w.get('[data-testid="mixer-popover"]');
    expect(popover.get("h2").text()).toBe("Audio mixer");
    expect(popover.get("header p").text()).toBe("Track levels change both the preview and the rendered video.");
    expect(w.get('[data-testid="mixer-close"]').attributes("aria-label")).toBe("Close audio mixer");
    await w.get('[data-testid="mixer-close"]').trigger("click");
    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(false);
  });

  it("lays each track out as name + M/S, range, mono dB", async () => {
    const { w } = await mountMixer([track("t1", { volume: 0.5 })]);
    const row = w.get('[data-testid="mixer-track-t1"]');
    expect(row.classes()).toEqual(expect.arrayContaining(["grid", "grid-cols-[125px_1fr_52px]"]));
    expect(row.get("b").text()).toBe("Track t1");
    const mute = w.get('[data-testid="mixer-muted-t1"]');
    expect(mute.text()).toBe("M");
    expect(mute.attributes("aria-label")).toBe("Mute Track t1");
    expect(w.get('[data-testid="mixer-solo-t1"]').attributes("aria-label")).toBe("Solo Track t1");
    expect(row.text()).toContain("audio");
    const readout = w.get('[data-testid="mixer-readout-t1"]');
    expect(readout.text()).toBe("−6.0 dB");
    expect(readout.classes()).toContain("font-mono");
  });

  it("an active M or S is gold", async () => {
    const { w } = await mountMixer([track("t1", { muted: true }), track("t2", { solo: true })]);
    for (const id of ["mixer-muted-t1", "mixer-solo-t2"]) {
      expect(w.get(`[data-testid="${id}"]`).classes()).toEqual(expect.arrayContaining(["bg-gold-bg", "text-gold"]));
    }
    expect(w.get('[data-testid="mixer-solo-t1"]').classes()).not.toContain("text-gold");
  });

  it("Master output reads its level as a mono percent", async () => {
    const { w } = await mountMixer([track("t1")]);
    const master = w.get('[data-testid="mixer-master-row"]');
    expect(master.get("b").text()).toBe("Master output");
    expect(w.get('[data-testid="mixer-master-readout"]').text()).toBe("70%");
  });

  // D14: a locked track's controls say why on screen, not only in a title.
  it("a locked track says so in its row", async () => {
    const { w } = await mountMixer([track("t1", { locked: true })]);
    expect(w.get('[data-testid="mixer-status-t1"]').text()).toBe("Track Track t1 is locked");
  });
});
