/**
 * Transition read-side rules (Task 30; F-19) — PURE, shared by the
 * `transition` action (`actions.ts`, the context menu's "Add transition")
 * and the Fades inspector's transition block (`ClipTransitions.vue`,
 * `TransitionRow.vue`), the `mixRules.ts` precedent, so neither grows a
 * second copy.
 *
 * Every refusal here MIRRORS one `core::editor::commands::transitions::
 * add_transition` makes (read from the Rust source, in the SAME order —
 * the sides before adjacency), so an enabled "Add transition" is one Rust
 * will accept for the reasons the graph alone can decide. It is a preview,
 * never the authority: a locked track is `actions.ts`'s shared check, and
 * the group refusal (a shifted clip grouped across tracks) is left to Rust,
 * whose message reaches the user through the store's `lastError`.
 */
import type { Clip, Project, Transition, TransitionKind } from "../editorTypes";
import { clipSpanOf } from "./actionTargets";
import type { EditorCommand } from "./editorCommandTypes";
import { clipOutputEnd } from "./timeMap";

/** The duration "Add transition" asks for — the reference editor's own
 * "Blend · 1s" — clamped to the pair's bound. */
const DEFAULT_TRANSITION_MS = 1_000;

export const NOT_ADJACENT = "A transition needs the next clip to start exactly where this one ends, on the same track";

function durationOf(clip: Clip): number {
  return clipOutputEnd(clipSpanOf(clip)) - clip.start_ms;
}

/** The transitions a clip carries on each side (at most one per side). */
export function transitionsOf(
  project: Project,
  clipId: string,
): { incoming: Transition | null; outgoing: Transition | null } {
  return {
    incoming: project.transitions.find((t) => t.to === clipId) ?? null,
    outgoing: project.transitions.find((t) => t.from === clipId) ?? null,
  };
}

/** Half the SHORTER clip's output duration, floored — Rust's
 * `transitions::duration_bound` (`fades::fade_limit` of each clip). */
export function transitionBoundMs(from: Clip, to: Clip): number {
  return Math.min(Math.floor(durationOf(from) / 2), Math.floor(durationOf(to) / 2));
}

/** The clip on `clip`'s track that starts exactly where `clip` ends. */
function nextAdjacentClip(project: Project, clip: Clip): Clip | null {
  const end = clipOutputEnd(clipSpanOf(clip));
  return project.clips.find((c) => c.id !== clip.id && c.track_id === clip.track_id && c.start_ms === end) ?? null;
}

/** A video clip dissolves, an audio clip crossfades at equal power
 * (`validate_media::transition_kind_for`). */
function transitionKindFor(project: Project, clip: Clip): TransitionKind {
  const kind = project.assets.find((a) => a.id === clip.asset_id)?.kind;
  return kind === "audio" ? "equal-power" : "dissolve";
}

/** Why `clip` cannot take a transition into the next clip, or `null`. */
export function transitionRefusal(project: Project, clip: Clip): string | null {
  const { outgoing } = transitionsOf(project, clip.id);
  if (outgoing) return "This clip already has a transition into the next clip";
  const next = nextAdjacentClip(project, clip);
  if (!next) return NOT_ADJACENT;
  if (transitionsOf(project, next.id).incoming) return "The next clip already has a transition in";
  if (transitionKindFor(project, next) !== transitionKindFor(project, clip)) {
    return "The next clip carries different media";
  }
  if (transitionBoundMs(clip, next) < 1) return "These clips are too short for a transition";
  return null;
}

/** The `addTransition` "Add transition" sends for `clip` — only called once
 * `transitionRefusal` has returned `null`, so the next clip exists. */
export function addTransitionCommand(project: Project, clip: Clip): EditorCommand {
  const next = nextAdjacentClip(project, clip) as Clip;
  return {
    kind: "addTransition",
    fromClipId: clip.id,
    toClipId: next.id,
    durationMs: Math.min(DEFAULT_TRANSITION_MS, transitionBoundMs(clip, next)),
    transitionKind: transitionKindFor(project, clip),
  };
}
