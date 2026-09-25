/**
 * The guided-onboarding progress wire type (Task 55; Contract reference
 * `GuideProgress`), split out of `editorTypes.ts` for its 500-line cap the
 * way `editorTakeTypes.ts` is, and re-exported from there. camelCase, like
 * `core::editor::guide::GuideProgress`, which pins the literal.
 */

export const GUIDE_MOTIONS = ["system", "reduced", "full"] as const;
type GuideMotion = (typeof GUIDE_MOTIONS)[number];

/** Presentation preferences — kept by a restart. */
export interface GuidePreferences {
  dimming: boolean;
  motion: GuideMotion;
}

/** The app-wide guide state (`editor-prefs\guide-progress.json`): lesson
 * ids and flags only, never a path, media or project detail. */
export interface GuideProgress {
  contentRevision: number;
  currentStepId: string | null;
  reviewed: string[];
  explored: string[];
  invitationDismissed: boolean;
  active: boolean;
  collapsed: boolean;
  completed: boolean;
  preferences: GuidePreferences;
}
