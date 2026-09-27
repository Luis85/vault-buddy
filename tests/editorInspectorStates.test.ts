/**
 * The inspector's frame and selection states (visual-parity Task 13;
 * concept spec §5): the 48px heading with its ✕ (D5's inspector toggle),
 * the selection card, the 2×3 category grid, and the four states that are
 * not a single clip — nothing selected, a track, several clips and a cue.
 * Every control in the track and multi states maps to one command or one
 * registry action (the track and multi-clip menus' own builders), and every
 * refusal says why.
 *
 * Fixtures are the concept's sample shapes (screen 02): a 32 s presenter
 * clip on "Webcam · presenter", two screen clips, an audio bed.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import RemoveTrackDialog from "../src/components/editor/dialogs/RemoveTrackDialog.vue";
import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import { useEditorMenuContext } from "../src/composables/useEditorMenuContext";
import { findMenuAction } from "../src/composables/useInspectorMenu";
import { baseActionContext } from "../src/editor/actionContext";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { trackMenu } from "../src/editor/menuSets";
import { checksDialogOpen } from "../src/editor/revealBus";
import { trackRemovalRequest } from "../src/editor/trackRemoval";
import type { Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { clip, openInspectorProject, project, track } from "./helpers/inspectorProject";

enableAutoUnmount(afterEach);

let executed: EditorCommand[];

async function openProject(p: Project = project()) {
  executed = await openInspectorProject(p);
}

beforeEach(() => {
  setActivePinia(createPinia());
  checksDialogOpen.value = false;
  trackRemovalRequest.value = null;
});

const heading = (w: ReturnType<typeof mount>) => w.get('[data-testid="inspector-heading"]');

// ---- the frame ----------------------------------------------------------------

describe("the inspector frame (§5)", () => {
  it("the heading is 48px and names what is selected", async () => {
    await openProject();
    const ws = useEditorWorkspaceStore();
    const w = mount(InspectorPanel);
    expect(heading(w).classes()).toContain("h-12");
    expect(heading(w).text()).toContain("Properties");

    ws.select(["c5"]);
    await flushPromises();
    expect(heading(w).text()).toContain("Clip properties");

    ws.select(["c1", "c2"]);
    await flushPromises();
    expect(heading(w).text()).toContain("Selection properties");

    ws.selectTrack("v3");
    await flushPromises();
    expect(heading(w).text()).toContain("Track properties");

    ws.select(["c1"]);
    ws.setSelected({ type: "effect", id: "fx1" });
    await flushPromises();
    expect(heading(w).text()).toContain("Teaching properties");
  });

  it("the ✕ hides the inspector through the panel rules (D5)", async () => {
    const ws = useEditorWorkspaceStore();
    ws.setViewport(1600, 1000);
    const w = mount(InspectorPanel);
    const hide = w.get('[data-testid="inspector-hide"]');
    expect(hide.attributes("aria-label")).toBe("Hide properties");
    expect(ws.inspectorVisible).toBe(true);
    await hide.trigger("click");
    expect(ws.propertiesHidden).toBe(true);
    expect(ws.inspectorVisible).toBe(false);

    // A drawer width closes the drawer instead.
    ws.setViewport(960, 640);
    ws.revealInspector();
    expect(ws.inspectorVisible).toBe(true);
    await hide.trigger("click");
    expect(ws.propertiesOpen).toBe(false);
  });

  it("the card is a 34px glyph tile, the clip's name and '{track} · {d.d}s'", async () => {
    await openProject();
    useEditorWorkspaceStore().select(["c5"]);
    const w = mount(InspectorPanel);
    const card = w.get('[data-testid="inspector-card"]');
    const glyph = card.get('[data-testid="inspector-card-glyph"]');
    expect(glyph.classes()).toEqual(expect.arrayContaining(["h-[34px]", "w-[34px]", "rounded-[7px]"]));
    expect(glyph.find("svg").exists()).toBe(true);
    const name = card.get('[data-testid="inspector-card-name"]');
    expect(name.text()).toBe("Presenter · demo");
    expect(name.classes()).toEqual(expect.arrayContaining(["text-[12px]", "font-[550]"]));
    expect(card.get('[data-testid="inspector-card-detail"]').text()).toBe("Webcam · presenter · 32.0s");
  });

  it("the tabs are a 3-column grid; the active one carries .active", async () => {
    await openProject();
    const ws = useEditorWorkspaceStore();
    ws.select(["c5"]);
    const w = mount(InspectorPanel);
    const list = w.get('[data-testid="inspector-tablist"]');
    expect(list.classes()).toEqual(expect.arrayContaining(["grid", "grid-cols-3", "bg-app", "border-line", "rounded-lg"]));
    expect(w.get('[data-testid="inspector-tab-clip"]').classes()).toContain("active");
    await w.get('[data-testid="inspector-tab-color"]').trigger("click");
    expect(w.get('[data-testid="inspector-tab-color"]').classes()).toContain("active");
    expect(w.get('[data-testid="inspector-tab-clip"]').classes()).not.toContain("active");
    expect(ws.propertyTab).toBe("color");
  });

  it("an audio clip shows Clip / Fades / Audio / Speed only, and a picture-only tab falls back to Clip", async () => {
    await openProject();
    const ws = useEditorWorkspaceStore();
    ws.setPropertyTab("layout");
    ws.select(["c6"]);
    const w = mount(InspectorPanel);
    const ids = w.findAll('[role="tab"]').map((t) => t.attributes("data-testid"));
    expect(ids).toEqual(["inspector-tab-clip", "inspector-tab-fades", "inspector-tab-audio", "inspector-tab-speed"]);
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="inspector-card-name"]').text()).toBe("Chapter cues · demo");
  });

  it("a clip on a locked track says so above the categories", async () => {
    await openProject(project({ tracks: [track("v3", "Webcam · presenter", "video", { locked: true })] }));
    useEditorWorkspaceStore().select(["c5"]);
    const w = mount(InspectorPanel);
    expect(w.get('[data-testid="inspector-locked"]').text()).toBe("Track locked. Unlock it using the padlock below.");
  });
});

// ---- nothing selected ---------------------------------------------------------

describe("nothing selected", () => {
  it("teaches the next step and states the project's facts", async () => {
    await openProject();
    const w = mount(InspectorPanel);
    const empty = w.get('[data-testid="inspector-empty"]');
    expect(empty.find("svg").exists()).toBe(true);
    expect(empty.text()).toContain("Select something to shape it.");
    expect(empty.text()).toContain("Choose a clip, annotation or track. Its controls will appear here.");
    expect(w.get('[data-testid="inspector-project-clips"]').text()).toBe("4 clips · 00:33.5");
    expect(w.get('[data-testid="inspector-project-canvas"]').text()).toBe("1280 × 720 · 30 fps");
    expect(w.findAll("[disabled]")).toHaveLength(0);
  });

  it("Review readiness opens the Checks dialog, the header button's own path", async () => {
    await openProject();
    const w = mount(InspectorPanel);
    await w.get('[data-testid="inspector-review-readiness"]').trigger("click");
    expect(checksDialogOpen.value).toBe(true);
  });

  it("one clip reads '1 clip'", async () => {
    await openProject(project({ clips: [clip("c1", "capture", "v1", 0, 9_500, "One")] }));
    const w = mount(InspectorPanel);
    expect(w.get('[data-testid="inspector-project-clips"]').text()).toMatch(/^1 clip · /);
  });
});

// ---- a track ------------------------------------------------------------------

describe("a selected track", () => {
  async function mountTrack(id: string, p?: Project) {
    await openProject(p);
    useEditorWorkspaceStore().selectTrack(id);
    return mount(InspectorPanel, { attachTo: document.body });
  }

  it("shows the track's card: layers for video, music for audio, with the concept's line", async () => {
    let w = await mountTrack("v3");
    expect(w.get('[data-testid="inspector-card-name"]').text()).toBe("Webcam · presenter");
    expect(w.get('[data-testid="inspector-card-detail"]').text()).toBe("Upper tracks appear in front");
    w.unmount();
    w = await mountTrack("a1");
    expect(w.get('[data-testid="inspector-card-detail"]').text()).toBe("Audio is mixed with other tracks");
  });

  it("Name commits renameTrack on Enter, and not when unchanged or empty", async () => {
    const w = await mountTrack("v3");
    const input = w.get<HTMLInputElement>('[data-testid="track-inspector-name"]');
    await input.setValue("   ");
    await input.trigger("keydown", { key: "Enter" });
    await input.setValue("Webcam · presenter");
    await input.trigger("blur");
    expect(executed).toEqual([]);
    await input.setValue("Presenter");
    await input.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toEqual([{ kind: "renameTrack", trackId: "v3", name: "Presenter" }]);
  });

  it("Escape reverts the name draft", async () => {
    const w = await mountTrack("v3");
    const input = w.get<HTMLInputElement>('[data-testid="track-inspector-name"]');
    await input.setValue("Draft");
    await input.trigger("keydown", { key: "Escape" });
    expect(input.element.value).toBe("Webcam · presenter");
    expect(executed).toEqual([]);
  });

  it("Track volume shows a percentage and commits setTrackFlags on release", async () => {
    const w = await mountTrack("v1");
    const range = w.get<HTMLInputElement>('[data-testid="track-inspector-volume"]');
    expect(range.attributes("max")).toBe("200");
    expect(w.get('[data-testid="track-inspector-volume-value"]').text()).toBe("100%");
    range.element.value = "150";
    await range.trigger("input");
    expect(w.get('[data-testid="track-inspector-volume-value"]').text()).toBe("150%");
    await range.trigger("change");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setTrackFlags", trackId: "v1", volume: 1.5 }]);
  });

  it("Up and Down send moveTrack; the edges say why they cannot", async () => {
    let w = await mountTrack("v3");
    const up = w.get('[data-testid="track-inspector-up"]');
    expect(up.attributes("aria-disabled")).toBe("true");
    expect(up.attributes("title")).toBe("Already the top track");
    await up.trigger("click");
    expect(executed).toEqual([]);
    await w.get('[data-testid="track-inspector-down"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "moveTrack", trackId: "v3", toIndex: 1 }]);
    w.unmount();

    w = await mountTrack("v9");
    expect(w.get('[data-testid="track-inspector-down"]').attributes("title")).toBe("Already the bottom track");
    await w.get('[data-testid="track-inspector-up"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "moveTrack", trackId: "v9", toIndex: 2 }]);
  });

  it("a locked track: every control but the name's reading says the lock reason and sends nothing", async () => {
    const locked = project({ tracks: [track("v3", "Webcam · presenter", "video", { locked: true }), track("v1", "Screen recording")] });
    const w = await mountTrack("v3", locked);
    const reason = "Track Webcam · presenter is locked";
    for (const id of ["track-inspector-up", "track-inspector-down", "track-inspector-remove"]) {
      const b = w.get(`[data-testid="${id}"]`);
      expect(b.attributes("aria-disabled")).toBe("true");
      expect(b.attributes("title")).toContain(reason);
      await b.trigger("click");
    }
    const name = w.get<HTMLInputElement>('[data-testid="track-inspector-name"]');
    expect(name.attributes("readonly")).toBeDefined();
    expect(name.attributes("title")).toContain(reason);
    await name.setValue("Renamed");
    await name.trigger("keydown", { key: "Enter" });
    const volume = w.get<HTMLInputElement>('[data-testid="track-inspector-volume"]');
    volume.element.value = "50";
    await volume.trigger("input");
    expect(volume.element.value).toBe("100");
    await volume.trigger("change");
    await flushPromises();
    expect(executed).toEqual([]);
    expect(trackRemovalRequest.value).toBeNull();
  });

  it("Remove track… on a track with clips asks first; Continue sends deleteTrack", async () => {
    const w = await mountTrack("v3");
    await w.get('[data-testid="track-inspector-remove"]').trigger("click");
    expect(executed).toEqual([]);
    expect(trackRemovalRequest.value).toEqual({ sessionId: "ses-a", trackId: "v3" });

    const dialog = mount(RemoveTrackDialog, { attachTo: document.body });
    await flushPromises();
    expect(dialog.text()).toContain("Remove this track?");
    expect(dialog.text()).toContain(
      "Remove “Webcam · presenter” and its 1 clip from this edit? Original media stays in the library. You can undo this.",
    );
    await dialog.get('[data-testid="remove-track-confirm"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "deleteTrack", trackId: "v3" }]);
    expect(trackRemovalRequest.value).toBeNull();
  });

  it("the question closes when its track is gone (an undo, a removal elsewhere)", async () => {
    await openProject();
    trackRemovalRequest.value = { sessionId: "ses-a", trackId: "v1" };
    const dialog = mount(RemoveTrackDialog, { attachTo: document.body });
    await flushPromises();
    expect(dialog.find('[data-testid="remove-track-dialog"]').exists()).toBe(true);
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        execute: () =>
          Promise.resolve({
            snapshot: { ...store.snapshot!, revision: 2 },
            project: { ...store.project!, tracks: store.project!.tracks.filter((t) => t.id !== "v1") },
          }),
      }),
    );
    await store.execute({ kind: "undo" });
    await flushPromises();
    expect(trackRemovalRequest.value).toBeNull();
    expect(dialog.find('[data-testid="remove-track-dialog"]').exists()).toBe(false);
  });

  it("Cancel keeps the track", async () => {
    await openProject();
    trackRemovalRequest.value = { sessionId: "ses-a", trackId: "v1" };
    const dialog = mount(RemoveTrackDialog, { attachTo: document.body });
    await flushPromises();
    expect(dialog.text()).toContain("its 2 clips");
    await dialog.get('[data-testid="remove-track-cancel"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
    expect(trackRemovalRequest.value).toBeNull();
  });

  it("an empty track is removed at once, with nothing to confirm (the concept's rule)", async () => {
    const w = await mountTrack("v9");
    await w.get('[data-testid="track-inspector-remove"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "deleteTrack", trackId: "v9" }]);
    expect(trackRemovalRequest.value).toBeNull();
  });

  it("the track menu's Remove track… shares the same confirm", async () => {
    await openProject();
    const ctxFor = useEditorMenuContext({ fitRange: () => {}, fitTimeline: () => {} });
    const store = useEditorProjectStore();
    const ctx = ctxFor(baseActionContext(store.project, store.snapshot, 0, []));
    findMenuAction(trackMenu(ctx, "v1"), "track-remove")?.run?.();
    await flushPromises();
    expect(executed).toEqual([]);
    expect(trackRemovalRequest.value).toEqual({ sessionId: "ses-a", trackId: "v1" });
  });

  it("a removed track leaves the track state", async () => {
    await openProject();
    const ws = useEditorWorkspaceStore();
    ws.selectTrack("v9");
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        execute: () =>
          Promise.resolve({
            snapshot: { ...store.snapshot!, revision: 2 },
            project: { ...store.project!, tracks: store.project!.tracks.filter((t) => t.id !== "v9") },
          }),
        saveWorkspace: () => Promise.resolve(),
      }),
    );
    await store.execute({ kind: "deleteTrack", trackId: "v9" });
    await flushPromises();
    expect(ws.selectedTrackId).toBeNull();
  });
});

// ---- several clips -------------------------------------------------------------

describe("several clips selected", () => {
  async function mountMulti(ids: string[], p?: Project) {
    await openProject(p);
    useEditorWorkspaceStore().select(ids);
    return mount(InspectorPanel, { attachTo: document.body });
  }

  it("states the count and lists the names", async () => {
    const w = await mountMulti(["c1", "c2", "c6"]);
    expect(w.get('[data-testid="inspector-card-name"]').text()).toBe("3 clips selected");
    expect(w.get('[data-testid="inspector-card-detail"]').text()).toBe("Shift-click to change selection");
    expect(w.get('[data-testid="multi-inspector-list"]').text()).toContain("Open your workspace");
    expect(w.get('[data-testid="multi-inspector-list"]').text()).toContain("Chapter cues · demo");
    expect(w.find('[role="tab"]').exists()).toBe(false);
  });

  it("Duplicate and Group run the registry actions", async () => {
    const w = await mountMulti(["c1", "c2"]);
    await w.get('[data-testid="multi-inspector-group"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="multi-inspector-duplicate"]').trigger("click");
    await flushPromises();
    expect(executed[0]).toEqual({ kind: "groupClips", clipIds: ["c1", "c2"] });
    expect(executed[1].kind).toBe("duplicateClips");
  });

  it("a grouped selection offers Ungroup instead", async () => {
    const grouped = project({
      clips: [
        clip("c1", "capture", "v1", 0, 9_500, "A", { group_id: "g1" }),
        clip("c2", "capture", "v1", 9_500, 14_000, "B", { group_id: "g1" }),
      ],
    });
    const w = await mountMulti(["c1", "c2"], grouped);
    const b = w.get('[data-testid="multi-inspector-group"]');
    expect(b.text()).toBe("Ungroup");
    expect(w.get('[data-testid="inspector-card-detail"]').text()).toBe("Grouped for synchronized movement");
    await b.trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "ungroupClips", groupId: "g1" }]);
  });

  it("Copy confirms what was copied and changes nothing", async () => {
    const w = await mountMulti(["c1", "c2"]);
    await w.get('[data-testid="multi-inspector-copy"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
    const { useNotificationsStore } = await import("../src/stores/notifications");
    expect(JSON.stringify(useNotificationsStore().$state)).toMatch(/Copied 2 clips/);
  });

  it("Shared fades send one setFades per clip, each within half its clip", async () => {
    const w = await mountMulti(["c1", "c2"]);
    const labels = ["None", "0.5s in & out", "1s in & out"];
    expect(w.findAll('[data-testid^="multi-inspector-fade-"]').map((b) => b.text())).toEqual(labels);
    await w.get('[data-testid="multi-inspector-fade-500"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "setFades", clipId: "c1", fadeInMs: 500, fadeOutMs: 500 },
      { kind: "setFades", clipId: "c2", fadeInMs: 500, fadeOutMs: 500 },
    ]);
  });

  it("Shared color offers the concept's five treatments and sends one setAdjustments", async () => {
    const w = await mountMulti(["c1", "c2"]);
    const tiles = w.findAll('[data-testid^="color-treatment-"]');
    expect(tiles.map((t) => t.text())).toEqual(["AaOriginal", "AaClear", "AaWarm", "AaSoft", "AaMono"]);
    expect(w.get('[data-testid="color-treatment-original"]').classes()).toContain("active");
    await w.get('[data-testid="color-treatment-mono"]').trigger("click");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect(executed[0]).toMatchObject({ kind: "setAdjustments", clipIds: ["c1", "c2"] });
  });

  it("Shared color is left out when no selected clip has a picture", async () => {
    const audioOnly = project({
      clips: [clip("c6", "music", "a1", 0, 1_000, "A"), clip("c7", "music", "a1", 2_000, 1_000, "B")],
    });
    const w = await mountMulti(["c6", "c7"], audioOnly);
    expect(w.find('[data-testid="color-treatment-original"]').exists()).toBe(false);
  });

  it("Mute all / Unmute all send setClipMix, and refuse when they would change nothing", async () => {
    const w = await mountMulti(["c1", "c2"]);
    const unmute = w.get('[data-testid="multi-inspector-unmute"]');
    expect(unmute.attributes("aria-disabled")).toBe("true");
    expect(unmute.attributes("title")).toBe("No selected clip is muted.");
    await unmute.trigger("click");
    await w.get('[data-testid="multi-inspector-mute"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setClipMix", clipIds: ["c1", "c2"], muted: true }]);
  });

  it("a locked clip in the selection: every edit says so and sends nothing; Copy still works", async () => {
    const locked = project({ tracks: [track("v1", "Screen recording", "video", { locked: true }), track("a1", "Guide cues", "audio")] });
    const w = await mountMulti(["c1", "c2"], locked);
    expect(w.get('[data-testid="multi-inspector-locked"]').text()).toBe("Unlock selected tracks before editing.");
    for (const id of ["multi-inspector-group", "multi-inspector-duplicate", "multi-inspector-fade-0", "multi-inspector-mute", "color-treatment-warm"]) {
      const b = w.get(`[data-testid="${id}"]`);
      expect(b.attributes("aria-disabled")).toBe("true");
      expect(b.attributes("title")).toContain("Track Screen recording is locked");
      await b.trigger("click");
    }
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="multi-inspector-copy"]').attributes("aria-disabled")).toBeUndefined();
  });
});

// ---- the track selection is view state -------------------------------------------

describe("editorWorkspace.selectedTrackId", () => {
  it("selecting a track clears clip and cue selection; any clip or cue selection clears it", async () => {
    const ws = useEditorWorkspaceStore();
    ws.select(["c1"]);
    ws.setSelected({ type: "effect", id: "fx1" });
    ws.selectTrack("v1");
    expect(ws.selectionClipIds).toEqual([]);
    expect(ws.selected).toBeNull();
    expect(ws.selectedTrackId).toBe("v1");

    ws.select(["c2"]);
    expect(ws.selectedTrackId).toBeNull();
    ws.selectTrack("v1");
    ws.setSelected({ type: "effect", id: "fx1" });
    expect(ws.selectedTrackId).toBeNull();
    ws.selectTrack("v1");
    ws.selectClipsOnly(["c1"]);
    expect(ws.selectedTrackId).toBeNull();
  });

  it("is never persisted and never an edit", async () => {
    await openProject();
    const saved: unknown[] = [];
    const ws = useEditorWorkspaceStore();
    ws.setPort(fakeEditorPort({ saveWorkspace: (_s, w) => (saved.push(w), Promise.resolve()) }));
    ws.sessionId = "ses-a";
    vi.useFakeTimers();
    try {
      ws.selectTrack("v1");
      vi.runAllTimers();
    } finally {
      vi.useRealTimers();
    }
    expect(executed).toEqual([]);
    expect(saved).toHaveLength(1);
    expect(JSON.stringify(saved)).not.toMatch(/v1/);
  });
});
