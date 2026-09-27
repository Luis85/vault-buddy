/**
 * Canvas formats and colour treatment (Task 32; F-38, F-39; visual-parity
 * Task 15): `colorPresets.ts` (the treatments and the CSS `filter:` mapping),
 * `ColorSection.vue` (the Color inspector category — a whole-selection
 * `setAdjustments`, the `setLayout` precedent). The canvas-format control that used to be tested here is
 * the preview header's Frame dialog now (`editorPreviewHeader.test.ts`,
 * visual-parity Task 11).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ColorSection from "../src/components/editor/inspector/ColorSection.vue";
import { adjustmentsFilter, COLOR_TREATMENTS } from "../src/editor/colorPresets";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import type { Asset, Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function asset(id: string, overrides: Partial<Asset> = {}): Asset {
  return { id, kind: "video", name: id, duration_ms: 10_000, ...overrides };
}
function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "cam",
    track_id: "v1",
    name: id,
    start_ms: 0,
    in_ms: 0,
    out_ms: 2_000,
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
    ...overrides,
  };
}
function project(
  clips: Clip[],
  tracks: Track[] = [track("v1"), track("a1", { kind: "audio" })],
  assets: Asset[] = [asset("cam")],
  canvas = { width: 1280, height: 720, fps: 30 },
): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas,
    master_gain: 1,
    assets,
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
let openCount = 0;

/** Each call opens a FRESH base id (`openStaged`'s own short-circuit
 * refuses to re-open the SAME base while a session is already live —
 * `editorProject.ts`'s own module doc — so a test that opens twice, e.g. to
 * mount against two different fixtures, needs each call to look like a
 * different capture). */
async function open(p: Project): Promise<void> {
  executed = [];
  openCount += 1;
  const base = `base-${openCount}`;
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
        Promise.resolve({ snapshot, project: p, workspace: {}, missing: [], sourceBase: base, recovered: false }),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: p });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged(base);
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("colorPresets", () => {
  it("none maps to the CSS identity filter", () => {
    expect(adjustmentsFilter(null)).toBe("none");
    expect(adjustmentsFilter(undefined)).toBe("none");
  });

  // Ruling T5-3: the context menu, the multi inspector and the Color tab
  // share ONE set, the concept's five treatments.
  it("the treatments are the concept's five, each a documented CSS filter string", () => {
    expect(COLOR_TREATMENTS.map((t) => t.label)).toEqual(["Original", "Clear", "Warm", "Soft", "Mono"]);
    const byId = Object.fromEntries(COLOR_TREATMENTS.map((t) => [t.id, t]));
    expect(adjustmentsFilter(byId.original.adjustments)).toBe("none");
    expect(adjustmentsFilter(byId.mono.adjustments)).toBe(
      "brightness(1) contrast(1.12) saturate(1) sepia(0) grayscale(1)",
    );
    expect(adjustmentsFilter(byId.warm.adjustments)).toBe(
      "brightness(1.02) contrast(1.04) saturate(1.08) sepia(0.18) grayscale(0)",
    );
  });
});

const WARM = { brightness: 1.02, contrast: 1.04, saturation: 1.08, sepia: 0.18, grayscale: 0 };

async function slide(w: ReturnType<typeof mount>, testid: string, value: string): Promise<void> {
  const input = w.get(`[data-testid="${testid}"]`);
  (input.element as HTMLInputElement).value = value;
  await input.trigger("input");
  await input.trigger("change");
  await flushPromises();
}

describe("ColorSection", () => {
  // Visual-parity Task 15 (concept spec §5 "Color").
  it("A consistent look: the five treatment tiles, one setAdjustments over the whole selection", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 3_000 })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1", "c2"] } });
    expect(w.findAll("h3").map((h) => h.text())).toEqual(["A consistent look", "Fine adjustments"]);
    // Each tile's picture carries a hidden "Aa"; its name is the label.
    expect(w.findAll("[data-testid^=color-preset-]").map((t) => t.text().replace("Aa", ""))).toEqual([
      "Original", "Clear", "Warm", "Soft", "Mono",
    ]);
    expect(w.get('[data-testid="color-preset-original"]').attributes("aria-pressed")).toBe("true");
    await w.get('[data-testid="color-preset-warm"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setAdjustments", clipIds: ["c1", "c2"], adjustments: WARM }]);
  });

  it("Fine adjustments: Brightness, Contrast and Saturation in percent, the full object sent", async () => {
    await open(project([clip("c1", { adjustments: WARM })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="color-section-brightness-value"]').text()).toBe("102%");
    expect(w.get('[data-testid="color-section-contrast-value"]').text()).toBe("104%");
    expect(w.get('[data-testid="color-section-saturation-value"]').text()).toBe("108%");
    const brightness = w.get('[data-testid="color-section-brightness"]');
    expect([brightness.attributes("min"), brightness.attributes("max")]).toEqual(["25", "200"]);
    expect(w.get('[data-testid="color-section-saturation"]').attributes("min")).toBe("0");
    await slide(w, "color-section-brightness", "150");
    expect(executed).toEqual([
      { kind: "setAdjustments", clipIds: ["c1"], adjustments: { ...WARM, brightness: 1.5 } },
    ]);
    expect(w.get('[data-testid="color-preset-warm"]').attributes("aria-pressed")).toBe("true");
  });

  // Fix round 1: the treatment every clip already wears is not an edit.
  it("the tile the selection already wears sends nothing", async () => {
    await open(project([clip("c1", { adjustments: WARM })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="color-preset-warm"]').trigger("click");
    await w.get('[data-testid="color-preset-original"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setAdjustments", clipIds: ["c1"], adjustments: null }]);
  });

  it("custom values light no tile", async () => {
    await open(project([clip("c1", { adjustments: { ...WARM, brightness: 1.5 } })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    for (const t of COLOR_TREATMENTS) {
      expect(w.get(`[data-testid="color-preset-${t.id}"]`).attributes("aria-pressed")).toBe("false");
    }
  });

  it("Reset color clears the adjustments, and says why when there is nothing to reset", async () => {
    await open(project([clip("c1", { adjustments: WARM })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="color-section-reset"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setAdjustments", clipIds: ["c1"], adjustments: null }]);

    await open(project([clip("c2")]));
    const original = mount(ColorSection, { props: { clipIds: ["c2"] } });
    const reset = original.get('[data-testid="color-section-reset"]');
    expect(reset.attributes("aria-disabled")).toBe("true");
    expect(reset.attributes("title")).toBe("The colour is already the original.");
    await reset.trigger("click");
    expect(executed).toEqual([]);
  });

  it("a card clip in the selection gets the Rust refusal wording, not controls", async () => {
    await open(
      project(
        [clip("c1"), clip("cd1", { asset_id: "cardAsset", start_ms: 5_000 })],
        undefined,
        [asset("cam"), asset("cardAsset", { builtin: "card" })],
      ),
    );
    const w = mount(ColorSection, { props: { clipIds: ["c1", "cd1"] } });
    expect(w.find('[data-testid="color-section"]').exists()).toBe(false);
    expect(w.get('[data-testid="color-section-note"]').text()).toBe("Colour applies to footage, not title cards.");
  });

  it("an audio clip in the selection gets a note instead of controls", async () => {
    await open(project([clip("c1"), clip("s1", { track_id: "a1" })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1", "s1"] } });
    expect(w.find('[data-testid="color-section"]').exists()).toBe(false);
    expect(w.get('[data-testid="color-section-note"]').text()).toContain("video and image clips");
  });

  it("a locked track disables the controls and says why", async () => {
    await open(project([clip("c1", { adjustments: WARM })], [track("v1", { locked: true, name: "Webcam" })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    // The inspector's frame says it once (visual-parity Task 13); the
    // fields carry the reason as their tooltip.
    expect(w.find('[data-testid="color-section-locked"]').exists()).toBe(false);
    expect(w.get("fieldset").attributes("title")).toContain("Track Webcam is locked");
    expect(w.get("fieldset").attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="color-preset-mono"]').attributes("title")).toContain("Track Webcam is locked");
    await w.get('[data-testid="color-preset-mono"]').trigger("click");
    await w.get('[data-testid="color-section-reset"]').trigger("click");
    await slide(w, "color-section-contrast", "150");
    expect(executed).toEqual([]);
  });
});
