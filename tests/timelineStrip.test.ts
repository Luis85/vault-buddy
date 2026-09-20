import { enableAutoUnmount, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it } from "vitest";

import TimelineStrip from "../src/components/editor/TimelineStrip.vue";

const T = {
  segments: [
    { sourceStartMs: 0, sourceEndMs: 1000 },
    { sourceStartMs: 5000, sourceEndMs: 8000 },
  ],
};

// The strip installs a window-level `pointerup` listener so a drag released
// off the window still ends (see its own comment). A strip left mounted
// would answer a LATER test's window dispatch too, so every mount is torn
// down between tests rather than left to the file's end.
enableAutoUnmount(afterEach);

/** happy-dom gives every element a zero-size rect, so the component must read
 * the strip's width from `getBoundingClientRect` and the test must supply one
 * — otherwise the fraction is `NaN` and any assertion passes for the wrong
 * reason. 200px wide at the origin, so a clientX IS the percentage doubled. */
function sized(w: ReturnType<typeof mount>) {
  const strip = w.get('[data-testid="timeline-strip"]');
  strip.element.getBoundingClientRect = () =>
    ({ left: 0, width: 200, top: 0, height: 64, right: 200, bottom: 64, x: 0, y: 0 }) as DOMRect;
  return strip;
}

describe("TimelineStrip", () => {
  it("renders one block per segment, sized by its share of the output", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const blocks = w.findAll('[data-testid^="segment-"]');
    expect(blocks).toHaveLength(2);
    expect(blocks[0].attributes("style")).toContain("25%");
    expect(blocks[1].attributes("style")).toContain("75%");
  });

  it("marks the selected segment for assistive tech, not just visually", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: 1, playheadMs: 0 } });
    expect(w.get('[data-testid="segment-1"]').attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="segment-0"]').attributes("aria-pressed")).toBe("false");
  });

  it("emits the segment a click selected", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    await w.get('[data-testid="segment-1"]').trigger("click");
    expect(w.emitted("select")?.[0]).toEqual([1]);
  });

  // The playhead is positioned as a fraction of OUTPUT time. Positioning it
  // by source time would put it in the wrong place the moment anything is
  // cut, which is every moment the editor is useful.
  it("places the playhead by output fraction", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 2000 } });
    expect(w.get('[data-testid="playhead"]').attributes("style")).toContain("50%");
  });

  // Drag-to-reorder (spec 8.2). Pointer events, not HTML5 drag-and-drop:
  // Tauri intercepts the latter, which is why the task list's own reorder
  // is pointer-based too (AGENTS.md, the tasks domain).
  //
  // The payload is an insertion SLOT in [0, n], not a destination index in
  // [0, n) — `[0, 2]` on a two-segment timeline is "move the first block
  // past the last one", which is a real destination here. The container
  // converts before calling `useEditorTimeline.reorder`.
  it("emits a reorder when a block is dragged past another's midpoint", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = sized(w);
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    await strip.trigger("pointermove", { clientX: 180 });
    await strip.trigger("pointerup", { clientX: 180 });
    // 180/200 = 0.9, past block 1's midpoint (62.5%) -> slot 2 -> "last".
    expect(w.emitted("reorder")?.[0]).toEqual([0, 2]);
  });

  // Nothing here tracks pointer MOVEMENT — `onUp` reads the release point and
  // nothing else — so the property is positional: a block released anywhere
  // over its own place is a select, not a zero-distance reorder. A press that
  // never moved is the special case of that (the pointer is still over the
  // block it went down on), which is why this needs no separate fixture.
  //
  // Both halves are pinned separately: the guard is two comparisons, and a
  // fixture for only one of them leaves a mutation that drops the other one
  // green.
  it("does not emit a reorder when a block is released in the front half of its own place", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = sized(w);
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    // 10/200 = 5%, short of block 0's own midpoint (12.5%) -> slot 0 == from.
    await strip.trigger("pointerup", { clientX: 10 });
    expect(w.emitted("reorder")).toBeUndefined();
  });

  it("does not emit a reorder when a block is released in the back half of its own place", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = sized(w);
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    // 40/200 = 20%, past block 0's midpoint but short of block 1's
    // (62.5%) -> slot 1 == from + 1, which is still where it already is.
    await strip.trigger("pointerup", { clientX: 40 });
    expect(w.emitted("reorder")).toBeUndefined();
  });

  // Releasing off the window is a cancelled drag, not a drop at the nearest
  // end: `onUp` lives on the strip, so without the window listener `dragFrom`
  // would stay set and the NEXT release over the strip's own padding — a
  // pointerup with no pointerdown on a block — would reorder a block the user
  // is no longer dragging.
  it("cancels a drag released outside the strip instead of dropping it at an end", async () => {
    const w = mount(TimelineStrip, {
      props: { timeline: T, selected: null, playheadMs: 0 },
      attachTo: document.body,
    });
    const strip = sized(w);
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    window.dispatchEvent(new MouseEvent("pointerup", { clientX: 500 }));
    expect(w.emitted("reorder")).toBeUndefined();
    // …and the drag is over, so a later release over the strip is inert.
    await strip.trigger("pointerup", { clientX: 180 });
    expect(w.emitted("reorder")).toBeUndefined();
  });

  // The pointer was taken away mid-drag (an OS gesture, a lost capture).
  // RegionRoot answers the same event for the same reason: without this the
  // drag would still be "live" and the next release would commit it.
  it("abandons a drag the OS cancelled", async () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = sized(w);
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    await strip.trigger("pointercancel");
    await strip.trigger("pointerup", { clientX: 180 });
    expect(w.emitted("reorder")).toBeUndefined();
  });

  // The copy named a delete that cannot have happened — `deleteSegment`
  // refuses to remove the last segment, so this state is reachable only from
  // a zero-duration capture or a hand-edited sidecar — and a "revert the
  // edit" verb the editor does not offer at all (the phase review's m-1).
  it("renders nothing but an empty state when there are no segments", () => {
    const w = mount(TimelineStrip, {
      props: { timeline: { segments: [] }, selected: null, playheadMs: 0 },
    });
    expect(w.findAll('[data-testid^="segment-"]')).toHaveLength(0);
    const empty = w.get('[data-testid="strip-empty"]').text();
    expect(empty).toContain("no footage to show");
    expect(empty).not.toMatch(/revert|undo/i);
  });

  // m-4. `segmentWidths` returns percentages of the WHOLE strip, the playhead
  // is `left: X%` of the same box, and `onUp` turns a pointer position into a
  // fraction of it — so all three are only true while the blocks tile that
  // box exactly. Padding inset them (the playhead never lined up with the
  // boundary it marks) and a gap shrank each block below its nominal width,
  // so the rendered geometry drifted from the geometry `dropIndex` computes
  // against, by more with every split. happy-dom lays nothing out, so this is
  // asserted structurally: it is the only place the contract can be pinned
  // without a real browser, and the manual checklist (row 27) carries the
  // visual half.
  it("lets the blocks tile the strip exactly, with no padding or gap", () => {
    const w = mount(TimelineStrip, { props: { timeline: T, selected: null, playheadMs: 0 } });
    const strip = w.get('[data-testid="timeline-strip"]');
    expect(strip.classes().filter((c) => /^p-|^px-|^pl-|^pr-/.test(c))).toEqual([]);
    const row = strip.element.querySelector("div.flex") as HTMLElement;
    expect([...row.classList].filter((c) => c.startsWith("gap-"))).toEqual([]);
  });
});
