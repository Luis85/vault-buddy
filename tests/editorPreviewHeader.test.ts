/**
 * The preview column's header (visual-parity Task 11; concept spec §4.1,
 * §9.11, §11; design D1, D5, D14): the panel toggle, "Preview", the ratio
 * button and its Frame dialog, the teaching-tool strip and its More tools
 * menu, Review, the View menu and the properties toggle — and the density
 * rules the header's own width drives.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import PreviewHeader from "../src/components/editor/shell/PreviewHeader.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { previewDensity } from "../src/editor/previewHeader";
import { requestReveal, revealSerial } from "../src/editor/revealBus";
import type { Asset, Clip, Effect, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
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
    out_ms: 4_000,
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
function project(clips: Clip[] = [clip("c1")], tracks: Track[] = [track("v1")]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [asset("cam")],
    tracks,
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

let executed: unknown[];
/** While true the fake Rust refuses every edit. */
let refuseEdits = false;

/** What the fake Rust answers: a canvas change, or a new cue named `fx-new`. */
function applied(p: Project, command: EditorCommand): Project {
  if (command.kind === "setCanvas") return { ...p, canvas: { ...p.canvas, width: command.width, height: command.height } };
  if (command.kind !== "addEffect") return p;
  const cue: Effect = { id: "fx-new", clip_id: command.clipId, kind: command.effectKind, start_ms: command.startMs, end_ms: command.endMs, x: 0.5, y: 0.5, color: "#ffd279" };
  return { ...p, effects: [...p.effects, cue] };
}
let openCount = 0;

async function open(p: Project = project()): Promise<void> {
  openCount += 1;
  const base = `base-${openCount}`;
  const store = useEditorProjectStore();
  const snapshot = {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 1,
    persistedRevision: null,
    title: "Tutorial",
    durationMs: 4_000,
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
        if (refuseEdits) return Promise.reject(new Error("refused"));
        return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: applied(p, req.command) });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged(base);
}

function setViewport(width: number, height = 1000): void {
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: width });
  Object.defineProperty(window, "innerHeight", { writable: true, configurable: true, value: height });
  useEditorWorkspaceStore().setViewport(width, height);
}

type Callback = () => void;
const observed: { cb: Callback | null } = { cb: null };
class FakeResizeObserver {
  constructor(cb: Callback) {
    observed.cb = cb;
  }
  observe() {
    /* the test drives the callback */
  }
  disconnect() {
    /* nothing to release */
  }
}

/** Mounts the header and reports `width` as its own measured width, the
 * way the ResizeObserver hands it over in the app. */
async function mountAtWidth(width: number) {
  const original = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver;
  try {
    const w = mount(PreviewHeader, { attachTo: document.body });
    const el = w.get('[data-testid="preview-header"]').element as HTMLElement;
    Object.defineProperty(el, "clientWidth", { configurable: true, value: width });
    observed.cb?.();
    await flushPromises();
    return w;
  } finally {
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = original;
  }
}

beforeEach(() => {
  setActivePinia(createPinia());
  executed = [];
  refuseEdits = false;
  setViewport(1600);
  delete document.documentElement.dataset.theme;
});

describe("previewDensity (concept §11)", () => {
  it("is wide from 800, medium from 520, compact below, and wide while unmeasured", () => {
    expect(previewDensity(1080)).toBe("wide");
    expect(previewDensity(800)).toBe("wide");
    expect(previewDensity(799)).toBe("medium");
    expect(previewDensity(520)).toBe("medium");
    expect(previewDensity(519)).toBe("compact");
    expect(previewDensity(0)).toBe("wide");
  });
});

describe("PreviewHeader — the row (§4.1)", () => {
  it("is 48px, and 44 in a window 760px tall or less", async () => {
    const tall = mount(PreviewHeader);
    expect(tall.get('[data-testid="preview-header"]').classes()).toContain("h-12");
    tall.unmount();
    setViewport(1600, 640);
    const short = mount(PreviewHeader);
    expect(short.get('[data-testid="preview-header"]').classes()).toContain("h-11");
  });

  it("left: the panel toggle (panelLeft, aria-pressed) and 'Preview'", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    const toggle = w.get('[data-testid="preview-library-toggle"]');
    expect(toggle.find("svg").exists()).toBe(true);
    expect(toggle.attributes("aria-pressed")).toBe("true");
    expect(toggle.attributes("title")).toBe("Hide media library");
    expect(w.get('[data-testid="preview-heading"]').text()).toBe("Preview");

    await toggle.trigger("click");
    expect(useEditorWorkspaceStore().libraryVisible).toBe(false);
    expect(toggle.attributes("aria-pressed")).toBe("false");
    expect(toggle.attributes("title")).toBe("Show media library");
  });

  it("right: Review, the View menu and the properties toggle (sliders, aria-pressed)", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    expect(w.get('[data-testid="preview-review"]').text()).toBe("Review");
    expect(w.get('[data-testid="preview-view-menu"]').attributes("aria-haspopup")).toBe("menu");
    const props = w.get('[data-testid="preview-properties-toggle"]');
    expect(props.attributes("aria-pressed")).toBe("true");
    await props.trigger("click");
    expect(useEditorWorkspaceStore().inspectorVisible).toBe(false);
    expect(props.attributes("aria-pressed")).toBe("false");
    expect(props.attributes("title")).toBe("Show properties");
  });

  it("at or below 1080px the properties toggle opens the inspector drawer", async () => {
    setViewport(960, 640);
    const w = mount(PreviewHeader, { attachTo: document.body });
    const props = w.get('[data-testid="preview-properties-toggle"]');
    expect(props.attributes("aria-pressed")).toBe("false");
    await props.trigger("click");
    expect(useEditorWorkspaceStore().propertiesOpen).toBe(true);
    expect(props.attributes("aria-pressed")).toBe("true");
  });
});

describe("PreviewHeader — the toolstrip (§4.1)", () => {
  it("is one toolbar with Text, Arrow, Highlight and Zoom, each with its icon, then More tools", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    expect(w.findAll('[role="toolbar"]')).toHaveLength(1);
    const strip = w.get('[data-testid="preview-toolstrip"]');
    expect(strip.attributes("role")).toBe("toolbar");
    for (const [key, label] of [["text", "Text"], ["arrow", "Arrow"], ["highlight", "Highlight"], ["zoom", "Zoom"]]) {
      const tool = w.get(`[data-testid="preview-tool-${key}"]`);
      expect(tool.text()).toBe(label);
      expect(tool.find("svg").exists()).toBe(true);
    }
    expect(w.get('[data-testid="preview-more-tools"]').text()).toBe("More tools");
  });

  it("roving tabindex: one tab stop, arrows wrap, Home/End jump", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    const strip = w.get('[data-testid="preview-toolstrip"]');
    const stops = () => strip.findAll('button[tabindex="0"]').map((b) => b.attributes("data-testid"));
    expect(stops()).toEqual(["preview-tool-text"]);

    await strip.trigger("keydown", { key: "ArrowLeft" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("preview-more-tools");
    await strip.trigger("keydown", { key: "ArrowRight" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("preview-tool-text");
    await strip.trigger("keydown", { key: "End" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("preview-more-tools");
    await strip.trigger("keydown", { key: "Home" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("preview-tool-text");
    expect(stops()).toEqual(["preview-tool-text"]);
  });

  it("a tool adds its cue at the playhead and selects it", async () => {
    await open();
    const ws = useEditorWorkspaceStore();
    ws.select(["c1"]);
    const w = mount(PreviewHeader, { attachTo: document.body });
    await w.get('[data-testid="preview-tool-arrow"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([expect.objectContaining({ kind: "addEffect", clipId: "c1", effectKind: "arrow" })]);
    expect(ws.selectionClipIds).toEqual(["c1"]);
    expect(ws.selected).toEqual({ type: "effect", id: "fx-new" });
  });

  it("a disabled tool says why, on hover and on press", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    const text = w.get('[data-testid="preview-tool-text"]');
    expect(text.attributes("aria-disabled")).toBe("true");
    expect(text.attributes("title")).toBe("No project is open.");
    await text.trigger("click");
    expect(useNotificationsStore().items.map((n) => n.message)).toEqual(["No project is open."]);
  });

  it("More tools lists Spotlight, Numbered step and Privacy cover, the cover disabled with the registry's reason", async () => {
    await open(project([clip("c1")], [track("v1", { locked: true })]));
    const w = mount(PreviewHeader, { attachTo: document.body });
    const more = w.get('[data-testid="preview-more-tools"]');
    expect(more.attributes("aria-haspopup")).toBe("menu");
    await more.trigger("click");
    await flushPromises();
    const menu = w.get('[data-testid="preview-more-tools-panel"]');
    expect(w.get('[data-testid="preview-more-tools-panel-heading"]').text()).toBe("More teaching tools");
    expect(w.get('[data-testid="preview-more-tools-panel-subtitle"]').text()).toBe("Choose a cue for the current moment");
    expect(menu.findAll('[role="menuitem"]').map((i) => i.text())).toEqual(["Spotlight", "Numbered step", "Privacy cover"]);
    const cover = w.get('[data-testid="preview-more-tools-panel-item-addMask"]');
    expect(cover.attributes("aria-disabled")).toBe("true");
    expect(cover.attributes("title")).toBeTruthy();
    expect(more.attributes("aria-expanded")).toBe("true");
  });

  it("a More tools item adds its cue", async () => {
    await open();
    useEditorWorkspaceStore().select(["c1"]);
    const w = mount(PreviewHeader, { attachTo: document.body });
    await w.get('[data-testid="preview-more-tools"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="preview-more-tools-panel-item-addStep"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([expect.objectContaining({ kind: "addEffect", effectKind: "step" })]);
    expect(w.find('[data-testid="preview-more-tools-panel"]').exists()).toBe(false);
  });

  it("pressing More tools again closes its menu", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    const more = w.get('[data-testid="preview-more-tools"]');
    await more.trigger("click");
    await flushPromises();
    await more.trigger("pointerdown");
    await more.trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="preview-more-tools-panel"]').exists()).toBe(false);
  });
});

describe("PreviewHeader — density (§11, measured on the header's own width)", () => {
  it("wide: 'Preview' and 'More tools', every tool in the strip", async () => {
    const w = await mountAtWidth(1080);
    expect(w.find('[data-testid="preview-heading"]').exists()).toBe(true);
    expect(w.get('[data-testid="preview-more-tools"]').text()).toBe("More tools");
    expect(w.find('[data-testid="preview-tool-zoom"]').exists()).toBe(true);
  });

  it("under 800: 'Preview' is hidden and More tools reads 'More'", async () => {
    const w = await mountAtWidth(700);
    expect(w.find('[data-testid="preview-heading"]').exists()).toBe(false);
    expect(w.get('[data-testid="preview-more-tools"]').text()).toBe("More");
    expect(w.get('[data-testid="preview-review"]').text()).toBe("Review");
  });

  it("narrowing from wide to compact with More focused keeps one tab stop, on More", async () => {
    // Regression (carried from the old toolbar's "clamp" test): More is
    // index 4 in the wide strip and index 2 in the compact one. An
    // unclamped index would match no button and drop the strip from the
    // Tab order entirely.
    await open();
    const w = await mountAtWidth(1080);
    const strip = w.get('[data-testid="preview-toolstrip"]');
    await strip.trigger("keydown", { key: "End" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("preview-more-tools");
    const el = w.get('[data-testid="preview-header"]').element as HTMLElement;
    Object.defineProperty(el, "clientWidth", { configurable: true, value: 480 });
    observed.cb?.();
    await flushPromises();
    expect(strip.findAll('button[tabindex="0"]').map((b) => b.attributes("data-testid"))).toEqual(["preview-more-tools"]);
  });

  it("under 520: Highlight and Zoom move into More, and Review is icon-only", async () => {
    await open();
    const w = await mountAtWidth(500);
    expect(w.find('[data-testid="preview-tool-highlight"]').exists()).toBe(false);
    expect(w.find('[data-testid="preview-tool-zoom"]').exists()).toBe(false);
    const review = w.get('[data-testid="preview-review"]');
    expect(review.text()).toBe("");
    expect(review.attributes("aria-label")).toBe("Review");
    await w.get('[data-testid="preview-more-tools"]').trigger("click");
    await flushPromises();
    const items = w.get('[data-testid="preview-more-tools-panel"]').findAll('[role="menuitem"]').map((i) => i.text());
    expect(items).toEqual(["Highlight", "Zoom", "Spotlight", "Numbered step", "Privacy cover"]);
  });
});

describe("PreviewHeader — the View menu (§4.1)", () => {
  async function openView(w: ReturnType<typeof mount>) {
    await w.get('[data-testid="preview-view-menu"]').trigger("click");
    await flushPromises();
  }
  const item = (w: ReturnType<typeof mount>, id: string) => w.get(`[data-testid="preview-view-panel-item-${id}"]`);

  it("lists the view items, the three toggles and the theme as checkboxes", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    await openView(w);
    expect(w.get('[data-testid="preview-view-panel-heading"]').text()).toBe("View & workspace");
    expect(w.get('[data-testid="preview-view-panel-subtitle"]').text()).toBe("Change your workspace, not your edit");
    const labels = w
      .get('[data-testid="preview-view-panel"]')
      .findAll('[role^="menuitem"]')
      .map((i) => i.text());
    expect(labels).toEqual([
      "Show media library",
      "Show properties",
      "Focus preview",
      "Reset panel layout",
      "Light theme",
      "Audio mixer…",
      "Keyboard shortcuts & help…",
    ]);
    expect(item(w, "library").attributes("role")).toBe("menuitemcheckbox");
    expect(item(w, "library").attributes("aria-checked")).toBe("true");
    expect(item(w, "properties").attributes("aria-checked")).toBe("true");
    expect(item(w, "focusPreview").attributes("aria-checked")).toBe("false");
    expect(item(w, "lightTheme").attributes("aria-checked")).toBe("false");
    expect(w.get('[data-testid="preview-view-panel"]').findAll('[role="separator"]')).toHaveLength(2);
  });

  it("Show media library, Show properties and Focus preview drive the panels", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    const ws = useEditorWorkspaceStore();
    await openView(w);
    await item(w, "library").trigger("click");
    expect(ws.libraryVisible).toBe(false);
    await openView(w);
    expect(item(w, "library").attributes("aria-checked")).toBe("false");
    await item(w, "properties").trigger("click");
    expect(ws.inspectorVisible).toBe(false);
    await openView(w);
    await item(w, "focusPreview").trigger("click");
    expect(ws.focusPreview).toBe(true);
    await openView(w);
    expect(item(w, "focusPreview").attributes("aria-checked")).toBe("true");
  });

  it("Reset panel layout shows both panels again and leaves Focus preview", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    const ws = useEditorWorkspaceStore();
    ws.toggleLibrary();
    ws.toggleFocusPreview();
    await openView(w);
    await item(w, "resetLayout").trigger("click");
    expect(ws.focusPreview).toBe(false);
    expect(ws.libraryVisible).toBe(true);
    expect(ws.inspectorVisible).toBe(true);
  });

  it("Audio mixer… asks the mixer to open; Keyboard shortcuts & help… asks for the shortcuts", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    const mixer = revealSerial("mixer");
    const shortcuts = revealSerial("shortcuts");
    await openView(w);
    await item(w, "mixer").trigger("click");
    expect(revealSerial("mixer")).toBe(mixer + 1);
    await openView(w);
    await item(w, "help").trigger("click");
    expect(revealSerial("shortcuts")).toBe(shortcuts + 1);
  });

  it("the trigger closes an open View menu", async () => {
    const w = mount(PreviewHeader, { attachTo: document.body });
    await openView(w);
    expect(w.get('[data-testid="preview-view-menu"]').attributes("aria-expanded")).toBe("true");
    await w.get('[data-testid="preview-view-menu"]').trigger("pointerdown");
    await openView(w);
    expect(w.find('[data-testid="preview-view-panel"]').exists()).toBe(false);
  });
});

describe("the ratio button and Frame your tutorial (§9.11)", () => {
  it("reads the canvas's ratio in mono and opens the dialog with four formats", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    const ratio = w.get('[data-testid="preview-ratio"]');
    expect(ratio.text()).toBe("16:9");
    expect(ratio.classes()).toContain("vb-mono");
    await ratio.trigger("click");
    await flushPromises();
    const dialog = w.get('[data-testid="frame-dialog"]');
    expect(dialog.findAll('[data-testid^="frame-choice-"]').map((c) => c.find("b").text())).toEqual([
      "Landscape · 16:9",
      "Portrait · 9:16",
      "Square · 1:1",
      "Classic · 4:3",
    ]);
    expect(w.get('[data-testid="frame-choice-1280x720"]').attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="frame-choice-1280x720"]').text()).toContain("1280 × 720");
  });

  it("choosing another format sends setCanvas, closes, and raises the one-time Checks toast", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    await w.get('[data-testid="preview-ratio"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="frame-choice-720x1280"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setCanvas", width: 720, height: 1280 }]);
    expect(w.find('[data-testid="frame-dialog"]').exists()).toBe(false);
    const notifications = useNotificationsStore();
    expect(notifications.items).toHaveLength(1);
    expect(notifications.items[0]).toMatchObject({
      kind: "info",
      message: "Canvas changed. Review crop, text and caption placement in Checks.",
    });
    expect(notifications.items[0].action?.label).toBe("Open Checks");
    expect(w.get('[data-testid="preview-ratio"]').text()).toBe("9:16");

    // A second change replaces the first toast rather than stacking.
    const firstId = notifications.items[0].id;
    await w.get('[data-testid="preview-ratio"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="frame-choice-720x720"]').trigger("click");
    await flushPromises();
    expect(notifications.items).toHaveLength(1);
    expect(notifications.items[0].id).not.toBe(firstId);
  });

  it("the current format and 'Keep current format' close without an edit", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    await w.get('[data-testid="preview-ratio"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="frame-choice-1280x720"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="frame-dialog"]').exists()).toBe(false);
    await w.get('[data-testid="preview-ratio"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="frame-dialog-keep"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="frame-dialog"]').exists()).toBe(false);
    expect(executed).toEqual([]);
  });

  it("a refused change keeps the dialog open and raises no Checks toast", async () => {
    await open();
    refuseEdits = true;
    const w = mount(PreviewHeader, { attachTo: document.body });
    await w.get('[data-testid="preview-ratio"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="frame-choice-720x720"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setCanvas", width: 720, height: 720 }]);
    expect(w.find('[data-testid="frame-dialog"]').exists()).toBe(true);
    expect(useNotificationsStore().items.some((n) => n.message.startsWith("Canvas changed"))).toBe(false);
    expect(w.get('[data-testid="preview-ratio"]').text()).toBe("16:9");
  });

  it("a canvas finding's reveal puts focus on the ratio button", async () => {
    await open();
    const w = mount(PreviewHeader, { attachTo: document.body });
    requestReveal("ratio");
    await flushPromises();
    expect(document.activeElement).toBe(w.get('[data-testid="preview-ratio"]').element);
  });

  it("with no project open the ratio button is disabled and says why", () => {
    const w = mount(PreviewHeader);
    const ratio = w.get('[data-testid="preview-ratio"]');
    expect(ratio.attributes("disabled")).toBeDefined();
    expect(ratio.attributes("title")).toBe("No project is open.");
  });
});
