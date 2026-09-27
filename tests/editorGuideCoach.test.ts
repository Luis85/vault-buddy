/**
 * The guided walkthrough itself (Task 56; F-46, F-49; SCREENS 01, 10;
 * A23–A26): the first-use invitation, the coach that points at the REAL
 * control (its placement: `editorGuidePosition.test.ts`), keyboard ownership (F6, Escape)
 * and suspension under a modal dialog.
 *
 * Every walkthrough test mounts the whole editor shell (the slot filling
 * `EditorRoot` uses) over a recording port, so "the guide never edits"
 * is a statement about what was SENT, not what a store holds. happy-dom
 * has no layout, so `getBoundingClientRect` is stubbed to one asymmetric
 * box: the coach renders its highlight only for a target with a real box.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { dialogStackSize } from "../src/editor/dialogs";
import { GUIDE_STEPS, STEP_TARGETS } from "../src/editor/guide/content";
import { resolve } from "../src/editor/guide/targets";
import type { EditorPort } from "../src/editor/port";
import { checksDialogOpen } from "../src/editor/revealBus";
import type { GuideProgress } from "../src/editorTypes";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { MountedShell, shellPort } from "./helpers/guideShell";

enableAutoUnmount(afterEach);

// Placement arithmetic: tests/editorGuidePosition.test.ts.

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
/** Put back an own property a test overrode (or remove the override so
 * the prototype's shows through again), whatever the test's outcome. */
function restoreOwn(target: object, key: string, original: PropertyDescriptor | undefined): void {
  if (original) Object.defineProperty(target, key, original);
  else Reflect.deleteProperty(target, key);
}
const ORIGINAL_MEDIA_DEVICES = Object.getOwnPropertyDescriptor(navigator, "mediaDevices");
const ORIGINAL_VISIBILITY = Object.getOwnPropertyDescriptor(document, "visibilityState");

afterEach(() => {
  vi.restoreAllMocks();
  restoreOwn(navigator, "mediaDevices", ORIGINAL_MEDIA_DEVICES);
  restoreOwn(document, "visibilityState", ORIGINAL_VISIBILITY);
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

    // Help → Resume walkthrough (Task 57 made Help a menu).
    await click(w, "editor-header-help");
    await click(w, "editor-help-menu-item-resume");
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
    await click(second, "editor-help-menu-item-resume");
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
    expect(layers.map((l) => l.getAttribute("data-guide-layer")).sort()).toEqual(["card", "label", "ring"]);
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

  // Review I-1: on the fades (and layout) lesson F6 lands in a TEXT FIELD
  // (the section's first focusable is "Fade in (ms)"). F6 types nothing,
  // so it must still come back to the card from there — while every other
  // shortcut keeps leaving text fields alone.
  it("F6 returns to the card from a lesson control that is a text field", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "fades");
    const card = coach(w).element;
    (w.get('[data-testid="guide-coach-title"]').element as HTMLElement).focus();

    keydown(document.activeElement as Element, "F6");
    await flushPromises();
    expect(document.activeElement?.tagName).toBe("INPUT");
    expect(w.get('[data-testid="fades-section"]').element.contains(document.activeElement)).toBe(true);

    keydown(document.activeElement as Element, "F6");
    await flushPromises();
    expect(card.contains(document.activeElement)).toBe(true);

    // The text-field rule still holds for everything else: "?" typed in the
    // field neither opens nor moves the guide.
    const field = w.get('[data-testid="fades-section-fade-in"]').element as HTMLElement;
    field.focus();
    const typed = keydown(field, "?");
    expect(typed.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(field);
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

  // The tracks lesson's menu (Add track, visual-parity Task 17): Escape in
  // it closes the menu, and the guide stays on its lesson.
  it("Escape with the Add track menu open closes the menu, not the guide", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "tracks");
    await click(w, "timeline-add-track");
    expect(w.find('[data-testid="timeline-add-track-panel-root"]').exists()).toBe(true);

    keydown(document.activeElement ?? w.get('[data-testid="timeline-add-track"]').element, "Escape");
    await flushPromises();
    expect(w.find('[data-testid="timeline-add-track-panel-root"]').exists()).toBe(false);
    expect(coach(w).attributes("data-step-id")).toBe("tracks");
  });

  it("collapse keeps a resume control that returns to the same lesson", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "undo");
    await click(w, "guide-collapse");
    expect(coach(w).exists()).toBe(false);
    expect(w.get('[data-testid="guide-resume"]').text()).toContain("7/22 · You can take an edit back.");

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
    w.unmount();
  });

  it("the coach renders the native lesson copy, never the reference's", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    // "Open Add track" is true since visual-parity Task 16 built the ruler's
    // Add track menu; the tracks lesson reads it verbatim again.
    for (const id of STEP_IDS) {
      const text = coach(w).text();
      for (const untrue of [/download/i, /three-minute/i, /\bsample\b/i, /built-in project/i, /Open Project/, /Browser/]) {
        expect(text, `${id}: ${String(untrue)}`).not.toMatch(untrue);
      }
      await click(w, "guide-next");
    }
  });
});

// ---- visual-parity Task 23: the concept's invitation, coach and label ------

describe("the concept's guide surfaces (§9.1–9.2)", () => {
  it("the invitation carries the overline, the compass, the heading, both answers and the footer", async () => {
    const w = await mountEditor();
    const invitation = w.get('[data-testid="guide-invitation"]');
    expect(invitation.text()).toContain("NEW HERE? START HERE.");
    expect(invitation.get("h2").text()).toBe("A little guidance.A clearer first edit.");
    expect(invitation.find("h2 br").exists()).toBe(true);
    expect(invitation.text()).toContain("Get to know the editor, one useful step at a time.");
    expect(w.get('[data-testid="guide-invitation-start"]').text()).toBe("Show me around");
    expect(w.get('[data-testid="guide-invitation-dismiss"]').text()).toBe("Not now");
    expect(invitation.text()).toContain("Always available from Help · No editing required");
    expect(invitation.classes()).toEqual(expect.arrayContaining(["w-[350px]", "right-6", "top-[76px]"]));
  });

  it("the card: chapter, GUIDED WALKTHROUGH with its counter, the task's three voices and D10's storage words", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    const card = coach(w);
    expect(w.get('[data-testid="guide-coach-contents"]').text()).toBe("Find your way");
    expect(card.text()).toContain("GUIDED WALKTHROUGH");
    expect(w.get('[data-testid="guide-coach-count"]').classes()).toContain("vb-mono");
    // No task in the welcome lesson: the plain prompt.
    expect(w.get('[data-testid="guide-coach-task"]').attributes("data-voice")).toBe("prompt");
    expect(w.get('[data-testid="guide-coach-task"]').text()).toBe("Read this step, then continue when you are ready.");
    expect(w.get('[data-testid="guide-coach-storage"]').text()).toBe("Progress remembered on this PC");
    expect(card.text()).toContain("F6 to focus control");
    expect(w.get('[data-testid="guide-next"]').text()).toBe("Next");
    expect(w.get('[data-testid="guide-coach-progress"]').attributes("style")).toContain(`width: ${100 / 22}%`);

    // An optional edit speaks in gold; trying the control turns it teal.
    await goTo(w, "fades");
    expect(w.get('[data-testid="guide-coach-task"]').attributes("data-voice")).toBe("edit");
    expect(w.get('[data-testid="guide-coach-task"]').text()).toBe("Optional edit: try a 0.5-second fade. Undo remains available.");
    await w.get('[data-testid="fades-section"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="guide-coach-task"]').attributes("data-voice")).toBe("tried");
    expect(w.get('[data-testid="guide-coach-task"]').text()).toBe("Control explored. Continue whenever you are ready.");

    await goTo(w, "help");
    expect(w.get('[data-testid="guide-next"]').text()).toBe("Finish guide");
  });

  it("Back carries its reason on the first lesson", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    const back = w.get('[data-testid="guide-back"]');
    expect(back.attributes("disabled")).toBeDefined();
    expect(back.attributes("title")).toBe("This is the first step.");
  });

  it("says Session only when progress cannot be stored", async () => {
    const w = await mountEditor();
    useEditorOnboardingStore().sessionOnly = true;
    await click(w, "guide-invitation-start");
    expect(w.get('[data-testid="guide-coach-storage"]').text()).toBe("Session only");
  });

  it("the chapter title pauses the walkthrough and opens the learning center's chapters", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "split");
    await click(w, "guide-coach-contents");

    expect(useEditorOnboardingStore().progress).toMatchObject({ active: false, currentStepId: "split" });
    expect(w.find('[data-testid="learning-center"]').exists()).toBe(true);
    expect(w.get('[data-testid="learning-tab-walkthrough"]').attributes("aria-selected")).toBe("true");
    expect(calls).not.toContain("execute");
  });

  it("the target label names the lesson's control", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "fades");
    const label = w.get('[data-testid="guide-target-label"]');
    expect(label.text()).toBe("Fade controls");
    expect(label.attributes("aria-hidden")).toBe("true");
  });

  it("the minimized bar's ✕ pauses, and Help resumes the same lesson", async () => {
    const w = await mountEditor();
    await click(w, "guide-invitation-start");
    await goTo(w, "undo");
    await click(w, "guide-collapse");
    await click(w, "guide-mini-close");
    expect(w.find('[data-testid="guide-mini"]').exists()).toBe(false);
    expect(useEditorOnboardingStore().progress).toMatchObject({ active: false, currentStepId: "undo" });
  });
});
