/**
 * The guided-onboarding store (Task 55; F-46, F-47; ONBOARDING.md § State
 * and persistence), its wire decoder and its two port methods. Progress is
 * an app-wide preference persisted through `editor_get_guide_progress` /
 * `editor_save_guide_progress` — never the project, never Undo — so every
 * test here drives the store through an injected fake port and reads what
 * it SENT, not only what it holds.
 *
 * The retired-step map is mocked to one entry (`trim` was an "edit"
 * lesson): the shipped map is empty, and a fallback test against an empty
 * map could only ever prove the "no chapter known" arm.
 */
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/editor/guide/retired-steps.json", () => ({ default: { trim: "edit" } }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { decodeGuideProgress } from "../src/editor/decodeGuide";
import type { GuideStepId } from "../src/editor/guide/content";
import { CONTENT_REVISION, GUIDE_STEPS, STEP_TARGETS } from "../src/editor/guide/content";
import type { EditorPort } from "../src/editor/port";
import { createTauriEditorPort, EditorPortError } from "../src/editor/port";
import type { GuideProgress } from "../src/editorTypes";
import { logWarning } from "../src/logging";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

function stored(overrides: Partial<GuideProgress> = {}): GuideProgress {
  return {
    contentRevision: 1,
    currentStepId: null,
    reviewed: [],
    explored: [],
    invitationDismissed: false,
    active: false,
    collapsed: false,
    completed: false,
    preferences: { dimming: false, motion: "reduced" },
    ...overrides,
  };
}

let saves: GuideProgress[];

function install(overrides: Partial<EditorPort> = {}, reply: GuideProgress = stored()) {
  saves = [];
  useEditorProjectStore().setPort(
    fakeEditorPort({
      getGuideProgress: () => Promise.resolve(reply),
      saveGuideProgress: (progress) => {
        saves.push(structuredClone(progress));
        return Promise.resolve();
      },
      ...overrides,
    }),
  );
  return useEditorOnboardingStore();
}

beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
  vi.mocked(logWarning).mockClear();
});
afterEach(() => {
  vi.useRealTimers();
  clearMocks();
});

async function settle() {
  await vi.runAllTimersAsync();
}

describe("editorOnboarding — resume and fallback", () => {
  it("progress resumes at the exact step id", async () => {
    const guide = install({}, stored({ currentStepId: "fades", reviewed: ["welcome", "fades"] }));
    await guide.load();

    guide.start();

    expect(guide.progress.currentStepId).toBe("fades");
    // "fades" also OPENS its chapter, so it cannot tell exact resumption from
    // a chapter-start fallback; "audio" (second in "polish") can.
    guide.next();
    guide.pause();
    guide.start();
    expect(guide.progress.currentStepId).toBe("audio");
    expect(STEP_TARGETS[guide.progress.currentStepId as GuideStepId]).toBe("mixer");
    expect(guide.progress.active).toBe(true);
  });

  it("resumes mid-chapter, not at the chapter start", async () => {
    const guide = install({}, stored({ currentStepId: "arrange" }));
    await guide.load();
    guide.start();
    expect(guide.progress.currentStepId).toBe("arrange");
    expect(GUIDE_STEPS[guide.stepIndex].chapter).toBe("edit");
  });

  it("unknown ids fall back to the chapter's first step", async () => {
    const guide = install({}, stored({ currentStepId: "trim", reviewed: ["trim", "welcome"], explored: ["C:\\x"] }));
    await guide.load();

    // `trim` is mapped to chapter "edit", whose first lesson is "select".
    expect(guide.progress.currentStepId).toBe("select");
    expect(guide.progress.reviewed).toEqual(["welcome"]);
    expect(guide.progress.explored).toEqual([]);
    // Unrelated preferences survive the fallback.
    expect(guide.progress.preferences).toEqual({ dimming: false, motion: "reduced" });
  });

  it("an id no chapter claims falls back to the first lesson", async () => {
    const guide = install({}, stored({ currentStepId: "gone" }));
    await guide.load();
    expect(guide.progress.currentStepId).toBe("welcome");
  });
});

describe("editorOnboarding — storage", () => {
  it("storage failure reports session only and does not throw", async () => {
    const guide = install({
      getGuideProgress: () => Promise.reject(new EditorPortError({ code: "internal", message: "no", retryable: false, operationId: "op" })),
      saveGuideProgress: () => Promise.reject(new EditorPortError({ code: "diskFull", message: "full", retryable: true, operationId: "op" })),
    });

    await expect(guide.load()).resolves.toBeUndefined();
    expect(guide.sessionOnly).toBe(true);
    expect(guide.progress.currentStepId).toBeNull();

    // The guide still works for this session…
    guide.start();
    guide.next();
    await settle();
    expect(guide.progress.currentStepId).toBe("media");
    expect(guide.sessionOnly).toBe(true);
    expect(logWarning).toHaveBeenCalled();
  });

  it("a later save that lands clears the session-only flag", async () => {
    let fail = true;
    const guide = install({
      saveGuideProgress: () => (fail ? Promise.reject(new Error("locked")) : Promise.resolve()),
    });
    await guide.load();
    guide.start();
    await settle();
    expect(guide.sessionOnly).toBe(true);

    fail = false;
    guide.next();
    await settle();
    expect(guide.sessionOnly).toBe(false);
  });

  it("persistence is debounced into one save carrying the latest state", async () => {
    const guide = install();
    await guide.load();

    guide.start();
    guide.next();
    guide.next();
    expect(saves).toHaveLength(0);
    await settle();

    expect(saves).toHaveLength(1);
    expect(saves[0].currentStepId).toBe("preview");
    expect(saves[0].contentRevision).toBe(CONTENT_REVISION);
    expect(saves[0].reviewed).toEqual(["welcome", "media", "preview"]);
  });

  it("suspend and resume are transient and never persisted", async () => {
    const guide = install();
    await guide.load();
    guide.start();
    await settle();
    const before = saves.length;

    guide.suspend();
    expect(guide.suspended).toBe(true);
    guide.resume();
    expect(guide.suspended).toBe(false);
    await settle();

    expect(saves).toHaveLength(before);
    expect(Object.keys(saves[0])).not.toContain("suspended");
  });
});

describe("editorOnboarding — navigation", () => {
  it("marking a control explored never advances the lesson", async () => {
    const guide = install();
    await guide.load();
    guide.start();

    guide.markExplored("welcome");
    guide.markExplored("split");
    guide.markExplored("not-a-lesson");

    expect(guide.progress.currentStepId).toBe("welcome");
    expect(guide.progress.explored).toEqual(["welcome", "split"]);
  });

  it("back stops at the first lesson and next completes the last", async () => {
    const guide = install({}, stored({ currentStepId: "help" }));
    await guide.load();
    guide.start();

    guide.next();
    expect(guide.progress.completed).toBe(true);
    expect(guide.progress.active).toBe(false);

    guide.restart();
    guide.back();
    expect(guide.progress.currentStepId).toBe("welcome");
  });

  it("pause, collapse and the invitation keep the current lesson", async () => {
    const guide = install({}, stored({ currentStepId: "tracks" }));
    await guide.load();
    guide.start();

    guide.collapse();
    expect(guide.progress).toMatchObject({ active: true, collapsed: true, currentStepId: "tracks" });
    guide.pause();
    expect(guide.progress).toMatchObject({ active: false, collapsed: false, currentStepId: "tracks" });
    guide.dismissInvitation();
    expect(guide.progress.invitationDismissed).toBe(true);
  });

  it("restart resets guide state only", async () => {
    const guide = install(
      {},
      stored({ currentStepId: "render", reviewed: ["welcome", "render"], explored: ["render"], completed: true, invitationDismissed: true }),
    );
    await guide.load();

    guide.restart();

    expect(guide.progress).toEqual({
      contentRevision: CONTENT_REVISION,
      currentStepId: "welcome",
      reviewed: ["welcome"],
      explored: [],
      invitationDismissed: true,
      active: true,
      collapsed: false,
      completed: false,
      preferences: { dimming: false, motion: "reduced" },
    });
  });
});

describe("decodeGuideProgress", () => {
  // The literal Rust's `guide_progress_serializes_to_the_contract_literal`
  // pins, spelled the same way.
  const LITERAL = {
    contentRevision: 1,
    currentStepId: null,
    reviewed: ["welcome"],
    explored: [],
    invitationDismissed: true,
    active: false,
    collapsed: true,
    completed: false,
    preferences: { dimming: true, motion: "full" },
  };

  it("decodes the contract literal", () => {
    expect(decodeGuideProgress(LITERAL)).toEqual(LITERAL);
  });

  it("refuses a missing key, a wrong type and an unknown motion", () => {
    const { currentStepId: _omit, ...missing } = LITERAL;
    expect(() => decodeGuideProgress(missing)).toThrow(/currentStepId/);
    expect(() => decodeGuideProgress({ ...LITERAL, reviewed: "welcome" })).toThrow(/reviewed/);
    expect(() => decodeGuideProgress({ ...LITERAL, preferences: { dimming: true, motion: "fast" } })).toThrow(/motion/);
    expect(() => decodeGuideProgress({ ...LITERAL, contentRevision: -1 })).toThrow(/contentRevision/);
  });

  it("the port sends no arguments to read and the whole document to save", async () => {
    vi.useRealTimers();
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_get_guide_progress") return LITERAL;
      if (cmd === "editor_save_guide_progress") return null;
      throw new Error(`unexpected command ${cmd}`);
    });
    const port = createTauriEditorPort();

    const read = await port.getGuideProgress();
    await port.saveGuideProgress(read);

    expect(calls).toEqual([
      { cmd: "editor_get_guide_progress", args: {} },
      { cmd: "editor_save_guide_progress", args: { progress: LITERAL } },
    ]);
  });
});
