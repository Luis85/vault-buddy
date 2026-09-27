/**
 * Task 13's fix round 1 (visual-parity): what several selected clips can
 * still do together, and the edges the first round left open.
 *
 * - "Adjust all" (ruling T13-1): Layout and Color reach the whole selection
 *   again — one atomic `setLayout` / `setAdjustments` (F-23) — when every
 *   clip is on a video track.
 * - Colour is refused, in the menu and the inspector alike, for a selection
 *   Rust refuses (`check_color_targets`: a clip off a video track, a title
 *   card).
 * - The guide's Layout and Fades lessons always find their tab.
 * - A pending "Remove this track?" belongs to its session.
 * - The ✕ hands focus to the control that brings the panel back.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import RemoveTrackDialog from "../src/components/editor/dialogs/RemoveTrackDialog.vue";
import ColorSection from "../src/components/editor/inspector/ColorSection.vue";
import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import LayoutSection from "../src/components/editor/inspector/LayoutSection.vue";
import { useEditorMenuContext } from "../src/composables/useEditorMenuContext";
import { findMenuAction } from "../src/composables/useInspectorMenu";
import { baseActionContext } from "../src/editor/actionContext";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { prepareLesson } from "../src/editor/guide/prepare";
import { multiClipMenu } from "../src/editor/menuSetsClip";
import { PROPERTIES_TOGGLE_ID } from "../src/editor/previewHeader";
import { trackRemovalRequest } from "../src/editor/trackRemoval";
import { CARD_NOTE, VISUAL_NOTE } from "../src/editor/visualTargets";
import type { Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { clip, openInspectorProject, project } from "./helpers/inspectorProject";

enableAutoUnmount(afterEach);

let executed: EditorCommand[];

beforeEach(() => {
  setActivePinia(createPinia());
  trackRemovalRequest.value = null;
});

async function open(p: Project = project(), sessionId = "ses-a") {
  executed = await openInspectorProject(p, sessionId);
}

/** The root's own Layout and Color sections in the panel's named slots. */
function mountPanel() {
  return mount(InspectorPanel, {
    attachTo: document.body,
    global: { components: { LayoutSection, ColorSection } },
    slots: {
      layout: '<template #layout="{ clipIds }"><LayoutSection :clip-ids="clipIds" /></template>',
      color: '<template #color="{ clipIds }"><ColorSection :clip-ids="clipIds" /></template>',
    },
  });
}

function colorItemFor(ids: string[]) {
  const store = useEditorProjectStore();
  const ctx = useEditorMenuContext({ fitRange: () => {}, fitTimeline: () => {} })(
    baseActionContext(store.project, store.snapshot, 0, ids),
  );
  return findMenuAction(multiClipMenu(ctx), "color");
}

const withCard = () =>
  project({
    assets: [
      { id: "capture", kind: "video", name: "Getting started.capture", duration_ms: 36_000 },
      { id: "card", kind: "video", name: "Title", duration_ms: 3_000, builtin: "card" },
    ],
    clips: [clip("c1", "capture", "v1", 0, 9_500, "A"), clip("k1", "card", "v3", 0, 3_000, "Intro")],
  });

describe("Adjust all (ruling T13-1)", () => {
  it("Layout reaches every selected clip in one setLayout", async () => {
    await open();
    useEditorWorkspaceStore().select(["c1", "c2"]);
    const w = mountPanel();
    const tabs = w.findAll('[role="tab"]').map((t) => t.attributes("data-testid"));
    expect(tabs).toEqual(["inspector-tab-layout", "inspector-tab-color"]);
    await w.get('[data-testid="layout-corner-tr"]').trigger("click");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect(executed[0]).toMatchObject({ kind: "setLayout", clipIds: ["c1", "c2"] });
  });

  it("Color reaches every selected clip in one setAdjustments", async () => {
    await open();
    useEditorWorkspaceStore().select(["c1", "c2"]);
    const w = mountPanel();
    await w.get('[data-testid="inspector-tab-color"]').trigger("click");
    await w.get('[data-testid="color-preset-mono"]').trigger("click");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect(executed[0]).toMatchObject({ kind: "setAdjustments", clipIds: ["c1", "c2"] });
  });

  it("is not offered when a selected clip is off a video track", async () => {
    await open();
    useEditorWorkspaceStore().select(["c1", "c6"]);
    const w = mountPanel();
    expect(w.find('[data-testid="inspector-tablist"]').exists()).toBe(false);
    expect(w.find('[data-testid="layout-section"]').exists()).toBe(false);
  });
});

describe("colour for a selection Rust refuses", () => {
  it("a clip off a video track: Shared color is left out and the menu item says why", async () => {
    await open();
    useEditorWorkspaceStore().select(["c1", "c6"]);
    const w = mountPanel();
    expect(w.find('[data-testid="color-treatment-original"]').exists()).toBe(false);
    expect(colorItemFor(["c1", "c6"])?.disabledReason).toBe(VISUAL_NOTE);
  });

  it("a title card: the tiles and the menu item carry the card refusal and send nothing", async () => {
    await open(withCard());
    useEditorWorkspaceStore().select(["c1", "k1"]);
    const w = mountPanel();
    const tile = w.get('[data-testid="color-treatment-warm"]');
    expect(tile.attributes("aria-disabled")).toBe("true");
    expect(tile.attributes("title")).toBe(CARD_NOTE);
    await tile.trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
    expect(colorItemFor(["c1", "k1"])?.disabledReason).toBe(CARD_NOTE);
  });

  it("footage only: the item is enabled", async () => {
    await open();
    expect(colorItemFor(["c1", "c2"])?.disabledReason ?? null).toBeNull();
  });
});

describe("the guide's Layout and Fades lessons find their tab", () => {
  const audioFirst = () =>
    project({ clips: [clip("c6", "music", "a1", 0, 5_000, "Bed"), clip("c1", "capture", "v1", 500, 9_500, "A")] });

  it("from several clips: one clip that offers the tab", async () => {
    await open();
    const ws = useEditorWorkspaceStore();
    ws.select(["c1", "c2", "c6"]);
    prepareLesson("inspector.layout", ws, useEditorProjectStore().project);
    expect(ws.selectionClipIds).toEqual(["c1"]);
    expect(ws.propertyTab).toBe("layout");
    const w = mountPanel();
    expect(w.find('[data-testid="inspector-tab-layout"]').exists()).toBe(true);
  });

  it("in an audio-first project: Layout picks the earliest picture clip, Fades any clip", async () => {
    await open(audioFirst());
    const ws = useEditorWorkspaceStore();
    prepareLesson("inspector.layout", ws, useEditorProjectStore().project);
    expect(ws.selectionClipIds).toEqual(["c1"]);
    ws.select([]);
    prepareLesson("inspector.fades", ws, useEditorProjectStore().project);
    expect(ws.selectionClipIds).toEqual(["c6"]);
  });

  it("a selected sound clip gives way to a picture clip for Layout; a fitting one is kept", async () => {
    await open(audioFirst());
    const ws = useEditorWorkspaceStore();
    ws.select(["c6"]);
    prepareLesson("inspector.fades", ws, useEditorProjectStore().project);
    expect(ws.selectionClipIds).toEqual(["c6"]);
    prepareLesson("inspector.layout", ws, useEditorProjectStore().project);
    expect(ws.selectionClipIds).toEqual(["c1"]);
  });

  it("a cue selected over the clip is set aside, so the tabs show", async () => {
    await open();
    const ws = useEditorWorkspaceStore();
    ws.select(["c1"]);
    ws.setSelected({ type: "effect", id: "fx1" });
    prepareLesson("inspector.fades", ws, useEditorProjectStore().project);
    expect(ws.selectionClipIds).toEqual(["c1"]);
    expect(ws.selected).toBeNull();
  });
});

describe("a pending track removal belongs to its session", () => {
  it("a question from a closed session is never put to the next one", async () => {
    await open();
    trackRemovalRequest.value = { sessionId: "ses-a", trackId: "v1" };
    const dialog = mount(RemoveTrackDialog, { attachTo: document.body });
    await flushPromises();
    expect(dialog.find('[data-testid="remove-track-dialog"]').exists()).toBe(true);

    // The next project has a track with the same id.
    await open(project(), "ses-b");
    await flushPromises();
    expect(dialog.find('[data-testid="remove-track-dialog"]').exists()).toBe(false);
    expect(trackRemovalRequest.value).toBeNull();
  });

  it("a stale question found on mount is dropped", async () => {
    await open(project(), "ses-b");
    trackRemovalRequest.value = { sessionId: "ses-a", trackId: "v1" };
    const dialog = mount(RemoveTrackDialog, { attachTo: document.body });
    await flushPromises();
    expect(dialog.find('[data-testid="remove-track-dialog"]').exists()).toBe(false);
    expect(trackRemovalRequest.value).toBeNull();
  });
});

describe("the ✕", () => {
  it("returns focus to the properties toggle once the panel is hidden", async () => {
    const toggle = document.createElement("button");
    toggle.id = PROPERTIES_TOGGLE_ID;
    document.body.append(toggle);
    try {
      const ws = useEditorWorkspaceStore();
      ws.setViewport(1600, 1000);
      const w = mount(InspectorPanel, { attachTo: document.body });
      const hide = w.get<HTMLButtonElement>('[data-testid="inspector-hide"]');
      hide.element.focus();
      await hide.trigger("click");
      await flushPromises();
      expect(ws.inspectorVisible).toBe(false);
      expect(document.activeElement).toBe(toggle);
    } finally {
      toggle.remove();
    }
  });
});
