/**
 * Who an editor action acts on, read off one `ActionContext` snapshot
 * (Task 30, split out of `actions.ts` at that file's own 500-line cap, the
 * `actionMeta.ts` precedent): the `ActionContext`/`PointerTarget` shapes,
 * the pointer-over-selection target rules (A14) and the locked-track
 * lookup every resolver shares, plus the `Verdict` those resolvers return.
 * Pure like the registry itself -- nothing here reads a store, the DOM or
 * a clock.
 *
 * `actions.ts` re-exports the public names (`ActionContext`,
 * `PointerTarget`, `targetClipIds`), so no caller outside these files has
 * to know the split exists; the rest are exported only for `actions.ts`
 * and the per-domain rule modules (`transitionRules.ts`, `mixRules.ts`).
 */
import type { Clip, ClipboardFragment, ClipSpan, EditorSnapshot, Project } from "../editorTypes";
import { lockedReason, NO_CLIP } from "./actionMeta";

/** The right-clicked/keyboard-focused thing an edit acts on
 * (SCREENS-AND-INTERACTIONS.md §03: "Context targets include clip(s),
 * effect, video layer, track, asset and gap"). `timeMs` is the pointer's
 * OWN time — `commandFor` prefers it over the playhead for every
 * time-sensitive command (A14). */
export interface PointerTarget {
  kind: "clip" | "effect" | "track" | "asset" | "gap" | "layer";
  id: string | null;
  timeMs: number | null;
}

/** Everything `resolveActions`/`commandFor` read — a snapshot, never a live
 * store reference, so both functions stay pure. See the module doc for why
 * the clipboard is two fields. */
export interface ActionContext {
  project: Project | null;
  snapshot: EditorSnapshot | null;
  playheadMs: number;
  selectedClipIds: string[];
  pointerTarget: PointerTarget | null;
  hasClipboard: boolean;
  clipboardFragment: ClipboardFragment | null;
}

// ---- read-side helpers over the projection --------------------------------

export function clipSpanOf(clip: Clip): ClipSpan {
  return { start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed: clip.speed ?? 1 };
}

function clipById(project: Project | null, id: string): Clip | null {
  return project?.clips.find((c) => c.id === id) ?? null;
}

/** The pointer's own clip takes priority over selection (A14: right-click
 * must act on its actual target, not stale selection) — falls back to a
 * single selected clip so toolbar/shortcut invocations (no pointer target)
 * still resolve a target when exactly one clip is selected. */
export function primaryTargetClip(ctx: ActionContext): Clip | null {
  const t = ctx.pointerTarget;
  if (t?.kind === "clip" && t.id) return clipById(ctx.project, t.id);
  if (ctx.selectedClipIds.length === 1) return clipById(ctx.project, ctx.selectedClipIds[0]);
  return null;
}

/** The clip ids a multi-clip mutation (delete/cut/duplicate/group/copy)
 * acts on: the pointer's clip alone UNLESS it is already part of the
 * current selection, in which case the whole selection moves together
 * (the common "right-click inside your selection acts on the selection"
 * rule) — otherwise the current selection.
 *
 * Exported (Task 21): `../clipboard.ts`'s `activateEditorAction` needs the
 * exact same "which clips does Copy/Cut act on" answer `resolveClipMutation`/
 * `buildCut` already use here, so a keyboard- or menu-triggered Copy can
 * never select a different set of clips than the Cut/Delete that follows
 * the identical gesture. */
export function targetClipIds(ctx: ActionContext): string[] {
  const t = ctx.pointerTarget;
  if (t?.kind === "clip" && t.id) {
    return ctx.selectedClipIds.includes(t.id) ? ctx.selectedClipIds : [t.id];
  }
  return ctx.selectedClipIds;
}

/** The first locked track among the given clips' own tracks, by name —
 * `null` when none of them sit on a locked track. Mirrors
 * `core::editor::commands::clips::ensure_unlocked`'s own message shape
 * ("Track {name} is locked", `clips.rs`). */
export function lockedTrackName(project: Project | null, clipIds: string[]): string | null {
  if (!project) return null;
  for (const id of clipIds) {
    const clip = project.clips.find((c) => c.id === id);
    const track = clip ? project.tracks.find((t) => t.id === clip.track_id) : undefined;
    if (track?.locked) return track.name;
  }
  return null;
}

/** The track a paste (or, in principle, any track-targeted insert) lands
 * on: the pointer's own track when it names one directly ("track"/"gap"),
 * or the track of a pointer/selected clip otherwise. */
export function targetTrackId(ctx: ActionContext): string | null {
  const t = ctx.pointerTarget;
  if (!t) return null;
  if ((t.kind === "track" || t.kind === "gap") && t.id) return t.id;
  if (t.kind === "clip" && t.id) return clipById(ctx.project, t.id)?.track_id ?? null;
  return null;
}

export function targetGroupId(ctx: ActionContext): string | null {
  const clip = primaryTargetClip(ctx) ?? clipById(ctx.project, targetClipIds(ctx)[0] ?? "");
  return clip?.group_id ?? null;
}

// ---- the verdict shape every resolver returns --------------------------

export type Verdict = { enabled: boolean; reason: string | null };
export const OK: Verdict = { enabled: true, reason: null };

/** Shared by `resolveSplit`/`resolveReorder`: the single target clip, or
 * the `Verdict` that already explains why there isn't a usable one (no
 * target, or its track is locked) — factored out so the two callers don't
 * carry an identical clip-lookup-then-lock-check block each. */
export function requireUnlockedTargetClip(ctx: ActionContext): { clip: Clip } | Verdict {
  const clip = primaryTargetClip(ctx);
  if (!clip) return { enabled: false, reason: NO_CLIP };
  const locked = lockedTrackName(ctx.project, [clip.id]);
  if (locked) return { enabled: false, reason: lockedReason(locked) };
  return { clip };
}
