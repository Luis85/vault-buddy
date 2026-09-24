/**
 * The learning center (Task 57; F-47; ONBOARDING.md: "chapter navigation,
 * individual lessons, searchable quick answers, shortcuts,
 * progress/preferences and restart"; SCREENS 11).
 *
 * The pure half (`answers.ts`) is tested directly: the answers are the
 * lessons AS THE COACH TELLS THEM (`lessonCopy`, never the raw browser-
 * reference sentences GAP-203 corrected), and the shortcut table is built
 * from `shortcuts.ts` itself. The mounted half drives the real editor shell
 * over a recording port, so "the learning center never edits, never starts
 * the guide on a restore and never touches a device" is a statement about
 * what was SENT.
 */
import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { ACTION_LABELS } from "../src/editor/actionMeta";
import { displayCombo, OTHER_KEYS, QUICK_ANSWERS, searchAnswers, SHORTCUT_TABLE } from "../src/editor/guide/answers";
import { GUIDE_CHAPTERS, GUIDE_STEPS, lessonCopy } from "../src/editor/guide/content";
import type { EditorPort } from "../src/editor/port";
import { createTauriEditorPort } from "../src/editor/port";
import { SHORTCUTS } from "../src/editor/shortcuts";
import type { GuideProgress } from "../src/editorTypes";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { MountedShell, shellPort } from "./helpers/guideShell";

enableAutoUnmount(afterEach);

const STEP_IDS = GUIDE_STEPS.map((s) => s.id);

// ---- answers.ts -----------------------------------------------------------

describe("quick answers", () => {
  it("search finds a lesson by a word in its body", () => {
    // "gold handles" is only in the fades lesson's (corrected) body.
    expect(searchAnswers("gold handles").map((a) => a.stepId)).toEqual(["fades"]);
    // A word from a lesson body that the title does not contain.
    expect(searchAnswers("ruler").map((a) => a.stepId)).toContain("preview");
  });

  it("search ignores case and diacritics", () => {
    expect(searchAnswers("GÖLD HÄNDLES").map((a) => a.stepId)).toEqual(["fades"]);
    expect(searchAnswers("  Fädé  ").map((a) => a.stepId)).toContain("fades");
    expect(searchAnswers("")).toHaveLength(22);
  });

  // GAP-203: the answers come from the coach's corrected copy. Each phrase
  // below — the coach suite's own list (editorGuideCoach.test.ts) —
  // appears in the raw browser-reference text ONLY inside a sentence the
  // app overrides, so finding it would mean the untrue text is back.
  it("never finds a word only an untrue original sentence holds", () => {
    const raw = JSON.stringify(GUIDE_STEPS).toLowerCase();
    const untrue = ["download", "three-minute", "sample", "built-in project", "Open Add track", "Open Project", "Browser"];
    for (const phrase of untrue) {
      const word = phrase.toLowerCase();
      expect(raw).toContain(word);
      expect({ word, found: searchAnswers(word) }).toEqual({ word, found: [] });
    }
  });

  it("is one answer per lesson, in the coach's words", () => {
    expect(QUICK_ANSWERS.map((a) => a.stepId)).toEqual(STEP_IDS);
    for (const a of QUICK_ANSWERS) {
      const copy = lessonCopy(a.stepId);
      expect(a).toEqual({
        stepId: a.stepId,
        question: GUIDE_STEPS.find((s) => s.id === a.stepId)?.title,
        label: copy.label,
        answer: copy.body,
        tip: copy.tip,
      });
    }
  });
});

describe("the shortcut table", () => {
  it("shortcut table matches shortcuts.ts", () => {
    // Every binding once, under the action it is bound to, and nothing else.
    const listed = SHORTCUT_TABLE.flatMap((row) => row.keys.map((k) => [k, row.actionId] as const));
    const expected = [...SHORTCUTS].map(([combo, id]) => [displayCombo(combo), id] as const);
    expect(listed).toEqual(expected);
    expect(new Set(SHORTCUT_TABLE.map((r) => r.actionId)).size).toBe(SHORTCUT_TABLE.length);
    expect(displayCombo("ctrl+shift+z")).toBe("Ctrl+Shift+Z");
    expect(displayCombo("?")).toBe("?");
    expect(displayCombo("f1")).toBe("F1");
    const byId = Object.fromEntries(SHORTCUT_TABLE.map((r) => [r.actionId, r]));
    expect(byId.split).toEqual({ actionId: "split", label: ACTION_LABELS.split, keys: ["S"] });
    expect(byId.help).toEqual({ actionId: "help", label: "Start or resume the guided walkthrough", keys: ["F1", "?"] });
    expect(OTHER_KEYS.map((k) => k.keys)).toEqual([["Shift+F10", "Menu"], ["Esc"]]);
  });
});

// ---- the mounted learning center ---------------------------------------------

function fresh(overrides: Partial<GuideProgress> = {}): GuideProgress {
  return {
    contentRevision: 1, currentStepId: null, reviewed: [], explored: [], invitationDismissed: true,
    active: false, collapsed: false, completed: false, preferences: { dimming: true, motion: "system" },
    ...overrides,
  };
}

let calls: string[];
let saved: GuideProgress | null;

function recording(port: EditorPort): EditorPort {
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

async function install(progress: GuideProgress = fresh(), overrides: Partial<EditorPort> = {}): Promise<void> {
  calls = [];
  saved = null;
  const port = recording(
    shellPort({
      getGuideProgress: () => Promise.resolve(progress),
      saveGuideProgress: (p) => {
        saved = structuredClone(p);
        return Promise.resolve();
      },
      ...overrides,
    }),
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

async function click(w: VueWrapper, testid: string): Promise<void> {
  await w.get(`[data-testid="${testid}"]`).trigger("click");
  await flushPromises();
}

async function openCenter(w: VueWrapper): Promise<void> {
  await click(w, "editor-header-help");
  await click(w, "editor-help-learning-center");
}

const center = (w: VueWrapper) => w.find('[data-testid="learning-center"]');
const coach = (w: VueWrapper) => w.find('[data-testid="guide-coach"]');

const ORIGINAL_MEDIA_DEVICES = Object.getOwnPropertyDescriptor(navigator, "mediaDevices");

beforeEach(async () => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(120, 90, 200, 40));
  await install();
});
afterEach(() => {
  vi.restoreAllMocks();
  clearMocks();
  if (ORIGINAL_MEDIA_DEVICES) Object.defineProperty(navigator, "mediaDevices", ORIGINAL_MEDIA_DEVICES);
  else Reflect.deleteProperty(navigator, "mediaDevices");
});

describe("the Help menu", () => {
  it("offers the learning center, resuming the walkthrough and the shortcuts — nothing dead", async () => {
    const w = await mountEditor();
    const toggle = w.get('[data-testid="editor-header-help"]');
    expect(toggle.attributes("aria-haspopup")).toBe("menu");

    await click(w, "editor-header-help");

    const items = w.findAll('[role="menu"][aria-label="Help"] [role="menuitem"]');
    expect(items.map((i) => i.text())).toEqual(["Learning center", "Resume walkthrough", "Keyboard shortcuts"]);
  });

  it("Resume walkthrough starts the coach at the saved lesson", async () => {
    await install(fresh({ currentStepId: "fades", reviewed: STEP_IDS.slice(0, 13) }));
    const w = await mountEditor();

    await click(w, "editor-header-help");
    await click(w, "editor-help-resume");

    expect(coach(w).attributes("data-step-id")).toBe("fades");
  });

  it("Keyboard shortcuts opens the learning center on its shortcut table", async () => {
    const w = await mountEditor();
    await click(w, "editor-header-help");
    await click(w, "editor-help-shortcuts");

    const rows = w.findAll('[data-testid="learning-shortcut"]');
    expect(rows.map((r) => r.attributes("data-action"))).toEqual(SHORTCUT_TABLE.map((r) => r.actionId));
    expect(rows[0].text()).toContain("S");
  });
});

describe("the learning center", () => {
  it("shows progress from lessons read, and a finished guide from all 22 read — not from completed", async () => {
    await install(fresh({ reviewed: STEP_IDS.slice(0, 5), completed: true }));
    let w = await mountEditor();
    await openCenter(w);
    expect(w.get('[data-testid="learning-progress"]').text()).toContain("5 / 22");
    expect(w.find('[data-testid="learning-finished"]').exists()).toBe(false);
    w.unmount();

    setActivePinia(createPinia());
    await install(fresh({ reviewed: [...STEP_IDS], completed: false }));
    w = await mountEditor();
    await openCenter(w);
    expect(w.get('[data-testid="learning-progress"]').text()).toContain("22 / 22");
    expect(w.find('[data-testid="learning-finished"]').exists()).toBe(true);
  });

  // Fix round 1: the button says what start() will do — a finished guide
  // is revisited from lesson 1, so it must not promise a resume.
  it("the walkthrough button names what it does: start, resume, or start again", async () => {
    const label = async (progress: GuideProgress) => {
      setActivePinia(createPinia());
      await install(progress);
      const w = await mountEditor();
      await openCenter(w);
      const text = w.get('[data-testid="learning-resume"]').text();
      w.unmount();
      return text;
    };
    expect(await label(fresh())).toBe("Start walkthrough");
    expect(await label(fresh({ currentStepId: "split", reviewed: ["welcome", "split"] }))).toBe("Resume walkthrough");
    expect(await label(fresh({ currentStepId: "help", reviewed: [...STEP_IDS], completed: true }))).toBe(
      "Start walkthrough again",
    );
  });

  // Fix round 1: "read" is spoken, not only drawn — visually-hidden text in
  // the lesson button's own name, the check mark hidden from assistive tech.
  it("a read lesson says so in its accessible name", async () => {
    await install(fresh({ reviewed: ["welcome"] }));
    const w = await mountEditor();
    await openCenter(w);

    const read = w.get('[data-testid="learning-lesson-welcome"]');
    expect(read.get(".sr-only").text()).toBe("Read:");
    expect(read.get('[aria-hidden="true"]').text()).toBe("✓");
    expect(w.get('[data-testid="learning-lesson-media"]').find(".sr-only").exists()).toBe(false);
  });

  it("lists every chapter and lesson; a lesson jump starts the coach there", async () => {
    const w = await mountEditor();
    await openCenter(w);

    expect(w.findAll('[data-testid^="learning-chapter-"]').map((c) => c.attributes("data-testid"))).toEqual(
      GUIDE_CHAPTERS.map((c) => `learning-chapter-${c.id}`),
    );
    expect(w.findAll('[data-testid^="learning-lesson-"]')).toHaveLength(22);

    await click(w, "learning-lesson-captions");

    expect(center(w).exists()).toBe(false);
    expect(coach(w).attributes("data-step-id")).toBe("captions");
    expect(useEditorOnboardingStore().progress).toMatchObject({ active: true, completed: false });
  });

  it("a chapter jump starts at that chapter's first lesson", async () => {
    const w = await mountEditor();
    await openCenter(w);
    await click(w, "learning-chapter-polish");
    expect(coach(w).attributes("data-step-id")).toBe("fades");
  });

  it("the quick-answer search filters the list and says when nothing matches", async () => {
    const w = await mountEditor();
    await openCenter(w);
    await click(w, "learning-tab-answers");

    await w.get('[data-testid="learning-search"]').setValue("gold handles");
    expect(w.findAll('[data-testid="learning-answer"]').map((a) => a.attributes("data-step-id"))).toEqual(["fades"]);

    await w.get('[data-testid="learning-search"]').setValue("download");
    expect(w.findAll('[data-testid="learning-answer"]')).toHaveLength(0);
    expect(w.find('[data-testid="learning-no-answers"]').exists()).toBe(true);
  });

  it("start over asks first and leaves the project untouched", async () => {
    await install(fresh({ currentStepId: "render", reviewed: STEP_IDS.slice(0, 20), explored: ["render"] }));
    const w = await mountEditor();
    const revision = useEditorProjectStore().snapshot?.revision;
    await openCenter(w);

    await click(w, "guide-start-over");
    // Asked, not done: nothing reset yet.
    expect(useEditorOnboardingStore().progress.reviewed).toHaveLength(20);
    expect(w.find('[data-testid="guide-start-over-confirm"]').exists()).toBe(true);

    await click(w, "guide-start-over-confirm");
    await useEditorOnboardingStore().flush();

    expect(useEditorOnboardingStore().progress).toMatchObject({
      currentStepId: "welcome", reviewed: ["welcome"], explored: [], preferences: { dimming: true, motion: "system" },
    });
    expect(saved?.reviewed).toEqual(["welcome"]);
    // The project: same revision, and no edit, save or file request sent.
    expect(useEditorProjectStore().snapshot?.revision).toBe(revision);
    expect(calls.filter((c) => ["execute", "saveProject", "exportPackage"].includes(c))).toEqual([]);
  });

  it("preferences change dimming and motion and are saved", async () => {
    const w = await mountEditor();
    await openCenter(w);

    await w.get('[data-testid="learning-pref-dimming"]').setValue(false);
    await w.get('[data-testid="learning-pref-motion"]').setValue("reduced");
    await useEditorOnboardingStore().flush();

    expect(saved?.preferences).toEqual({ dimming: false, motion: "reduced" });
  });
});

describe("the progress file", () => {
  it("Save progress file sends the current progress and reports the file it landed in", async () => {
    const exportGuideProgress = vi.fn((_p: GuideProgress) => Promise.resolve("backup.json"));
    await install(fresh({ currentStepId: "split", reviewed: ["welcome", "split"] }), { exportGuideProgress });
    const w = await mountEditor();
    await openCenter(w);

    await click(w, "learning-save-file");

    expect(exportGuideProgress).toHaveBeenCalledTimes(1);
    expect(exportGuideProgress.mock.calls[0][0]).toMatchObject({ currentStepId: "split", reviewed: ["welcome", "split"] });
    expect(w.get('[data-testid="learning-file-status"]').text()).toBe(
      "Saved to backup.json. It holds lesson progress only — no project or media.",
    );
  });

  it("restoring progress does not start the guide or touch devices", async () => {
    const getUserMedia = vi.fn();
    const enumerateDevices = vi.fn();
    Object.defineProperty(navigator, "mediaDevices", { value: { getUserMedia, enumerateDevices }, configurable: true });
    const restored = fresh({
      currentStepId: "layout", reviewed: ["welcome", "media", "layout"], active: true, collapsed: true,
      invitationDismissed: false, preferences: { dimming: false, motion: "full" },
    });
    await install(fresh(), { importGuideProgress: () => Promise.resolve(restored) });
    const w = await mountEditor();
    const revision = useEditorProjectStore().snapshot?.revision;
    await openCenter(w);

    await click(w, "learning-restore-file");
    await click(w, "learning-close");
    await useEditorOnboardingStore().flush();

    const guide = useEditorOnboardingStore();
    expect(guide.progress).toMatchObject({
      currentStepId: "layout", reviewed: ["welcome", "media", "layout"], active: false, collapsed: false,
      invitationDismissed: true, preferences: { dimming: false, motion: "full" },
    });
    expect(saved).toMatchObject({ currentStepId: "layout", active: false });
    expect(coach(w).exists()).toBe(false);
    expect(w.find('[data-testid="guide-invitation"]').exists()).toBe(false);
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(enumerateDevices).not.toHaveBeenCalled();
    expect(calls.filter((c) => c.startsWith("webcam") || c === "execute")).toEqual([]);
    expect(useEditorProjectStore().snapshot?.revision).toBe(revision);
  });

  it("a refused or dismissed restore changes nothing", async () => {
    const importGuideProgress = vi
      .fn<() => Promise<GuideProgress | null>>()
      .mockRejectedValueOnce(new Error("That file is not Vault Buddy guide progress."))
      .mockResolvedValueOnce(null);
    await install(fresh({ currentStepId: "split", reviewed: ["welcome", "split"] }), { importGuideProgress });
    const w = await mountEditor();
    await openCenter(w);
    const before = JSON.parse(JSON.stringify(useEditorOnboardingStore().progress)) as GuideProgress;

    await click(w, "learning-restore-file");
    expect(w.get('[data-testid="learning-file-status"]').text()).toContain("That file is not Vault Buddy guide progress.");
    await click(w, "learning-restore-file");
    expect(w.get('[data-testid="learning-file-status"]').text()).toBe("Nothing was restored — the file dialog was closed.");

    expect(useEditorOnboardingStore().progress).toEqual(before);
  });

  it("the port sends the progress to export and nothing to import", async () => {
    const LITERAL = fresh({ currentStepId: "welcome", reviewed: ["welcome"] });
    const sent: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      sent.push({ cmd, args });
      if (cmd === "editor_export_guide_progress") return "backup.json";
      if (cmd === "editor_import_guide_progress") return LITERAL;
      throw new Error(`unexpected command ${cmd}`);
    });
    const port = createTauriEditorPort();

    expect(await port.exportGuideProgress(LITERAL)).toBe("backup.json");
    expect(await port.importGuideProgress()).toEqual(LITERAL);

    expect(sent).toEqual([
      { cmd: "editor_export_guide_progress", args: { progress: LITERAL } },
      { cmd: "editor_import_guide_progress", args: {} },
    ]);
  });
});
