/**
 * The editor window's LAYOUT CONTRACT: the preview absorbs the window's
 * slack, and nothing below it can be squeezed or pushed out of reach.
 *
 * Both defects this file pins were reported from one real session on a
 * 3840x2400 display, and both are the same root cause: `EditorRoot` is a
 * fixed `h-screen` flex column, and the preview `<video>` was `w-full` with
 * NO height bound. A video sizes itself from its width at its own aspect
 * ratio, so widening the window made it TALLER, and the column had more
 * content than window:
 *
 *  1. The timeline strip collapsed to a line. `h-16` is a height, not a
 *     minimum, and a flex item whose content has no intrinsic height shrinks
 *     to nothing long before its siblings do. The user watched the strip
 *     thin out as they dragged the window wider.
 *  2. Maximised, Save to vault and Discard were pushed past the bottom edge
 *     with nothing to scroll -- the capture could be edited and never saved.
 *
 * HONESTY ABOUT WHAT THESE PROVE. happy-dom has no layout engine: nothing
 * here measures a pixel, and a test that claimed to would be the kind this
 * repo keeps finding. They assert the CLASS CONTRACT that produces the
 * layout, which is the mechanism itself -- `flex-1`+`min-h-0` on the one
 * element allowed to take the slack, `shrink-0` on every element that must
 * keep its size. The real proof is verification-checklist row 37.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const listeners: Record<string, (e?: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e?: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { open, THREE, video } from "./helpers/editorMount";

enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const key of Object.keys(listeners)) delete listeners[key];
  mockConvertFileSrc("windows");
  // Task 15: EditorRoot (mounted by `open()` below) now calls
  // `useEditorProjectStore()`.
  setActivePinia(createPinia());
});

/** Every class on an element, as a set, so order never matters. */
function classes(el: Element | null | undefined) {
  return new Set((el?.className ?? "").split(/\s+/).filter(Boolean));
}

describe("the editor's layout contract", () => {
  it("lets the video take the leftover height instead of dictating it", async () => {
    const w = await open(THREE);

    const v = classes(video(w));
    // `flex-1` + `min-h-0`: take what is left, and be allowed to give it
    // back. Without `min-h-0` a flex item refuses to shrink below its
    // content, which for a video is its full intrinsic height -- the exact
    // overflow this pair exists to prevent.
    expect(v).toContain("flex-1");
    expect(v).toContain("min-h-0");
    // `object-contain` is BELT, not braces, and this comment says so because
    // its first version did not: it claimed a bounded box plus `w-full`
    // would stretch the picture. It would not. Chromium's UA stylesheet
    // already sets `object-fit: contain` on every `<video>` (probed in a
    // real browser, not assumed), so the class changes nothing today. It is
    // pinned anyway because the default is the BROWSER's, not ours, and a
    // stated intent survives a UA change or an `object-fit` rule arriving
    // from somewhere else in the cascade. The `bg-black` beside it is what
    // makes the letterbox bars read as deliberate.
    expect(v).toContain("object-contain");
  });

  it("never shrinks the strip, the verbs or the export bar", async () => {
    const w = await open(THREE);

    // The strip is the one the user actually watched collapse.
    expect(classes(w.get("[data-testid='timeline-strip']").element)).toContain(
      "shrink-0",
    );
    // The verb row and the export bar are what got pushed off the bottom.
    // What must not shrink is the FLEX ITEM -- the direct child of `main` --
    // not whatever element happens to wrap the button today, so each is
    // found by walking up from a button it contains. That keeps the
    // assertion on the mechanism while the markup around it stays free.
    for (const testid of ["editor-split", "export-save"]) {
      const column = w.get("main").element;
      let item = w.get(`[data-testid='${testid}']`).element;
      while (item.parentElement && item.parentElement !== column) {
        item = item.parentElement;
      }
      expect(item.parentElement, `${testid} is not inside main`).toBe(column);
      expect(classes(item), `${testid}'s column item can be squeezed`).toContain(
        "shrink-0",
      );
    }
  });

  it("scrolls rather than hiding the save when even the chrome will not fit", async () => {
    const w = await open(THREE);

    // The preview can give up all of its height, so this is reachable only
    // when the header, strip, verbs and bar alone exceed the window -- a
    // very short window, or a long error banner. Losing the save there would
    // be the same defect in a smaller window, so the column scrolls.
    expect(classes(w.get("main").element)).toContain("overflow-y-auto");
  });
});
