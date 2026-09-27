/**
 * The timeline footer's live edit hint (visual-parity Task 20; concept
 * spec §7, `#editHint`): one line under the lanes saying what the drag in
 * hand will do, and "Callouts follow their clip." when nothing is being
 * dragged. There is no second drag model here: the dragged `ClipItem` or
 * `CueChip` turns its OWN drag composable's live state (`useTimelineDrag`'s
 * previews, `useCueDrag`'s grip) into a line with `clipDragHint` /
 * `cueDragHint` and reports it the way it reports the snap guide
 * (`useTimelineReport`); `TimelineView` provides the ref and hands it to
 * `TimelineFooter`.
 *
 * Every line is true of what the release sends. A clip move counts what
 * `moveClips` really moves — the selection grown to whole groups
 * (`withGroups`, Rust's own expansion) — so one clip of a group of three
 * reads "Moving 3 clips together"; and with more than one clip moving
 * Rust takes no `trackId`, so "tracks stay fixed" is exactly its rule. A
 * cue move and trim are clamped inside the cue's own clip (`useCueDrag`),
 * which is what "stays on its clip" says. The concept's own wording is
 * kept wherever it is true here.
 */
import type { InjectionKey, Ref } from "vue";

import { formatMenuTime } from "../components/editor/menus/menuModel";
import type { CueGrip } from "../composables/useCueDrag";
import type { FadePreview, MovePreview, TrimPreview } from "../composables/useTimelineDrag";
import type { Clip, EffectKind, Project } from "../editorTypes";
import { EFFECT_NAMES } from "./effectFields";
import { useTimelineReport } from "./snapGuide";
import { withGroups } from "./speedRipple";

export const DEFAULT_EDIT_HINT = "Callouts follow their clip.";

export const DRAG_HINT_KEY: InjectionKey<Ref<string | null>> = Symbol("timeline edit hint");

/** One clip's live drag, as `useTimelineDrag` holds it. */
export interface ClipDragState {
  project: Project | null;
  clip: Clip;
  /** The ids the release names (`ClipItem`'s `moveTargetClipIds`). */
  moveIds: string[];
  move: MovePreview | null;
  trim: TrimPreview | null;
  fade: FadePreview | null;
}

function moveHint(s: ClipDragState, deltaMs: number): string {
  const moving = s.project ? withGroups(s.project, s.moveIds).size : s.moveIds.length;
  if (moving > 1) return `Moving ${moving} clips together · tracks stay fixed · Esc cancels`;
  return `Starts at ${formatMenuTime(Math.max(0, s.clip.start_ms + deltaMs))} · release to place`;
}

/** The line for a clip drag in progress, or `null` when there is none. */
export function clipDragHint(s: ClipDragState): string | null {
  if (s.move) return moveHint(s, s.move.deltaMs);
  if (s.trim) return "Release to apply · Escape to cancel";
  if (s.fade) return `Fade ${s.fade.edge}: ${(s.fade.ms / 1000).toFixed(2)}s`;
  return null;
}

/** The line for a teaching cue's drag in progress, or `null`. */
export function cueDragHint(kind: EffectKind, grip: CueGrip | null): string | null {
  if (!grip) return null;
  const verb = grip === "move" ? "Moving" : "Resizing";
  return `${verb} ${EFFECT_NAMES[kind].toLowerCase()} · stays on its clip · Esc cancels`;
}

/** Reports a dragged thing's line to the timeline footer. Call from setup. */
export function useDragHintReport(hint: Readonly<Ref<string | null>>): void {
  useTimelineReport(DRAG_HINT_KEY, hint);
}
