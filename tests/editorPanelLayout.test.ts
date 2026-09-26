/**
 * The editor frame's panel state (visual-parity Task 4; design D4, D5;
 * concept spec §1.3, §1.4, §6.1). `editorWorkspace` owns whether the media
 * library and the properties inspector are shown, Focus preview, and the
 * timeline's height — and each toggle is a real grid state at EVERY width:
 * a column that collapses to 0 above the drawer breakpoints, a drawer that
 * opens and closes below them (audit finding 2: the toggles used to flip a
 * hidden ref at full width and change nothing).
 */
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { clampTimelineHeight, timelineHeightRange } from "../src/editor/panelLayout";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

beforeEach(() => {
  setActivePinia(createPinia());
});

function at(width: number, height = 1000) {
  const ws = useEditorWorkspaceStore();
  ws.setViewport(width, height);
  return ws;
}

describe("panel toggles at full width (> 1080): the columns collapse to 0", () => {
  it("shows both panels by default, as columns", () => {
    const ws = at(1600);
    expect(ws.libraryVisible).toBe(true);
    expect(ws.inspectorVisible).toBe(true);
    expect(ws.libraryIsDrawer).toBe(false);
    expect(ws.inspectorIsDrawer).toBe(false);
  });

  it("the library toggle hides and re-shows the library column, persisted as library_hidden", () => {
    const ws = at(1600);
    ws.toggleLibrary();
    expect(ws.libraryVisible).toBe(false);
    expect(ws.libraryHidden).toBe(true);
    expect(ws.inspectorVisible).toBe(true);
    ws.toggleLibrary();
    expect(ws.libraryVisible).toBe(true);
    expect(ws.libraryHidden).toBe(false);
  });

  it("the properties toggle hides the inspector column, persisted as properties_hidden", () => {
    const ws = at(1600);
    ws.toggleInspector();
    expect(ws.inspectorVisible).toBe(false);
    expect(ws.propertiesHidden).toBe(true);
    expect(ws.libraryVisible).toBe(true);
  });

  it("Focus preview hides both; a panel toggle afterwards leaves focus and shows that panel", () => {
    const ws = at(1600);
    ws.toggleFocusPreview();
    expect(ws.focusPreview).toBe(true);
    expect(ws.libraryVisible).toBe(false);
    expect(ws.inspectorVisible).toBe(false);

    ws.toggleLibrary();
    expect(ws.focusPreview).toBe(false);
    expect(ws.libraryVisible).toBe(true);
    expect(ws.inspectorVisible).toBe(true);
  });
});

describe("panel toggles at 861–1080: the inspector is a drawer, the library a column", () => {
  it("the inspector drawer starts closed and the toggle opens it", () => {
    const ws = at(960, 640);
    expect(ws.inspectorIsDrawer).toBe(true);
    expect(ws.libraryIsDrawer).toBe(false);
    expect(ws.inspectorVisible).toBe(false);
    ws.toggleInspector();
    expect(ws.inspectorVisible).toBe(true);
    expect(ws.propertiesOpen).toBe(true);
    // The column preference is not what a drawer toggle changes.
    expect(ws.propertiesHidden).toBe(false);
  });

  it("the library toggle still collapses the library column", () => {
    const ws = at(960, 640);
    expect(ws.libraryVisible).toBe(true);
    ws.toggleLibrary();
    expect(ws.libraryVisible).toBe(false);
  });
});

describe("panel toggles at ≤ 860: both panels are drawers", () => {
  it("the library drawer starts closed; opening it closes the inspector drawer, and back", () => {
    const ws = at(820, 640);
    expect(ws.libraryIsDrawer).toBe(true);
    expect(ws.libraryVisible).toBe(false);

    ws.toggleInspector();
    expect(ws.inspectorVisible).toBe(true);
    ws.toggleLibrary();
    expect(ws.libraryVisible).toBe(true);
    expect(ws.inspectorVisible).toBe(false);
    ws.toggleInspector();
    expect(ws.inspectorVisible).toBe(true);
    expect(ws.libraryVisible).toBe(false);
  });

  it("Focus preview closes an open drawer", () => {
    const ws = at(820, 640);
    ws.toggleLibrary();
    ws.toggleFocusPreview();
    expect(ws.libraryVisible).toBe(false);
    expect(ws.libraryDrawerOpen).toBe(false);
  });
});

describe("reveals (a finding or a guide lesson asks for a panel)", () => {
  it("un-hides a hidden library column and leaves Focus preview", () => {
    const ws = at(1600);
    ws.toggleLibrary();
    ws.toggleFocusPreview();
    ws.revealLibrary();
    expect(ws.libraryVisible).toBe(true);
    expect(ws.focusPreview).toBe(false);
  });

  it("opens the drawer where the panel is one", () => {
    const ws = at(820, 640);
    ws.revealInspector();
    expect(ws.inspectorVisible).toBe(true);
    ws.revealLibrary();
    expect(ws.libraryVisible).toBe(true);
  });
});

describe("the window height", () => {
  it("is short at ≤ 760", () => {
    expect(at(1600, 760).shortWindow).toBe(true);
    expect(at(1600, 761).shortWindow).toBe(false);
  });
});

describe("timeline height (§1.3, §6.1): 400 by default, clamped 170 … min(540, innerHeight − 370)", () => {
  it("the range follows the window height", () => {
    expect(timelineHeightRange(1000)).toEqual([170, 540]);
    expect(timelineHeightRange(640)).toEqual([170, 270]);
    expect(timelineHeightRange(400)).toEqual([170, 170]);
    expect(clampTimelineHeight(400, 640)).toBe(270);
    expect(clampTimelineHeight(5, 1000)).toBe(170);
  });

  it("defaults to 400, shown clamped to the window", () => {
    expect(at(1600, 1000).timelineHeightPx).toBe(400);
    expect(at(960, 640).timelineHeightPx).toBe(270);
  });

  it("setTimelineHeight clamps to the current window", () => {
    const ws = at(1600, 1000);
    ws.setTimelineHeight(900);
    expect(ws.timelineHeight).toBe(540);
    ws.setTimelineHeight(5);
    expect(ws.timelineHeight).toBe(170);
    ws.setViewport(960, 640);
    ws.setTimelineHeight(500);
    expect(ws.timelineHeight).toBe(270);
  });
});
