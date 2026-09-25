/**
 * The tutorial editor's accessibility floor (Task 58; F-49; PRODUCT-SPEC
 * NFR "keyboard and screen-reader complete"), over the editor as
 * `EditorRoot` mounts it — every library tab and inspector category, a
 * selected clip and a selected teaching cue, and every menu the header,
 * toolbar and timeline open.
 *
 * Two rules a screen-reader user meets first:
 * - **Every button has a name** it can be announced by: visible text, an
 *   `aria-label` or an `aria-labelledby`. A `title` alone is a hover
 *   tooltip — Narrator and NVDA may read it, but it is not a label, and an
 *   icon-only button that has nothing else reads as "button".
 * - **Menus and dialogs give focus back** to the control that opened them
 *   when they close with Escape, so a keyboard user is never dropped at the
 *   top of the window.
 *
 * The real-browser half (a whole keyboard journey, forced colours,
 * light-theme contrast) is `tests/e2e/editorKeyboard.spec.ts`.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import AudioSection from "../src/components/editor/inspector/AudioSection.vue";
import ClipSection from "../src/components/editor/inspector/ClipSection.vue";
import ColorSection from "../src/components/editor/inspector/ColorSection.vue";
import EffectSection from "../src/components/editor/inspector/EffectSection.vue";
import FadesSection from "../src/components/editor/inspector/FadesSection.vue";
import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import LayoutSection from "../src/components/editor/inspector/LayoutSection.vue";
import SpeedSection from "../src/components/editor/inspector/SpeedSection.vue";
import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import EditorShell from "../src/components/editor/shell/EditorShell.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import type { Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { shellPort } from "./helpers/guideShell";

enableAutoUnmount(afterEach);

/** `EditorRoot`'s own slot filling of the shell, every category included. */
const FullShell = defineComponent({
  setup() {
    const section = (component: unknown) => ({ clipIds }: { clipIds: string[] }) =>
      h(component as never, { key: clipIds.join(","), clipIds });
    return () =>
      h(EditorShell, null, {
        library: () => h(LibraryPanel),
        preview: () => h(PreviewSurface),
        inspector: () =>
          h(InspectorPanel, null, {
            clip: section(ClipSection),
            layout: section(LayoutSection),
            fades: section(FadesSection),
            audio: section(AudioSection),
            speed: section(SpeedSection),
            color: section(ColorSection),
            effect: ({ effectId }: { effectId: string }) => h(EffectSection, { key: effectId, effectId }),
          }),
        timeline: () => h(TimelineView),
      });
  },
});

/** The guide suites' project, plus one teaching cue on `body`. */
async function open(): Promise<void> {
  const base = shellPort();
  const opened = await base.openStaged("cap one");
  const project: Project = {
    ...opened.project,
    effects: [
      { id: "cue-1", clip_id: "body", kind: "text", start_ms: 3_500, end_ms: 5_000, x: 0.2, y: 0.3, color: "#ffffff", text: "Look here" },
    ],
  };
  const port = shellPort({ openStaged: () => Promise.resolve({ ...opened, project }) });
  const store = useEditorProjectStore();
  store.setPort(port);
  useEditorWorkspaceStore().setPort(port);
  await store.openStaged("cap one");
}

beforeEach(async () => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(120, 90, 200, 40));
  await open();
});
afterEach(() => vi.restoreAllMocks());

async function mountEditor(): Promise<VueWrapper> {
  const w = mount(FullShell, { attachTo: document.body });
  await flushPromises();
  return w;
}

// ---- accessible names -------------------------------------------------------

const CONTROLS = 'button, [role="button"], [role="menuitem"], [role="tab"], [role="switch"]';
const FIELDS = 'input:not([type="hidden"]), select, textarea';

function labelledByText(el: Element): string {
  const ids = el.getAttribute("aria-labelledby")?.split(/\s+/) ?? [];
  return ids
    .map((id) => document.getElementById(id)?.textContent ?? "")
    .join(" ")
    .trim();
}

function explicitLabel(el: Element): string {
  return labelledByText(el) || (el.getAttribute("aria-label") ?? "").trim();
}

/** A control's name: `aria-labelledby`, `aria-label`, then its text. */
function accessibleName(el: Element): string {
  return explicitLabel(el) || (el.textContent ?? "").trim();
}

/** A field's label: `aria-labelledby`/`aria-label`, a wrapping `<label>`,
 * then a `<label for>`. */
function fieldLabel(el: Element): string {
  const id = el.getAttribute("id");
  const byFor = id ? document.querySelector(`label[for="${id}"]`) : null;
  const wrapping = el.closest("label");
  return explicitLabel(el) || (wrapping?.textContent ?? "").trim() || (byFor?.textContent ?? "").trim();
}

/** A glyph (`⋮`, `+`, `−`, `👁`) or a lone letter (`M`, `S`) is what a
 * compact button SHOWS, not a name: a screen reader reads "vertical
 * ellipsis", "plus" or "M". A name has two letters or digits in it. */
const isName = (text: string) => (text.match(/[\p{L}\p{N}]/gu) ?? []).length >= 2;

const whichOne = (el: Element) => el.getAttribute("data-testid") ?? el.outerHTML.slice(0, 120);

/** Every control in the document without a name, described. */
function unnamed(): string[] {
  return Array.from(document.body.querySelectorAll(CONTROLS))
    .filter((el) => !isName(accessibleName(el)))
    .map(whichOne);
}

/** Every form field in the document without a label, described. */
function unlabelledFields(): string[] {
  return Array.from(document.body.querySelectorAll(FIELDS))
    .filter((el) => fieldLabel(el) === "")
    .map(whichOne);
}

const LIBRARY_TABS = ["media", "titles", "captions", "chapters", "products"];
const CATEGORIES = ["clip", "layout", "fades", "audio", "speed", "color"];

/** Every surface the editor can show without a dialog: each library tab,
 * each inspector category over one selected clip, a selected cue, and the
 * header/toolbar/timeline menus opened. `check` runs on each. */
async function everySurface(w: VueWrapper, check: () => string[]): Promise<string[]> {
  const workspace = useEditorWorkspaceStore();
  const found = new Set<string>();
  const run = async () => {
    await flushPromises();
    for (const offender of check()) found.add(offender);
  };
  workspace.select(["body"]);
  for (const tab of LIBRARY_TABS) {
    workspace.setLibraryTab(tab);
    await run();
  }
  for (const tab of CATEGORIES) {
    workspace.setPropertyTab(tab);
    await run();
  }
  workspace.setSelected({ type: "effect", id: "cue-1" });
  await run();
  for (const toggle of ["editor-header-help", "editor-header-save-menu-toggle", "timeline-toolbar-more", "mixer-toggle", "track-header-v1-menu"]) {
    const el = w.find(`[data-testid="${toggle}"]`);
    expect(el.exists(), `${toggle} is rendered`).toBe(true);
    await el.trigger("click");
    await run();
    await el.trigger("keydown", { key: "Escape" });
    await flushPromises();
  }
  return [...found];
}

/** Each walk re-renders the whole editor about twenty times: under the
 * coverage run's parallel load that outlasts Vitest's 5 s default (the
 * `task-detail.test.ts` precedent). */
const WALK_TIMEOUT_MS = 20_000;

describe("accessible names", () => {
  it("every icon-only button has an accessible name", async () => {
    const w = await mountEditor();
    expect(await everySurface(w, unnamed)).toEqual([]);
  }, WALK_TIMEOUT_MS);

  it("every form field has a label", async () => {
    const w = await mountEditor();
    expect(await everySurface(w, unlabelledFields)).toEqual([]);
  }, WALK_TIMEOUT_MS);
});

// ---- focus return -----------------------------------------------------------

async function pressEscapeOn(el: Element): Promise<void> {
  el.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  await flushPromises();
}

/** Open the menu `toggle` owns from the keyboard's point of view (focus on
 * the toggle, then activate it), move focus into it, press Escape there,
 * and return where focus landed. */
async function escapeFrom(w: VueWrapper, toggle: string, menu: string): Promise<Element | null> {
  const button = w.get(`[data-testid="${toggle}"]`);
  (button.element as HTMLElement).focus();
  await button.trigger("click");
  await flushPromises();
  const opened = document.querySelector(menu);
  expect(opened, `${toggle} opened ${menu}`).not.toBeNull();
  const inside = opened!.querySelector<HTMLElement>("button, input, select") ?? (opened as HTMLElement);
  inside.focus();
  await pressEscapeOn(document.activeElement ?? inside);
  expect(document.querySelector(menu), `${menu} closed on Escape`).toBeNull();
  return document.activeElement;
}

describe("menus and dialogs return focus", () => {
  it.each([
    ["editor-header-help", '[role="menu"][aria-label="Help"]'],
    ["editor-header-save-menu-toggle", '[data-testid="editor-header-save-menu"], [role="menu"][aria-label="Save project"]'],
    ["timeline-toolbar-more", '[data-testid="editor-context-menu"]'],
    ["mixer-toggle", '[data-testid="mixer-popover"]'],
    ["track-header-v1-menu", '[data-testid="track-header-v1-menu-list"]'],
  ])("Escape in the menu %s opens gives focus back to it", async (toggle, menu) => {
    useEditorWorkspaceStore().select(["body"]);
    const w = await mountEditor();
    const landed = await escapeFrom(w, toggle, menu);
    expect(landed?.getAttribute("data-testid")).toBe(toggle);
  });

  it("Escape in a clip's context menu gives focus back to the clip", async () => {
    const w = await mountEditor();
    const clip = w.get('[data-testid="clip-body"]');
    (clip.element as HTMLElement).focus();
    await clip.trigger("keydown", { key: "F10", shiftKey: true });
    await flushPromises();
    const menu = document.querySelector('[data-testid="editor-context-menu"]');
    expect(menu).not.toBeNull();
    await pressEscapeOn(document.activeElement ?? menu!);
    expect(document.querySelector('[data-testid="editor-context-menu"]')).toBeNull();
    expect(document.activeElement?.getAttribute("data-testid")).toBe("clip-body");
  });

  it.each([
    ["editor-header-render", "Render a video"],
    ["editor-header-checks", null],
  ])("closing the dialog %s opens gives focus back to it", async (toggle, label) => {
    const w = await mountEditor();
    const button = w.get(`[data-testid="${toggle}"]`);
    (button.element as HTMLElement).focus();
    await button.trigger("click");
    await flushPromises();
    const dialog = document.querySelector(label ? `[role="dialog"][aria-label="${label}"]` : '[role="dialog"]');
    expect(dialog, `${toggle} opened a dialog`).not.toBeNull();
    expect(dialog!.contains(document.activeElement), "focus moved into the dialog").toBe(true);
    await pressEscapeOn(document.activeElement!);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement?.getAttribute("data-testid")).toBe(toggle);
  });

  it("closing the learning center gives focus back to Help", async () => {
    const w = await mountEditor();
    const help = w.get('[data-testid="editor-header-help"]');
    (help.element as HTMLElement).focus();
    await help.trigger("click");
    await flushPromises();
    await w.get('[data-testid="editor-help-learning-center"]').trigger("click");
    await flushPromises();
    expect(document.querySelector('[data-testid="learning-center"]')).not.toBeNull();
    await pressEscapeOn(document.activeElement!);
    expect(document.querySelector('[data-testid="learning-center"]')).toBeNull();
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-header-help");
  });
});
