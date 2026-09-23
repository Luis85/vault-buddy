/**
 * The seven teaching-tool actions (Task 35; F-27–F-33) — `addText`,
 * `addArrow`, `addHighlight`, `addSpotlight`, `addZoom`, `addStep`,
 * `addMask` — as the resolver/builder pair `actions.ts` registers, replacing
 * the placeholder "no surface yet" resolvers Task 34 left so those buttons
 * were not enabled-but-broken. Split out because `actions.ts` sits near its
 * own 500-line cap.
 *
 * **Which clip a cue lands on** (SCREENS-AND-INTERACTIONS.md: "Teaching
 * tools add editable clip-linked cues"): the single SELECTED clip, if it is
 * on a visible video track and the playhead is on it; otherwise the TOPMOST
 * active clip on a visible video track whose box covers the stage centre —
 * `tracks[0]` paints on top (`previewLayers.ts`), so the lowest track index
 * wins. The reference editor picks the topmost active clip without the
 * centre test; the brief adds it so a small picture-in-picture above a
 * full-frame recording does not capture a cue meant for the recording.
 *
 * **Times are SOURCE time** (Task 34's `cues.rs`): the playhead is output
 * time, so the cue starts at `timeMap.sourceAt(clip, playhead)`. The
 * default length is 3000 ms of OUTPUT — `3000 · speed` ms of source — or up
 * to the clip's own source end if that is sooner (controller ruling on
 * Task 34: the default belongs to the UI, not the command; the reference's
 * own `start + 4000 * clipSpeed(c)`, capped at `out_ms`, is the same shape).
 * Every other property is left to Rust's per-kind defaults (`cues.rs`'s
 * `default_shell`), except a step's number, which counts on from the
 * project's existing steps (the reference's own rule), capped at the
 * schema's 99.
 */
import type { Clip, Effect, EffectKind, Project, Selected } from "../editorTypes";
import type { ActionId } from "./actionMeta";
import { lockedReason, NO_PROJECT } from "./actionMeta";
import type { ActionContext, Verdict } from "./actionTargets";
import { clipSpanOf, lockedTrackName, OK } from "./actionTargets";
import type { AddEffectCommand } from "./editorCommandTypes";
import { clipIsActive, sourceAt } from "./timeMap";

/** The default cue length, in OUTPUT ms. */
const CUE_DEFAULT_MS = 3_000;
/** `core::editor::limits::MAX_EFFECTS`. */
const MAX_EFFECTS = 1_200;
/** The schema's own bound on a step's `number`. */
const MAX_STEP_NUMBER = 99;

const NO_CUE_TARGET = "No visible clip at the playhead to add a cue to";
const CUES_FULL = `This project already has the maximum of ${MAX_EFFECTS} teaching cues`;

const CUE_ACTION_KIND: Partial<Record<ActionId, EffectKind>> = {
  addText: "text",
  addArrow: "arrow",
  addHighlight: "highlight",
  addSpotlight: "spotlight",
  addZoom: "zoom",
  addStep: "step",
  addMask: "mask",
};

function onVisibleVideoTrack(project: Project, clip: Clip): boolean {
  const track = project.tracks.find((t) => t.id === clip.track_id);
  return track !== undefined && track.kind === "video" && track.visible;
}

function coversCentre(clip: Clip): boolean {
  return clip.x <= 0.5 && clip.x + clip.w >= 0.5 && clip.y <= 0.5 && clip.y + clip.h >= 0.5;
}

/** The clip a teaching tool would annotate right now, or `null`. */
function cueTargetClip(ctx: ActionContext): Clip | null {
  const project = ctx.project;
  if (!project) return null;
  const t = ctx.playheadMs;
  const usable = (c: Clip) => onVisibleVideoTrack(project, c) && clipIsActive(clipSpanOf(c), t);
  if (ctx.selectedClipIds.length === 1) {
    const selected = project.clips.find((c) => c.id === ctx.selectedClipIds[0]);
    if (selected && usable(selected)) return selected;
  }
  const trackIndex = (c: Clip) => project.tracks.findIndex((tr) => tr.id === c.track_id);
  const candidates = project.clips.filter((c) => usable(c) && coversCentre(c));
  candidates.sort((a, b) => trackIndex(a) - trackIndex(b));
  return candidates[0] ?? null;
}

/** Enabled when there is a clip to annotate, on an unlocked track, and
 * room for one more cue. */
export function resolveCue(ctx: ActionContext): Verdict {
  if (!ctx.project) return { enabled: false, reason: NO_PROJECT };
  const clip = cueTargetClip(ctx);
  if (!clip) return { enabled: false, reason: NO_CUE_TARGET };
  const locked = lockedTrackName(ctx.project, [clip.id]);
  if (locked) return { enabled: false, reason: lockedReason(locked) };
  if (ctx.project.effects.length >= MAX_EFFECTS) return { enabled: false, reason: CUES_FULL };
  return OK;
}

/** The `addEffect` for one teaching tool. Called only after `resolveCue`
 * said yes (`commandFor`'s contract), so the target is known to exist. */
export function buildCue(ctx: ActionContext, actionId: ActionId): AddEffectCommand {
  const project = ctx.project as Project;
  const clip = cueTargetClip(ctx) as Clip;
  const span = clipSpanOf(clip);
  const startMs = sourceAt(span, ctx.playheadMs) as number;
  const endMs = Math.min(clip.out_ms, startMs + Math.round(CUE_DEFAULT_MS * span.speed));
  const effectKind = CUE_ACTION_KIND[actionId] as EffectKind;
  const base = { kind: "addEffect" as const, clipId: clip.id, startMs, endMs };
  if (effectKind === "step") {
    const steps = project.effects.filter((e) => e.kind === "step").length;
    return { ...base, effectKind, props: { number: Math.min(MAX_STEP_NUMBER, steps + 1) } };
  }
  return { ...base, effectKind, props: {} } as AddEffectCommand;
}

/** The id of the one effect `after` has that `before` did not — the cue an
 * `addEffect` just created, so the toolbar can select it. */
export function addedEffectId(before: Project | null, after: Project | null): string | null {
  const known = new Set((before?.effects ?? []).map((e) => e.id));
  return after?.effects.find((e) => !known.has(e.id))?.id ?? null;
}

/**
 * The cue the workspace has selected, or `null`. A cue counts as selected
 * only while its own clip is the whole clip selection: picking another clip
 * on the timeline moves the selection on without anyone having to clear
 * the cue by hand, and a removed cue simply stops resolving.
 */
export function selectedEffectOf(
  project: Project | null,
  selected: Selected | null,
  selectionClipIds: string[],
): Effect | null {
  if (!project || selected?.type !== "effect") return null;
  const effect = project.effects.find((e) => e.id === selected.id);
  if (!effect || selectionClipIds.length !== 1 || selectionClipIds[0] !== effect.clip_id) return null;
  return effect;
}
