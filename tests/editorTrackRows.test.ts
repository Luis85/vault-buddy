/**
 * The timeline's track rows as a whole (visual-parity Task 17; concept spec
 * §6.4, §1.4; design D12, ruling P4): the badges, the label column's width
 * at each window width (and every conversion that takes it back out), the
 * track menu from a header — by right-click or Shift+F10, Escape giving
 * focus back — its Remove track… through the one confirm, and the badge
 * showing Track properties. Pinning itself is measured in real Chromium
 * (`tests/e2e/editorParity.spec.ts`): happy-dom has no layout.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { revealScrollLeft, trackBadges, trackLabelWidthAt } from "../src/editor/timelineLayout";
import { trackRemovalRequest } from "../src/editor/trackRemoval";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { openInspectorProject, project, track } from "./helpers/inspectorProject";

enableAutoUnmount(afterEach);

let executed: EditorCommand[] = [];

beforeEach(async () => {
  setActivePinia(createPinia());
  trackRemovalRequest.value = null;
  executed = await openInspectorProject();
});

async function timeline(windowWidth = 1600) {
  useEditorWorkspaceStore().setViewport(windowWidth, 1000);
  const w = mount(TimelineView, { attachTo: document.body, props: { viewportWidth: 1_000 } });
  await flushPromises();
  return w;
}

const menu = () => document.querySelector('[data-testid="editor-context-menu"]');

describe("track badges (§6.4)", () => {
  it("video tracks count down to V1 top to bottom; audio tracks count up from A1", () => {
    const tracks = [track("v3", "a"), track("v1", "b"), track("a1", "c", "audio"), track("v9", "d"), track("a2", "e", "audio")];
    expect(trackBadges(tracks)).toEqual({ v3: "V3", v1: "V2", a1: "A1", v9: "V1", a2: "A2" });
    expect(trackBadges([])).toEqual({});
  });

  it("each header shows its own badge", async () => {
    const w = await timeline();
    const badges = ["v3", "v1", "a1", "v9"].map((id) => w.get(`[data-testid="track-header-${id}-badge"]`).text());
    expect(badges).toEqual(["V3", "V2", "A1", "V1"]);
  });
});

describe("the label column's width (§1.4)", () => {
  it("is 196, 184 at or below 1200px wide and 174 at or below 1000px", () => {
    expect([1600, 1201, 1200, 1001, 1000, 960].map(trackLabelWidthAt)).toEqual([196, 196, 184, 184, 174, 174]);
  });

  it.each([
    [1600, 196],
    [1100, 184],
    [960, 174],
  ])("at %ipx the ruler cell, every header and the playhead use %ipx", async (width, label) => {
    useEditorWorkspaceStore().setPlayhead(2_000); // 100px at zoom 1
    const w = await timeline(width);
    expect(w.get('[data-testid="timeline-ruler-label"]').attributes("style")).toContain(`width: ${label}px`);
    for (const id of ["v3", "v1", "a1", "v9"]) {
      expect(w.get(`[data-testid="track-lane-header-${id}"]`).attributes("style")).toContain(`width: ${label}px`);
    }
    expect(w.get('[data-testid="timeline-playhead"]').attributes("style")).toContain(`left: ${label + 100}px`);
  });

  it("a pointer's time takes the narrower column back out", async () => {
    const w = await timeline(960);
    await w.get('[data-testid="track-lane-body-v9"]').trigger("contextmenu", { clientX: 174 + 250, clientY: 5 });
    await w.get('[data-testid="editor-context-menu-item-lane-title"]').trigger("click");
    await flushPromises();
    expect(executed).toContainEqual(expect.objectContaining({ kind: "addCard", trackId: "v9", startMs: 5_000 }));
  });

  it("a revealed instant is judged against the column's own width", () => {
    expect(revealScrollLeft(2_000, 1, 0, 400, 174)).toBeNull(); // x 274, inside [174, 400]
    expect(revealScrollLeft(6_000, 1, 0, 400, 174)).toBe(225); // x 474 -> 300 - 226/3
    expect(revealScrollLeft(6_000, 1, 0, 400)).toBe(232); // the full 196px column
  });
});

describe("the track menu from a header (ruling P4)", () => {
  it("a right-click on the header opens the track's own menu, headed by its name", async () => {
    const w = await timeline();
    await w.get('[data-testid="track-lane-header-v1"]').trigger("contextmenu", { clientX: 30, clientY: 200 });
    await flushPromises();
    expect(menu()).not.toBeNull();
    expect(document.querySelector('[data-testid="editor-context-menu-heading"]')?.textContent).toContain("Screen recording");
    const present = ["track-select", "track-rename", "track-visible", "track-up", "track-down", "track-remove"].filter(
      (id) => document.querySelector(`[data-testid="editor-context-menu-item-${id}"]`) !== null,
    );
    expect(present).toHaveLength(6);
  });

  it("Shift+F10 on the focused badge opens it; Escape closes it and gives focus back", async () => {
    const w = await timeline();
    const badge = w.get('[data-testid="track-header-a1-badge"]');
    (badge.element as HTMLElement).focus();
    await badge.trigger("keydown", { key: "F10", shiftKey: true });
    await flushPromises();
    expect(document.querySelector('[data-testid="editor-context-menu-heading"]')?.textContent).toContain("Guide cues");
    (document.activeElement ?? document.body).dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    await flushPromises();
    expect(menu()).toBeNull();
    expect(document.activeElement?.getAttribute("data-testid")).toBe("track-header-a1-badge");
  });

  it("Remove track… on a track with clips asks first, through the one confirm, and sends nothing yet", async () => {
    const w = await timeline();
    await w.get('[data-testid="track-lane-header-v1"]').trigger("contextmenu", { clientX: 30, clientY: 200 });
    await flushPromises();
    (document.querySelector('[data-testid="editor-context-menu-item-track-remove"]') as HTMLElement).click();
    await flushPromises();
    expect(trackRemovalRequest.value).toEqual({ sessionId: "ses-a", trackId: "v1" });
    expect(executed).toEqual([]);
  });

  it("Remove track… on an empty track removes it at once", async () => {
    const w = await timeline();
    await w.get('[data-testid="track-lane-header-v9"]').trigger("contextmenu", { clientX: 30, clientY: 200 });
    await flushPromises();
    (document.querySelector('[data-testid="editor-context-menu-item-track-remove"]') as HTMLElement).click();
    await flushPromises();
    expect(executed).toEqual([{ kind: "deleteTrack", trackId: "v9" }]);
  });
});

describe("the badge shows Track properties (Task 13's inspector)", () => {
  it("the empty inspector's promise holds: a badge click selects the track and the inspector reads Track properties", async () => {
    const inspector = mount(InspectorPanel);
    await flushPromises();
    expect(inspector.get('[data-testid="inspector-title"]').text()).toBe("Properties");
    const w = await timeline();
    await w.get('[data-testid="track-header-v3-badge"]').trigger("click");
    await flushPromises();
    expect(inspector.get('[data-testid="inspector-title"]').text()).toBe("Track properties");
    expect(inspector.text()).toContain("Webcam · presenter");
  });
});

describe("twenty tracks", () => {
  it("every row renders with its own pinned header", async () => {
    const tracks = Array.from({ length: 20 }, (_, i) => track(`t${i}`, `Track ${i}`, i < 12 ? "video" : "audio"));
    setActivePinia(createPinia());
    await openInspectorProject(project({ tracks, clips: [] }));
    const w = await timeline();
    const cells = w.findAll('[data-testid^="track-lane-header-"]');
    expect(cells).toHaveLength(20);
    for (const cell of cells) expect(cell.classes()).toEqual(expect.arrayContaining(["sticky", "left-0"]));
    expect(w.get('[data-testid="track-header-t0-badge"]').text()).toBe("V12");
    expect(w.get('[data-testid="track-header-t19-badge"]').text()).toBe("A8");
  });
});
