/**
 * The guided walkthrough itself (Task 56; F-46, F-49; SCREENS 01, 10;
 * A23–A26): the first-use invitation, the coach that points at the REAL
 * control, its placement (`position.ts`), keyboard ownership (F6, Escape)
 * and suspension under a modal dialog.
 *
 * Every walkthrough test mounts the whole editor shell (the slot filling
 * `EditorRoot` uses) over a recording port, so "the guide never edits"
 * is a statement about what was SENT, not what a store holds. happy-dom
 * has no layout, so `getBoundingClientRect` is stubbed to one asymmetric
 * box: the coach renders its highlight only for a target with a real box,
 * and the placement arithmetic has its own table below.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { dialogStackSize } from "../src/editor/dialogs";
import { GUIDE_STEPS, STEP_TARGETS } from "../src/editor/guide/content";
import type { Rect, Size } from "../src/editor/guide/position";
import { DOCKED_BELOW_PX, EDGE_MARGIN_PX, MIN_CARD, placeCoach, TARGET_GAP_PX } from "../src/editor/guide/position";
import { resolve } from "../src/editor/guide/targets";
import type { EditorPort } from "../src/editor/port";
import { checksDialogOpen } from "../src/editor/revealBus";
import type { GuideProgress } from "../src/editorTypes";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { MountedShell, shellPort } from "./helpers/guideShell";

enableAutoUnmount(afterEach);

// ---- position.ts ----------------------------------------------------------

/** Where a placement's card really lands: `maxHeight` caps it. */
function cardBox(p: ReturnType<typeof placeCoach>, card: Size): Rect {
  return { x: p.x, y: p.y, width: p.width, height: Math.min(card.height, p.maxHeight) };
}

function intersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/** Six real control shapes, as fractions of the viewport so each exists at
 * every size: small header button, library button, the wide transport, a
 * full-width toolbar, the tall inspector, a tiny bottom-left track menu.
 * Deliberately asymmetric (x ≠ y fractions, w ≠ h). */
const TARGETS: [string, (vp: Size) => Rect][] = [
  ["header help", (vp) => ({ x: vp.width * 0.7, y: vp.height * 0.01, width: 60, height: 28 })],
  ["library import", (vp) => ({ x: vp.width * 0.01, y: vp.height * 0.1, width: 120, height: 36 })],
  ["transport", (vp) => ({ x: vp.width * 0.3, y: vp.height * 0.5, width: vp.width * 0.4, height: 40 })],
  ["timeline toolbar", (vp) => ({ x: 0, y: vp.height * 0.62, width: vp.width, height: 32 })],
  ["inspector", (vp) => ({ x: vp.width * 0.74, y: vp.height * 0.08, width: vp.width * 0.25, height: vp.height * 0.6 })],
  ["track menu", (vp) => ({ x: vp.width * 0.12, y: vp.height * 0.93, width: 24, height: 24 })],
];
const VIEWPORTS: Size[] = [
  { width: 1600, height: 1000 },
  { width: 1280, height: 820 },
  { width: 960, height: 640 },
];
const CARD: Size = { width: 340, height: 320 };

describe("position.ts", () => {
  it("position.ts never overlaps the target rect", () => {
    for (const vp of VIEWPORTS) {
      for (const [name, shape] of TARGETS) {
        const target = shape(vp);
        const p = placeCoach(target, CARD, vp);
        const box = cardBox(p, CARD);
        const where = `${name} at ${vp.width}x${vp.height}: ${JSON.stringify(box)}`;
        expect(intersects(box, target), `covers the target — ${where}`).toBe(false);
        expect(box.x >= EDGE_MARGIN_PX && box.y >= EDGE_MARGIN_PX, `off the top/left — ${where}`).toBe(true);
        expect(box.x + box.width <= vp.width - EDGE_MARGIN_PX, `off the right — ${where}`).toBe(true);
        expect(box.y + box.height <= vp.height - EDGE_MARGIN_PX, `off the bottom — ${where}`).toBe(true);
        // Always big enough to keep its header and Pause/Back/Next row.
        expect(box.width >= MIN_CARD.width && box.height >= MIN_CARD.height, `too small — ${where}`).toBe(true);
        expect(p.mode, `mode — ${where}`).toBe(vp.width < DOCKED_BELOW_PX ? "docked" : "beside");
      }
    }
  });

  it("beside mode sits next to the target, on the side with the most room", () => {
    const vp = { width: 1600, height: 1000 };
    // A button high on the left: the right-hand side has the most room.
    const target = { x: 40, y: 200, width: 120, height: 36 };
    const p = placeCoach(target, CARD, vp);
    expect(p).toMatchObject({ mode: "beside", side: "right", x: 40 + 120 + TARGET_GAP_PX, width: CARD.width });
    // Centred on the target's own middle.
    expect(p.y).toBe(200 + 18 - CARD.height / 2);
  });

  it("docked mode anchors the card to the viewport edge away from the target", () => {
    const vp = { width: 960, height: 640 };
    const target = { x: 0, y: 380, width: 960, height: 32 };
    const p = placeCoach(target, CARD, vp);
    // More room above the toolbar than below it: docked at the top.
    expect(p).toMatchObject({ mode: "docked", side: "top", y: EDGE_MARGIN_PX, x: 960 - EDGE_MARGIN_PX - CARD.width });
    expect(p.maxHeight).toBe(380 - TARGET_GAP_PX - EDGE_MARGIN_PX);
  });

  it("no target docks the card in the bottom-right corner", () => {
    const vp = { width: 1280, height: 820 };
    expect(placeCoach(null, CARD, vp)).toEqual({
      mode: "docked",
      side: null,
      x: 1280 - EDGE_MARGIN_PX - CARD.width,
      y: 820 - EDGE_MARGIN_PX - CARD.height,
      width: CARD.width,
      maxHeight: CARD.height,
    });
  });
});

// ---- the walkthrough in the mounted editor --------------------------------

const STEP_IDS = GUIDE_STEPS.map((s) => s.id);

/** Port methods a walkthrough may call: reads, and the two preference
 * writes (workspace view state, guide progress). Anything else — an edit,
 * a save, a render, an import, a dialog, a camera — is a failure. */
const READ_ONLY = new Set([
  "getSnapshot", "getWorkspace", "saveWorkspace", "getGuideProgress", "saveGuideProgress",
  "getChecks", "getProducts", "getJobs", "mediaUrl", "mediaPeaks", "mediaThumbnail",
]);

function fresh(overrides: Partial<GuideProgress> = {}): GuideProgress {
  return {
    contentRevision: 1, currentStepId: null, reviewed: [], explored: [], invitationDismissed: false,
    active: false, collapsed: false, completed: false, preferences: { dimming: true, motion: "system" },
    ...overrides,
  };
}

/** Records every port method called, by name. */
function recording(port: EditorPort, calls: string[]): EditorPort {
  return new Proxy(port, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver) as unknown;
      if (typeof value !== "function" || typeof prop !== "string") return value;
      return (...args: unknown[]) => {
        calls.push(prop);
        return (value as (...a: unknown[]) => unknown).apply(target, args);
      };
    },
  });
}

let calls: string[];
let saved: GuideProgress | null;

async function install(progress: GuideProgress = fresh()): Promise<void> {
  calls = [];
  saved = null;
  const port = recording(
    shellPort({
      getGuideProgress: () => Promise.resolve(saved ?? progress),
      saveGuideProgress: (p) => {
        saved = structuredClone(p);
        return Promise.resolve();
      },
    }),
    calls,
  );
  const project = useEditorProjectStore();
  project.setPort(port);
  useEditorWorkspaceStore().setPort(port);
  await project.openStaged("cap one");
}

async function mountEditor(): Promise<VueWrapper> {
  const w = mount(MountedShell, { attachTo: document.body });
  await flushPromises();
  return w;
}

const coach = (w: VueWrapper) => w.find('[data-testid="guide-coach"]');

async function click(w: VueWrapper, testid: string): Promise<void> {
  await w.get(`[data-testid="${testid}"]`).trigger("click");
  await flushPromises();
}

async function goTo(w: VueWrapper, id: string): Promise<void> {
  while (coach(w).attributes("data-step-id") !== id) await click(w, "guide-next");
}

function keydown(el: Element, key: string): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
  el.dispatchEvent(event);
  return event;
}

beforeEach(async () => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
  checksDialogOpen.value = false;
  // One asymmetric box for every element: the coach highlights only a
  // target that has one.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(120, 90, 200, 40));
  await install();
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("the invitation", () => {
  it("is nonmodal: it neither steals focus nor blocks editing", async () => {
    const before = document.createElement("button");
    document.body.appendChild(before);
    before.focus();
    const w = await mountEditor();

    const invitation = w.get('[data-testid="guide-invitation"]');
    expect(document.activeElement).toBe(before);
    expect(invitation.attributes("aria-modal")).toBeUndefined();
    expect(invitation.attributes("role")).toBe("region");
    expect(coach(w).exists()).toBe(false);
    // The editor behind it still answers.
    await click(w, "timeline-toolbar-more");
    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(true);
    before.remove();
  });

  it("Not now dismisses it for good; Show me around starts at the first lesson", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-dismiss");
    expect(w.find('[data-testid="guide-invitation"]').exists()).toBe(false);
    await useEditorOnboardingStore().flush();
    expect(saved).toMatchObject({ invitationDismissed: true, active: false, currentStepId: null });

    await click(w, "editor-header-help");
    expect(coach(w).attributes("data-step-id")).toBe("welcome");
    expect(w.get('[data-testid="guide-coach-count"]').text()).toBe("1 / 22");
  });
});

describe("the coach", () => {
  it("walking all 22 steps sends zero editor_execute and zero device or file requests", async () => {
    const getUserMedia = vi.fn();
    const enumerateDevices = vi.fn();
    Object.defineProperty(navigator, "mediaDevices", { value: { getUserMedia, enumerateDevices }, configurable: true });
    const w = await mountEditor();
    const revision = useEditorProjectStore().snapshot?.revision;
    calls.length = 0;

    await click(w, "guide-invitation-start");
    for (const id of STEP_IDS) {
      expect(coach(w).attributes("data-step-id")).toBe(id);
      // The coach resolved the lesson's own control, or says it could not.
      expect(["direct", "overflow"], `lesson ${id}`).toContain(coach(w).attributes("data-target-state"));
      await click(w, "guide-next");
    }

    expect(useEditorOnboardingStore().progress).toMatchObject({ completed: true, active: false });
    expect(coach(w).exists()).toBe(false);
    expect(calls.filter((c) => c === "execute")).toEqual([]);
    expect(calls.filter((c) => !READ_ONLY.has(c))).toEqual([]);
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(enumerateDevices).not.toHaveBeenCalled();
    expect(dialogStackSize()).toBe(0);
    expect(useEditorProjectStore().snapshot).toMatchObject({ revision, canUndo: false });
  });

  it("closing and reopening resumes the exact step", async () => {
    const first = await mountEditor();
    await click(first, "guide-invitation-start");
    await goTo(first, "fades");
    await click(first, "guide-close");
    expect(coach(first).exists()).toBe(false);
    first.unmount();
    await flushPromises();
    expect(saved).toMatchObject({ currentStepId: "fades", active: false });

    // A native reload: fresh stores, the progress read back from storage.
    setActivePinia(createPinia());
    const kept = saved;
    await install(fresh());
    saved = kept;
    const second = await mountEditor();
    expect(coach(second).exists()).toBe(false);
    expect(second.find('[data-testid="guide-invitation"]').exists()).toBe(false);

    await click(second, "editor-header-help");
    expect(coach(second).attributes("data-step-id")).toBe("fades");
    expect(second.get('[data-testid="guide-coach-count"]').text()).toBe("13 / 22");
    expect(useEditorOnboardingStore().progress.reviewed).toEqual(STEP_IDS.slice(0, 13));
  });

  it("opening a dialog suspends and closing resumes the same step", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "checks");
    const checks = w.get('[data-testid="editor-header-checks"]');

    (checks.element as HTMLElement).focus();
    await checks.trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="checks-dialog"]').exists()).toBe(true);
    expect(useEditorOnboardingStore().suspended).toBe(true);
    expect(coach(w).exists()).toBe(false);
    expect(w.find('[data-testid="guide-ring"]').exists()).toBe(false);

    await click(w, "checks-close");
    expect(useEditorOnboardingStore().suspended).toBe(false);
    expect(coach(w).attributes("data-step-id")).toBe("checks");
    // Focus went back to the control that opened the dialog, not the coach.
    expect(document.activeElement).toBe(checks.element);
    // Trying the control was recorded, and did not move the lesson.
    expect(useEditorOnboardingStore().progress.explored).toEqual(["checks"]);
  });

  it("guide elements are not descendants of the preview stage", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "callouts");
    const preview = w.get('[data-testid="editor-shell-preview"]').element;
    const target = resolve(STEP_TARGETS.callouts)?.element;
    // The lesson's control really is inside the preview section…
    expect(target && preview.contains(target)).toBe(true);

    const layers = [...document.querySelectorAll("[data-guide-layer]")];
    expect(layers.map((l) => l.getAttribute("data-guide-layer")).sort()).toEqual(["card", "ring"]);
    for (const layer of layers) {
      // …while nothing the guide draws is.
      expect(preview.contains(layer)).toBe(false);
      expect(target?.contains(layer)).toBe(false);
    }
  });

  it("F6 toggles focus between card and target", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    const card = coach(w).element;
    const target = resolve(STEP_TARGETS.welcome)?.element as HTMLElement;
    // Starting from Show me around puts focus in the card.
    expect(card.contains(document.activeElement)).toBe(true);

    keydown(document.activeElement as Element, "F6");
    await flushPromises();
    expect(target.contains(document.activeElement)).toBe(true);

    keydown(document.activeElement as Element, "F6");
    await flushPromises();
    expect(card.contains(document.activeElement)).toBe(true);
  });

  it("exploring the highlighted control records it and never advances the lesson", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "split");

    await click(w, "timeline-toolbar-split");

    expect(useEditorOnboardingStore().progress.explored).toEqual(["split"]);
    expect(coach(w).attributes("data-step-id")).toBe("split");
  });

  it("Escape in an open menu closes the menu before the guide", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "context");
    await click(w, "timeline-toolbar-more");
    const menu = w.get('[data-testid="editor-context-menu"]');

    keydown(menu.element, "Escape");
    await flushPromises();
    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(false);
    expect(coach(w).attributes("data-step-id")).toBe("context");

    keydown(w.get('[data-testid="timeline-toolbar-more"]').element, "Escape");
    await flushPromises();
    expect(coach(w).exists()).toBe(false);
    expect(useEditorOnboardingStore().progress).toMatchObject({ active: false, currentStepId: "context" });
  });

  // The track menu closes on a WINDOW Escape listener and lets the event
  // bubble through the shell, so here it is the guide that must notice an
  // open menu and leave the key to it.
  it("Escape with the track menu open closes the menu, not the guide", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "tracks");
    await click(w, "track-header-v1-menu");
    expect(w.find('[data-testid="track-header-v1-menu-list"]').exists()).toBe(true);

    keydown(w.get('[data-testid="track-header-v1-menu"]').element, "Escape");
    await flushPromises();
    expect(w.find('[data-testid="track-header-v1-menu-list"]').exists()).toBe(false);
    expect(coach(w).attributes("data-step-id")).toBe("tracks");
  });

  it("collapse keeps a resume control that returns to the same lesson", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "undo");
    await click(w, "guide-collapse");
    expect(coach(w).exists()).toBe(false);
    expect(w.get('[data-testid="guide-resume"]').text()).toContain("7 / 22");

    await click(w, "guide-resume");
    expect(coach(w).attributes("data-step-id")).toBe("undo");
  });

  it("start over asks first and resets guide state only", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "arrange");
    await click(w, "guide-start-over");
    expect(coach(w).attributes("data-step-id")).toBe("arrange");
    await click(w, "guide-start-over-cancel");
    expect(coach(w).attributes("data-step-id")).toBe("arrange");

    await click(w, "guide-start-over");
    await click(w, "guide-start-over-confirm");
    expect(coach(w).attributes("data-step-id")).toBe("welcome");
    expect(useEditorOnboardingStore().progress.reviewed).toEqual(["welcome"]);
    expect(calls).not.toContain("execute");
  });

  it("a pending progress save is flushed when the window hides", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await click(w, "guide-next");
    expect(saved).toBeNull(); // still inside the 400 ms debounce

    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(saved).toMatchObject({ currentStepId: "media", active: true });
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    w.unmount();
  });

  it("the coach renders the native lesson copy, never the reference's", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    for (const id of STEP_IDS) {
      const text = coach(w).text();
      for (const untrue of [/download/i, /three-minute/i, /\bsample\b/i, /built-in project/i, /Open Add track/, /Open Project/, /Browser/]) {
        expect(text, `${id}: ${String(untrue)}`).not.toMatch(untrue);
      }
      await click(w, "guide-next");
    }
  });
});
