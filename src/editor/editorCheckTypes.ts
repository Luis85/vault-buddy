/**
 * The before-you-share checks' wire types (Task 54; Contract reference
 * `CheckFinding`), split out of `editorTypes.ts` for its 500-line cap the
 * way `editorRenderTypes.ts` is, and re-exported from there. An IPC
 * envelope, so camelCase (every field is one word); the members of each
 * union are `core::editor::checks`' serde spellings, pinned on the Rust
 * side by `every_code_and_action_uses_the_contract_spelling`.
 */

export const CHECK_SEVERITIES = ["blocking", "warning", "info"] as const;
export type CheckSeverity = (typeof CHECK_SEVERITIES)[number];

export const CHECK_CODES = [
  "missingMedia",
  "emptyProject",
  "noDestination",
  "gap",
  "allMuted",
  "clipping",
  "excludedCaptions",
  "captionOverlap",
  "captionDensity",
  "pendingTake",
  "transparentClip",
  "textCollision",
  "privacyCover",
  "canvasReview",
] as const;
export type CheckCode = (typeof CHECK_CODES)[number];

export const CHECK_TARGET_KINDS = ["project", "asset", "track", "clip", "effect", "caption", "marker"] as const;
export type CheckTargetKind = (typeof CHECK_TARGET_KINDS)[number];

export const CHECK_ACTIONS = [
  "reconnect",
  "select",
  "openCaptions",
  "openLayout",
  "openAudio",
  "openWebcam",
  "reviewCanvas",
  "setDestination",
] as const;
export type CheckAction = (typeof CHECK_ACTIONS)[number];

/** The object a finding concerns; `id` is `null` for the project itself. */
export interface CheckTarget {
  kind: CheckTargetKind;
  id: string | null;
}

/** Contract reference `CheckFinding` — one row of `editor_get_checks`. */
export interface CheckFinding {
  id: string;
  severity: CheckSeverity;
  code: CheckCode;
  message: string;
  target: CheckTarget;
  action: CheckAction | null;
}
