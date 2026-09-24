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
 * guide keeps working for this session. A later save that lands clears it
 * — unless the READ failed (Task 56): then a file exists that this session
 * could not read, and writing fresh progress over it would lose the user's
 * real place, so nothing is written for the rest of the session and
 * `sessionOnly` stays. Saves are debounced: a quick Next/Next/Next is one
 * write of the latest state, and `flush()` writes a pending one at once
 * (the editor shell calls it when the window hides or the shell unmounts,
 * the close guard before it hides the window).
 *
 * **Callers (Task 56).** `GuideInvitation` (start, dismissInvitation),
 * `GuideCoach` (next, back, pause, collapse, restart, markExplored — a
 * click on the highlighted control), `GuideHelpButton` and F1/? (start),
 * and every `DialogHost` (suspend on open, resume on close: a depth, so
 * a confirm stacked on a dialog keeps the coach suspended until the last
 * one closes).
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
    /** Storage exists and could not be read: never write over it. */
    readFailed: false,
    /** Open modal dialogs; transient, never persisted. */
    suspendDepth: 0,
  }),
  getters: {
    /** A modal dialog has the screen. */
    suspended(state): boolean {
      return state.suspendDepth > 0;
    },
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
        this.readFailed = true;
        logWarning(`editor guide: could not read saved progress, this session only: ${String(e)}`);
      } finally {
        this.loaded = true;
      }
    },
    /** Starts, or resumes at the exact saved lesson. A finished guide is
     * revisited from its first lesson (what was read stays read). */
    start(): void {
      const p = this.progress;
      const resume = !p.completed && p.currentStepId !== null;
      p.completed = false;
      p.active = true;
      p.collapsed = false;
      p.invitationDismissed = true;
      this.show(resume ? (p.currentStepId as GuideStepId) : GUIDE_STEPS[0].id);
    },
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
    back(): void {
      const i = this.stepIndex;
      if (i > 0) this.show(GUIDE_STEPS[i - 1].id);
    },
    /** Closes the coach, keeping the lesson for Resume. */
    pause(): void {
      this.progress.active = false;
      this.progress.collapsed = false;
      this.persist();
    },
    /** Keeps a small resume control while the user works. */
    collapse(): void {
      this.progress.collapsed = true;
      this.persist();
    },
    dismissInvitation(): void {
      this.progress.invitationDismissed = true;
      this.persist();
    },
    /** A control was tried. Recorded only — the lesson stays put. */
    markExplored(id: string): void {
      if (!isGuideStepId(id) || this.progress.explored.includes(id)) return;
      this.progress.explored.push(id);
      this.persist();
    },
    /** Start over: guide state only. Preferences — and the fact the
     * invitation was already answered — are the person's, and stay. */
    restart(): void {
      this.progress = { ...fresh(this.progress.preferences), invitationDismissed: true };
      this.start();
    },
    suspend(): void {
      this.suspendDepth += 1;
    },
    resume(): void {
      this.suspendDepth = Math.max(0, this.suspendDepth - 1);
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
    /** Writes a pending (debounced) save now. Never throws. */
    async flush(): Promise<void> {
      if (persistTimer === null) return;
      clearTimeout(persistTimer);
      persistTimer = null;
      await this.save();
    },
    async save(): Promise<void> {
      if (this.readFailed) return;
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
