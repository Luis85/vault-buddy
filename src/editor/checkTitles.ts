/**
 * Each before-you-share finding's heading (visual-parity Task 21; concept
 * spec §9.4 `.issue-row h3`). Rust sends one sentence per finding
 * (`CheckFinding.message`); the concept's row has a short title above it,
 * so the title is named here by the finding's CODE — never derived from
 * the message, which can quote a clip or file name.
 */
import type { CheckCode, CheckSeverity } from "./editorCheckTypes";

export const CHECK_TITLES: Record<CheckCode, string> = {
  missingMedia: "An original is missing",
  emptyProject: "The timeline is empty",
  noDestination: "No vault chosen yet",
  gap: "A gap in the timeline",
  allMuted: "Nothing can be heard",
  clipping: "The sound may distort",
  excludedCaptions: "Captions are turned off",
  captionOverlap: "Captions overlap",
  captionDensity: "A caption reads fast",
  pendingTake: "A webcam take is not saved",
  transparentClip: "A clip will not be seen",
  textCollision: "Text cues overlap",
  privacyCover: "Check the privacy cover",
  canvasReview: "Review the new canvas",
};

/** The row's kind chip (§9.4 `.issue-kind`): its word and its colours.
 * FIX takes `danger-fg`, not the concept's `--danger`: red-400 on its own
 * 15 % tint measured 4.50:1 rounded up in the dark theme, below 4.5. */
export const CHECK_KINDS: Record<CheckSeverity, { tag: string; chip: string }> = {
  blocking: { tag: "FIX", chip: "bg-danger/15 text-danger-fg" },
  warning: { tag: "REVIEW", chip: "bg-gold-bg text-gold" },
  info: { tag: "NOTE", chip: "bg-accent-bg text-accent-ink" },
};
