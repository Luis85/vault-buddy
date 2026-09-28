/**
 * `disabledAnnouncer.ts` (final review, minor 7): the ONE rule for a
 * disabled control's activation — it says its reason, and a held key
 * (which repeats the activation) says it at most once every 1.5 s per
 * reason, while a different reason still speaks at once.
 */
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ColorTreatmentGrid from "../src/components/editor/inspector/ColorTreatmentGrid.vue";
import InspectorButton from "../src/components/editor/inspector/InspectorButton.vue";
import TrackHeader from "../src/components/editor/timeline/TrackHeader.vue";
import { lockedReason } from "../src/editor/actionMeta";
import { announceDisabled } from "../src/editor/disabledAnnouncer";
import type { Track } from "../src/editorTypes";
import { useNotificationsStore } from "../src/stores/notifications";

const said = () => useNotificationsStore().items.map((i) => i.message);

beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(10_000);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("announceDisabled", () => {
  it("says a reason once per 1.5 s, however often it is pressed", () => {
    announceDisabled("Already at the start.");
    useNotificationsStore().clear();
    for (let i = 0; i < 10; i += 1) announceDisabled("Already at the start.");
    vi.setSystemTime(11_400);
    announceDisabled("Already at the start.");
    expect(said()).toEqual([]);
    vi.setSystemTime(11_600);
    announceDisabled("Already at the start.");
    expect(said()).toEqual(["Already at the start."]);
  });

  it("a different reason is said at once", () => {
    announceDisabled("Already at the start.");
    announceDisabled("Select a clip first");
    expect(said()).toEqual(["Already at the start.", "Select a clip first"]);
  });

  it("no reason says nothing", () => {
    announceDisabled(null);
    announceDisabled("");
    expect(said()).toEqual([]);
  });

  it("each Pinia (each editor window, each test) keeps its own clock", () => {
    announceDisabled("Zoomed all the way in");
    setActivePinia(createPinia());
    announceDisabled("Zoomed all the way in");
    expect(said()).toEqual(["Zoomed all the way in"]);
  });
});

// The controls that were `aria-disabled` and said nothing when pressed now
// say their reason through the same announcer.
describe("a refused control says why when pressed", () => {
  it("InspectorButton", async () => {
    const w = mount(InspectorButton, { props: { reason: "Already the top track" }, slots: { default: "Up" } });
    await w.trigger("click");
    expect(w.emitted("click")).toBeUndefined();
    expect(said()).toEqual(["Already the top track"]);
  });

  it("ColorTreatmentGrid", async () => {
    const w = mount(ColorTreatmentGrid, { props: { activeId: null, reason: "Track Screen is locked." } });
    await w.get('[data-testid="color-treatment-warm"]').trigger("click");
    expect(w.emitted("pick")).toBeUndefined();
    expect(said()).toEqual(["Track Screen is locked."]);
  });

  it("a locked track header's mute", async () => {
    const track: Track = {
      id: "v1", kind: "video", name: "Screen", visible: true, locked: true, muted: false, solo: false, volume: 1,
    };
    const w = mount(TrackHeader, { props: { track, badge: "V1" } });
    await w.get('[data-testid="track-header-v1-mute"]').trigger("click");
    expect(said()).toEqual([lockedReason("Screen")]);
  });
});
