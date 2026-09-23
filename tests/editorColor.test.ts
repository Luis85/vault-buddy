/**
 * Canvas formats and colour treatment (Task 32; F-38, F-39): `colorPresets.ts`
 * (presets and the CSS `filter:` mapping), `ColorSection.vue` (the Color
 * inspector category — a whole-selection `setAdjustments`, the `setLayout`
 * precedent), and `PreviewToolbar.vue`'s ratio control (sends `setCanvas`
 * directly, never through `commandFor`).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ColorSection from "../src/components/editor/inspector/ColorSection.vue";
import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
import ToolbarOverflowMenu from "../src/components/editor/shell/ToolbarOverflowMenu.vue";
import NotificationHost from "../src/components/NotificationHost.vue";
import { baseActionContext } from "../src/editor/actionContext";
import { resolveActions } from "../src/editor/actions";
import type { ColorPresetId } from "../src/editor/colorPresets";
import { adjustmentsFilter, COLOR_PRESETS, findColorPreset } from "../src/editor/colorPresets";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import type { Asset, Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useNotificationsStore } from "../src/stores/notifications";
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

async function type(w: ReturnType<typeof mount>, testid: string, value: string): Promise<void> {
  const input = w.get(`[data-testid="${testid}"]`);
  await input.setValue(value);
  await input.trigger("keydown", { key: "Enter" });
  await flushPromises();
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("colorPresets", () => {
  it("none maps to the CSS identity filter", () => {
    expect(adjustmentsFilter(null)).toBe("none");
    expect(adjustmentsFilter(undefined)).toBe("none");
  });

  it("findColorPreset resolves every declared id and refuses an unknown one", () => {
    for (const preset of COLOR_PRESETS) {
      expect(findColorPreset(preset.id)).toBe(preset);
    }
    expect(() => findColorPreset("nope" as ColorPresetId)).toThrow("unknown color preset");
  });

  // Named test (brief): "preset maps to the documented CSS filter string".
  it("each documented preset maps to the exact brightness/contrast/saturate/sepia/grayscale CSS filter string", () => {
    const byId = Object.fromEntries(COLOR_PRESETS.map((p) => [p.id, p]));
    expect(adjustmentsFilter(byId.none.adjustments)).toBe("none");
    expect(adjustmentsFilter(byId.vivid.adjustments)).toBe(
      "brightness(1) contrast(1.1) saturate(1.3) sepia(0) grayscale(0)",
    );
    expect(adjustmentsFilter(byId.warm.adjustments)).toBe(
      "brightness(1) contrast(1) saturate(1.1) sepia(0.2) grayscale(0)",
    );
    expect(adjustmentsFilter(byId.cool.adjustments)).toBe(
      "brightness(1) contrast(1) saturate(0.9) sepia(0) grayscale(0)",
    );
    expect(adjustmentsFilter(byId.mono.adjustments)).toBe(
      "brightness(1) contrast(1) saturate(1) sepia(0) grayscale(1)",
    );
    expect(adjustmentsFilter(byId.sepia.adjustments)).toBe(
      "brightness(1) contrast(1) saturate(1) sepia(0.8) grayscale(0)",
    );
  });
});

describe("ColorSection", () => {
  it("shows the first clip's values and sends a typed brightness as one setAdjustments over the whole selection", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 3_000 })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1", "c2"] } });
    expect((w.get('[data-testid="color-section-saturation"]').element as HTMLInputElement).value).toBe("1");
    await type(w, "color-section-brightness", "1.5");
    expect(executed).toEqual([
      {
        kind: "setAdjustments",
        clipIds: ["c1", "c2"],
        adjustments: { brightness: 1.5, contrast: 1, saturation: 1, sepia: 0, grayscale: 0 },
      },
    ]);
  });

  it("refuses an out-of-range value inline and sends nothing", async () => {
    await open(project([clip("c1")]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    await type(w, "color-section-contrast", "3");
    expect(w.get('[data-testid="color-section-contrast-error"]').text()).toContain("0.25");
    expect(executed).toEqual([]);
    await type(w, "color-section-saturation", "2.5");
    expect(w.get('[data-testid="color-section-saturation-error"]').text()).toContain("0 and 2");
    expect(executed).toEqual([]);
  });

  it("a preset sends every field at once and None clears", async () => {
    await open(project([clip("c1")]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="color-preset-none"]').attributes("aria-pressed")).toBe("true");
    await w.get('[data-testid="color-preset-vivid"]').trigger("click");
    expect(executed).toEqual([
      { kind: "setAdjustments", clipIds: ["c1"], adjustments: { brightness: 1, contrast: 1.1, saturation: 1.3, sepia: 0, grayscale: 0 } },
    ]);
    await w.get('[data-testid="color-preset-none"]').trigger("click");
    expect(executed[1]).toEqual({ kind: "setAdjustments", clipIds: ["c1"], adjustments: null });
  });

  it("sends a typed grayscale as one setAdjustments", async () => {
    await open(project([clip("c1")]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    await type(w, "color-section-grayscale", "1");
    expect(executed).toEqual([
      { kind: "setAdjustments", clipIds: ["c1"], adjustments: { brightness: 1, contrast: 1, saturation: 1, sepia: 0, grayscale: 1 } },
    ]);
  });

  it("a clip's own adjustments light the matching preset, and custom values light none", async () => {
    await open(
      project([clip("c1", { adjustments: { brightness: 1, contrast: 1.1, saturation: 1.3, sepia: 0, grayscale: 0 } })]),
    );
    const matched = mount(ColorSection, { props: { clipIds: ["c1"] } });
    expect(matched.get('[data-testid="color-preset-vivid"]').attributes("aria-pressed")).toBe("true");
    expect(matched.get('[data-testid="color-preset-none"]').attributes("aria-pressed")).toBe("false");

    await open(
      project([clip("c2", { adjustments: { brightness: 1.5, contrast: 1, saturation: 1, sepia: 0, grayscale: 0 } })]),
    );
    const custom = mount(ColorSection, { props: { clipIds: ["c2"] } });
    for (const preset of COLOR_PRESETS) {
      expect(custom.get(`[data-testid="color-preset-${preset.id}"]`).attributes("aria-pressed")).toBe("false");
    }
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
    await open(project([clip("c1")], [track("v1", { locked: true, name: "Webcam" })]));
    const w = mount(ColorSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="color-section-locked"]').text()).toContain("Track Webcam is locked");
    expect(w.get("fieldset").attributes("disabled")).toBeDefined();
    await w.get('[data-testid="color-preset-mono"]').trigger("click");
    expect(executed).toEqual([]);
  });
});

describe("PreviewToolbar — ratio control", () => {
  // Named test (brief): "ratio control sends setCanvas".
  it("sends setCanvas for the chosen aspect ratio and raises the one-time toast through the shared notifications store", async () => {
    await open(project([clip("c1")], undefined, undefined, { width: 1280, height: 720, fps: 30 }));
    const notifications = useNotificationsStore();
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    const select = w.get('[data-testid="preview-toolbar-ratio"]');
    expect((select.element as HTMLSelectElement).value).toBe("1280x720");
    await select.setValue("720x1280");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setCanvas", width: 720, height: 1280 }]);
    // Fix round 1 (Important 2): the toast is `useNotificationsStore`'s own
    // "info" item, not a hand-rolled local one -- PreviewToolbar itself no
    // longer renders any toast markup at all.
    expect(notifications.items).toHaveLength(1);
    expect(notifications.items[0]).toMatchObject({
      kind: "info",
      message: "Canvas changed. Review crop, text and caption placement in Checks.",
    });
  });

  it("the toast is dismissible through NotificationHost, the same pair ActionPanel.vue already uses", async () => {
    await open(project([clip("c1")]));
    const notifications = useNotificationsStore();
    const toolbar = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    // Fix round 1: EditorShell.vue mounts ONE NotificationHost for the
    // whole editor window (AGENTS.md's per-window-Pinia model, so a
    // separately-mounted host here shares the SAME active store the
    // toolbar just wrote to).
    const host = mount(NotificationHost);
    await toolbar.get('[data-testid="preview-toolbar-ratio"]').setValue("720x1280");
    await flushPromises();
    expect(host.get('[data-testid="notification"]').text()).toContain("Checks");
    await host.get('[data-testid="notification-dismiss"]').trigger("click");
    expect(notifications.items).toHaveLength(0);
    expect(host.find('[data-testid="notification"]').exists()).toBe(false);
  });

  it("the ratio control is disabled with no project open", () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    expect(w.get('[data-testid="preview-toolbar-ratio"]').attributes("disabled")).toBeDefined();
  });

  it("a second pick before the first toast expires restarts its TTL rather than stacking a second toast", async () => {
    await open(project([clip("c1")]));
    const notifications = useNotificationsStore();
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    await w.get('[data-testid="preview-toolbar-ratio"]').setValue("720x1280");
    await flushPromises();
    expect(notifications.items).toHaveLength(1);
    const firstId = notifications.items[0]!.id;
    // The project the fake port returns never changes, so the SAME pick
    // fires no native `change` (already selected) -- pick a THIRD ratio to
    // trigger a second, genuine toast while the first is still showing.
    await w.get('[data-testid="preview-toolbar-ratio"]').setValue("720x720");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "setCanvas", width: 720, height: 1280 },
      { kind: "setCanvas", width: 720, height: 720 },
    ]);
    // The message is the SAME constant string both times, so the store's
    // own dedupe (notifications.ts's isRepeat) reuses the one notification
    // and restarts its TTL, rather than pushing a second toast.
    expect(notifications.items).toHaveLength(1);
    expect(notifications.items[0]!.id).toBe(firstId);
  });

  it("reaching the ratio control via ArrowRight focuses its native select", async () => {
    await open(project([clip("c1")]));
    const w = mount(PreviewToolbar, { attachTo: document.body, props: { overflowCount: 0 } });
    const row = w.get('[data-testid="preview-toolbar"]');
    // TOOLBAR_ITEMS: 7 teaching tools, then ratio -- seven ArrowRights from
    // the initial index 0 land on it.
    for (let i = 0; i < 7; i++) await row.trigger("keydown", { key: "ArrowRight" });
    expect(document.activeElement).toBe(w.get('[data-testid="preview-toolbar-ratio"]').element);
    w.unmount();
  });

});

describe("ToolbarOverflowMenu", () => {
  function resolved() {
    return resolveActions(
      baseActionContext(project([clip("c1")]), {
        sessionId: "s",
        projectId: "p",
        revision: 1,
        persistedRevision: null,
        title: "T",
        durationMs: 1000,
        canUndo: false,
        canRedo: false,
        undoLabel: null,
        redoLabel: null,
      }, 0, []),
    );
  }

  it("renders the ratio select and a normal item, and emits for both", async () => {
    const w = mount(ToolbarOverflowMenu, {
      props: { items: ["ratio", "toggleLibrary"], resolved: resolved(), canvasValue: "1280x720" },
    });
    const select = w.get('[data-testid="preview-toolbar-ratio"]');
    expect(select.element.tagName).toBe("SELECT");
    await select.setValue("720x1280");
    expect(w.emitted("ratio-change")).toEqual([["720x1280"]]);

    await w.get('[data-testid="preview-toolbar-toggleLibrary"]').trigger("click");
    expect(w.emitted("activate")).toEqual([["toggleLibrary"]]);
  });
});
