/**
 * The decoder for `editor_get_guide_progress`' `GuideProgress` (Task 55),
 * its own file like `decodeChecks.ts`. Structural only: which step ids
 * are lessons is `content.ts`' `resolveStepId`, applied by the
 * `editorOnboarding` store — Rust has already resolved them on read, and
 * the decoder does not second-guess that with a copy of the list.
 */
import type { GuideProgress } from "../editorTypes";
import { asArray, asBoolean, asEnum, asInteger, asNullableString, asObject, asString, fail } from "./decodePrimitives";
import { GUIDE_MOTIONS } from "./editorGuideTypes";

function asStrings(value: unknown, field: string): string[] {
  return asArray(value, field).map((v, i) => asString(v, `${field}[${i}]`));
}

export function decodeGuideProgress(value: unknown): GuideProgress {
  const v = asObject(value, "guideProgress");
  const contentRevision = asInteger(v.contentRevision, "guideProgress.contentRevision");
  if (contentRevision < 0) fail("guideProgress.contentRevision must not be negative");
  const prefs = asObject(v.preferences, "guideProgress.preferences");
  return {
    contentRevision,
    currentStepId: asNullableString(v.currentStepId, "guideProgress.currentStepId"),
    reviewed: asStrings(v.reviewed, "guideProgress.reviewed"),
    explored: asStrings(v.explored, "guideProgress.explored"),
    invitationDismissed: asBoolean(v.invitationDismissed, "guideProgress.invitationDismissed"),
    active: asBoolean(v.active, "guideProgress.active"),
    collapsed: asBoolean(v.collapsed, "guideProgress.collapsed"),
    completed: asBoolean(v.completed, "guideProgress.completed"),
    preferences: {
      dimming: asBoolean(prefs.dimming, "guideProgress.preferences.dimming"),
      motion: asEnum(prefs.motion, "guideProgress.preferences.motion", GUIDE_MOTIONS),
    },
  };
}
