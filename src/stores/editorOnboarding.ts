/**
 * The guided onboarding's state (Task 55; F-46, F-47; ONBOARDING.md § State
 * and persistence; ADR R16): which lesson is current, which were read
 * (`reviewed`) and which controls were tried (`explored` — separate, and
 * trying a control NEVER advances the lesson), the invitation, active/
 * collapsed, completion and the presentation preferences.
 *
 * **Apart from the project.** Progress is an app-wide preference persisted
 * through `editor_get_guide_progress`/`editor_save_guide_progress`
 * (`editor-prefs\guide-progress.json`), never the project document, a
 * render snapshot or Undo — no action here calls `editor_execute`.
 * `suspended` (a modal dialog has the screen) is transient and is not part
 * of `progress`, so it can never be persisted.
 *
 * **Storage is best effort.** A read or save that fails sets `sessionOnly`
 * (the header says so) and logs; it never throws into the editor, and the
 * guide keeps working for this session. A later save that lands clears it.
 * Saves are debounced: a quick Next/Next/Next is one write of the latest
 * state.
 *
 * **Parked actions.** The coach, the invitation and the learning center
 * that call Next/Back/Pause/Collapse/… are Task 56's; until they land, the
 * actions below are exercised only by `tests/editorOnboardingStore.test.ts`,
 * so each carries a `fallow-ignore-next-line unused-store-member` that Task
 * 56 removes (fallow reports a suppression that has gone stale).
 */
import { defineStore } from "pinia";

import type { GuideStepId } from "../editor/guide/content";
import { CONTENT_REVISION, GUIDE_STEPS, isGuideStepId, resolveStepId } from "../editor/guide/content";
import type { GuidePreferences, GuideProgress } from "../editorTypes";
import { logWarning } from "../logging";
import { useEditorProjectStore } from "./editorProject";

const PERSIST_DEBOUNCE_MS = 400;

function fresh(preferences: GuidePreferences = { dimming: true, motion: "system" }): GuideProgress {
  return {
    contentRevision: CONTENT_REVISION,
    currentStepId: null,
    reviewed: [],
    explored: [],
    invitationDismissed: false,
    active: false,
    collapsed: false,
    completed: false,
    preferences: { ...preferences },
  };
}

/** Lessons only, first occurrence kept. */
function lessons(ids: string[]): string[] {
  return [...new Set(ids.filter(isGuideStepId))];
}

/** A stored document, resolved against THIS build's lessons. */
function hydrate(stored: GuideProgress): GuideProgress {
  return {
    ...stored,
    contentRevision: CONTENT_REVISION,
    currentStepId: stored.currentStepId === null ? null : resolveStepId(stored.currentStepId),
    reviewed: lessons(stored.reviewed),
    explored: lessons(stored.explored),
    preferences: { ...stored.preferences },
  };
}

let persistTimer: ReturnType<typeof setTimeout> | null = null;

export const useEditorOnboardingStore = defineStore("editorOnboarding", {
  state: () => ({
    progress: fresh(),
    /** The saved progress has been read (or found unreadable). */
    loaded: false,
    /** Storage failed: progress lasts for this session only. */
    sessionOnly: false,
    /** A modal dialog has the screen; transient, never persisted. */
    suspended: false,
  }),
  getters: {
    /** The current lesson's position in `GUIDE_STEPS`, or -1. */
    stepIndex(state): number {
      return GUIDE_STEPS.findIndex((s) => s.id === state.progress.currentStepId);
    },
  },
  actions: {
    /** Reads the saved progress once per window. Never throws. */
    async load(): Promise<void> {
      if (this.loaded) return;
      try {
        this.progress = hydrate(await useEditorProjectStore().port.getGuideProgress());
      } catch (e) {
        this.sessionOnly = true;
        logWarning(`editor guide: could not read saved progress, this session only: ${String(e)}`);
      } finally {
        this.loaded = true;
      }
    },
    /** Starts, or resumes at the exact saved lesson. */
    start(): void {
      const p = this.progress;
      const resume = !p.completed && p.currentStepId !== null;
      p.active = true;
      p.collapsed = false;
      p.invitationDismissed = true;
      this.show(resume ? (p.currentStepId as GuideStepId) : GUIDE_STEPS[0].id);
    },
    // fallow-ignore-next-line unused-store-member
    next(): void {
      const i = this.stepIndex;
      if (i === GUIDE_STEPS.length - 1) {
        this.progress.completed = true;
        this.progress.active = false;
        this.progress.collapsed = false;
        this.persist();
        return;
      }
      this.show(GUIDE_STEPS[Math.max(0, i + 1)].id);
    },
    // fallow-ignore-next-line unused-store-member
    back(): void {
      const i = this.stepIndex;
      if (i > 0) this.show(GUIDE_STEPS[i - 1].id);
    },
    /** Closes the coach, keeping the lesson for Resume. */
    // fallow-ignore-next-line unused-store-member
    pause(): void {
      this.progress.active = false;
      this.progress.collapsed = false;
      this.persist();
    },
    /** Keeps a small resume control while the user works. */
    // fallow-ignore-next-line unused-store-member
    collapse(): void {
      this.progress.collapsed = true;
      this.persist();
    },
    // fallow-ignore-next-line unused-store-member
    dismissInvitation(): void {
      this.progress.invitationDismissed = true;
      this.persist();
    },
    /** A control was tried. Recorded only — the lesson stays put. */
    // fallow-ignore-next-line unused-store-member
    markExplored(id: string): void {
      if (!isGuideStepId(id) || this.progress.explored.includes(id)) return;
      this.progress.explored.push(id);
      this.persist();
    },
    /** Start over: guide state only. Preferences — and the fact the
     * invitation was already answered — are the person's, and stay. */
    // fallow-ignore-next-line unused-store-member
    restart(): void {
      this.progress = { ...fresh(this.progress.preferences), invitationDismissed: true };
      this.start();
    },
    // fallow-ignore-next-line unused-store-member
    suspend(): void {
      this.suspended = true;
    },
    // fallow-ignore-next-line unused-store-member
    resume(): void {
      this.suspended = false;
    },
    /** Internal: makes `id` current and records it as read. */
    show(id: GuideStepId): void {
      this.progress.currentStepId = id;
      if (!this.progress.reviewed.includes(id)) this.progress.reviewed.push(id);
      this.persist();
    },
    /** Debounced save of the latest progress. Never throws. */
    persist(): void {
      if (persistTimer !== null) clearTimeout(persistTimer);
      persistTimer = setTimeout(() => {
        persistTimer = null;
        void this.save();
      }, PERSIST_DEBOUNCE_MS);
    },
    async save(): Promise<void> {
      const p = this.progress;
      const snapshot: GuideProgress = {
        ...p,
        contentRevision: CONTENT_REVISION,
        reviewed: [...p.reviewed],
        explored: [...p.explored],
        preferences: { ...p.preferences },
      };
      try {
        await useEditorProjectStore().port.saveGuideProgress(snapshot);
        this.sessionOnly = false;
      } catch (e) {
        this.sessionOnly = true;
        logWarning(`editor guide: could not save progress, this session only: ${String(e)}`);
      }
    },
  },
});
