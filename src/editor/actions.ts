/**
 * The tutorial editor's single action registry (Task 17; F-15, F-49, F-12,
 * F-48). Every surface that lets the user trigger an edit — the preview
 * toolbar, the keyboard shortcut map (`shortcuts.ts`), and the context menu
 * — reads the SAME `resolveActions`/`commandFor` pair, so "is this action
 * available right now, and why not" is answered in exactly one place
 * (SCREENS-AND-INTERACTIONS.md §03: "The same actions are reachable from
 * Edit actions/More and Shift+F10"). Both functions are PURE — they read an
 * `ActionContext` snapshot and never touch a store, a DOM node or
 * `Date.now()` — so a caller (a Vue component today, a future keyboard
 * dispatcher) can call them synchronously from a computed property.
 *
 * **Which commands this task can send.** Sixteen `EditorCommand` kinds are
 * implemented in Rust today (`core::editor::commands::mod.rs`'s own count);
 * the other thirty are rejected with `invalidRequest` and a message of the
 * shape `"<kind> is not available yet"`
 * (`unimplemented_kinds_are_invalid_request_not_panic`, that module's own
 * test). `UNIMPLEMENTED_KINDS` below is the SAME thirty kind strings,
 * gathered in one place so a later task that implements e.g. `addEffect` in
 * Rust only has to delete one entry here — every action that maps to that
 * kind (all seven teaching-tool `add*` actions) flips from disabled to live
 * in the same edit, with no per-action logic to hunt down (this task's own
 * brief: "centralize that list … so later tasks flip entries as they
 * land").
 *
 * **Actions with no wire command.** `copy`/`save`/`render`/`checks`/`help`/
 * `importMedia`/`webcam`/`toggleLibrary`/`toggleInspector`/`focusPreview`/
 * `ratio` never appear in `ACTION_KIND` below — `save` goes through
 * `editorProject.save()` (a distinct IPC call, not `editor_execute`), the
 * `render`/`checks`/`help`/`importMedia`/`webcam` surfaces and the panel/
 * focus toggles are a later task's job or local view state, and `ratio`
 * opens a picker whose eventual choice becomes a `setCanvas` call this task
 * cannot pre-build. `commandFor` returns `null` for all of these — a caller
 * must special-case them (see `PreviewToolbar.vue`'s `onActivate`), never
 * send a `null` command to Rust.
 *
 * **`copy`/`paste` and the clipboard.** `ActionContext` carries the
 * clipboard as TWO fields on purpose: `hasClipboard` is the literal
 * "clipboard presence" flag this task's brief names, cheap for
 * `resolveActions` to gate `paste` on without a caller building real
 * fragment data just to ask "is there anything to paste"; `clipboardFragment`
 * carries the actual `ClipboardFragment` `commandFor` needs to build a real
 * `pasteFragment` command, which `hasClipboard` alone cannot do. The two are
 * kept in sync by whoever assembles `ActionContext` (`hasClipboard =
 * clipboardFragment !== null`). `copy` never appears in `ACTION_KIND` at
 * all — there is no `copyClips` wire command (F-12's copy is a local,
 * uncommitted read via `fragment.ts`'s `buildFragment`, never sent to
 * Rust) — so `commandFor("copy", ctx)` always returns `null`; a caller
 * reads `resolveActions(ctx).copy.enabled` and builds the fragment itself.
 */
import type { Clip, ClipboardFragment, ClipSpan, EditorSnapshot, Project } from "../editorTypes";
import type { EditorCommand } from "./editorCommandTypes";
import { clipOutputDuration, clipOutputEnd } from "./timeMap";

/** Every action id a caller can resolve/execute (this task's own brief —
 * the exact 38-member list, in the order it lists them). */
export type ActionId =
  | "split"
  | "delete"
  | "deleteClose"
  | "undo"
  | "redo"
  | "copy"
  | "cut"
  | "paste"
  | "duplicate"
  | "group"
  | "ungroup"
  | "earlier"
  | "later"
  | "addText"
  | "addArrow"
  | "addHighlight"
  | "addSpotlight"
  | "addZoom"
  | "addStep"
  | "addMask"
  | "addCaption"
  | "addMarker"
  | "addTrackVideo"
  | "addTrackAudio"
  | "fadeIn"
  | "fadeOut"
  | "transition"
  | "detachAudio"
  | "save"
  | "render"
  | "checks"
  | "help"
  | "importMedia"
  | "webcam"
  | "toggleLibrary"
  | "toggleInspector"
  | "focusPreview"
  | "ratio";

/** Every `ActionId`, once, in the union's own declared order — the one
 * place `resolveActions` iterates from, and what `editorActions.test.ts`
 * checks every action id resolves against. */
export const ACTION_IDS: readonly ActionId[] = [
  "split", "delete", "deleteClose", "undo", "redo", "copy", "cut", "paste",
  "duplicate", "group", "ungroup", "earlier", "later",
  "addText", "addArrow", "addHighlight", "addSpotlight", "addZoom", "addStep", "addMask",
  "addCaption", "addMarker", "addTrackVideo", "addTrackAudio",
  "fadeIn", "fadeOut", "transition", "detachAudio",
  "save", "render", "checks", "help", "importMedia", "webcam",
  "toggleLibrary", "toggleInspector", "focusPreview", "ratio",
];

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

export interface ResolvedAction {
  enabled: boolean;
  reason: string | null;
  label: string;
  shortcut: string | null;
}

// ---- human-text reasons (Behavior section's own literal values) -----------

const NO_CLIP = "Select a clip first";
const CLIP_BOUNDARY = "The playhead is at a clip boundary";
const NO_PROJECT = "No project is open.";
const RENDER_REASON = "Rendering a video arrives in a later update.";

function lockedReason(trackName: string): string {
  return `Track ${trackName} is locked`;
}

// ---- static labels/wire kinds ----------------------------------------------

const ACTION_LABELS: Record<ActionId, string> = {
  split: "Split", delete: "Delete", deleteClose: "Delete (close gap)",
  undo: "Undo", redo: "Redo", copy: "Copy", cut: "Cut", paste: "Paste",
  duplicate: "Duplicate", group: "Group", ungroup: "Ungroup",
  earlier: "Move earlier", later: "Move later",
  addText: "Text", addArrow: "Arrow", addHighlight: "Highlight",
  addSpotlight: "Spotlight", addZoom: "Zoom", addStep: "Step", addMask: "Privacy cover",
  addCaption: "Add caption", addMarker: "Add marker",
  addTrackVideo: "Add video track", addTrackAudio: "Add audio track",
  fadeIn: "Fade in", fadeOut: "Fade out", transition: "Add transition",
  detachAudio: "Detach audio",
  save: "Save project", render: "Review", checks: "Checks", help: "Help",
  importMedia: "Import media", webcam: "Webcam",
  toggleLibrary: "Library", toggleInspector: "Inspector", focusPreview: "Focus preview",
  ratio: "Aspect ratio",
};

/** The one `EditorCommand` wire `kind` each action maps to, when it maps to
 * exactly one — see the module doc for the actions deliberately absent. */
const ACTION_KIND: Partial<Record<ActionId, string>> = {
  split: "splitClip", delete: "deleteClips", deleteClose: "deleteClips",
  undo: "undo", redo: "redo", cut: "cutClips", paste: "pasteFragment",
  duplicate: "duplicateClips", group: "groupClips", ungroup: "ungroupClips",
  earlier: "reorderClip", later: "reorderClip",
  addText: "addEffect", addArrow: "addEffect", addHighlight: "addEffect",
  addSpotlight: "addEffect", addZoom: "addEffect", addStep: "addEffect", addMask: "addEffect",
  addCaption: "addCaption", addMarker: "addMarker",
  addTrackVideo: "addTrack", addTrackAudio: "addTrack",
  fadeIn: "setFades", fadeOut: "setFades", transition: "addTransition",
  detachAudio: "detachAudio", ratio: "setCanvas",
};

/** The human-readable shortcut shown beside an action's label/tooltip —
 * `shortcuts.ts`'s `SHORTCUTS` map's DISPLAY twin (both `ctrl+shift+z` and
 * `ctrl+y` resolve to `redo`, but `redo` shows only one). Lives here, not in
 * `shortcuts.ts`, because `resolveActions` needs it synchronously and this
 * file must not import from `shortcuts.ts` (the reverse import already runs
 * the other way, and a back-edge is the cycle `circularDependencies 0`
 * exists to catch); `shortcuts.ts` re-exports it for a caller that wants
 * both from one import. */
export const SHORTCUT_DISPLAY: Partial<Record<ActionId, string>> = {
  split: "S", delete: "Delete", deleteClose: "Shift+Delete",
  undo: "Ctrl+Z", redo: "Ctrl+Shift+Z", copy: "Ctrl+C", cut: "Ctrl+X",
  paste: "Ctrl+V", duplicate: "Ctrl+D", group: "Ctrl+G", ungroup: "Ctrl+Shift+G",
  save: "Ctrl+S", render: "Ctrl+E", help: "F1", focusPreview: "F6",
};

/** The exact thirty wire kinds `apply()` still rejects — see module doc.
 * `undo`/`redo`/`splitClip`/`deleteClips`/`cutClips`/`pasteFragment`/
 * `duplicateClips`/`groupClips`/`ungroupClips`/`reorderClip` are
 * deliberately absent: those ten (of the sixteen implemented kinds) are the
 * ones an action in `ACTION_KIND` maps to. */
export const UNIMPLEMENTED_KINDS: ReadonlySet<string> = new Set([
  "addTrack", "renameTrack", "moveTrack", "setTrackFlags", "deleteTrack",
  "setClipMix", "setMasterGain", "detachAudio", "setFades",
  "addTransition", "setTransitionDuration", "removeTransition",
  "setSpeed", "setLayout", "setAdjustments", "setCanvas",
  "addCard", "updateCard", "insertIntro",
  "addEffect", "updateEffect", "removeEffect",
  "setCaptionSettings", "addCaption", "updateCaption", "splitCaption", "removeCaptions",
  "addMarker", "updateMarker", "removeMarker",
]);

// ---- read-side helpers over the projection --------------------------------

function clipSpanOf(clip: Clip): ClipSpan {
  return { start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed: clip.speed ?? 1 };
}

function clipById(project: Project | null, id: string): Clip | null {
  return project?.clips.find((c) => c.id === id) ?? null;
}

/** The pointer's own clip takes priority over selection (A14: right-click
 * must act on its actual target, not stale selection) — falls back to a
 * single selected clip so toolbar/shortcut invocations (no pointer target)
 * still resolve a target when exactly one clip is selected. */
function primaryTargetClip(ctx: ActionContext): Clip | null {
  const t = ctx.pointerTarget;
  if (t?.kind === "clip" && t.id) return clipById(ctx.project, t.id);
  if (ctx.selectedClipIds.length === 1) return clipById(ctx.project, ctx.selectedClipIds[0]);
  return null;
}

/** The clip ids a multi-clip mutation (delete/cut/duplicate/group/copy)
 * acts on: the pointer's clip alone UNLESS it is already part of the
 * current selection, in which case the whole selection moves together
 * (the common "right-click inside your selection acts on the selection"
 * rule) — otherwise the current selection. */
function targetClipIds(ctx: ActionContext): string[] {
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
function lockedTrackName(project: Project | null, clipIds: string[]): string | null {
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
function targetTrackId(ctx: ActionContext): string | null {
  const t = ctx.pointerTarget;
  if (!t) return null;
  if ((t.kind === "track" || t.kind === "gap") && t.id) return t.id;
  if (t.kind === "clip" && t.id) return clipById(ctx.project, t.id)?.track_id ?? null;
  return null;
}

function targetGroupId(ctx: ActionContext): string | null {
  const clip = primaryTargetClip(ctx) ?? clipById(ctx.project, targetClipIds(ctx)[0] ?? "");
  return clip?.group_id ?? null;
}

// ---- per-action resolvers (only for actions NOT gated as unimplemented) ---

type Verdict = { enabled: boolean; reason: string | null };
const OK: Verdict = { enabled: true, reason: null };

/** Shared by `resolveSplit`/`resolveReorder`: the single target clip, or
 * the `Verdict` that already explains why there isn't a usable one (no
 * target, or its track is locked) — factored out so the two callers don't
 * carry an identical clip-lookup-then-lock-check block each. */
function requireUnlockedTargetClip(ctx: ActionContext): { clip: Clip } | Verdict {
  const clip = primaryTargetClip(ctx);
  if (!clip) return { enabled: false, reason: NO_CLIP };
  const locked = lockedTrackName(ctx.project, [clip.id]);
  if (locked) return { enabled: false, reason: lockedReason(locked) };
  return { clip };
}

function resolveSplit(ctx: ActionContext): Verdict {
  const target = requireUnlockedTargetClip(ctx);
  if (!("clip" in target)) return target;
  const { clip } = target;
  const atMs = ctx.pointerTarget?.timeMs ?? ctx.playheadMs;
  const end = clipOutputEnd(clipSpanOf(clip));
  if (atMs <= clip.start_ms || atMs >= end) return { enabled: false, reason: CLIP_BOUNDARY };
  return OK;
}

/** Shared shape for delete/deleteClose/cut/duplicate: any non-empty clip
 * target, refused when any target clip's track is locked (R14/AGENTS.md:
 * "locked track refuses every clip mutation"). */
function resolveClipMutation(ctx: ActionContext): Verdict {
  const ids = targetClipIds(ctx);
  if (ids.length === 0) return { enabled: false, reason: NO_CLIP };
  const locked = lockedTrackName(ctx.project, ids);
  if (locked) return { enabled: false, reason: lockedReason(locked) };
  return OK;
}

/** Copy is read-only (F-12: a local, uncommitted snapshot) — a locked track
 * may still be copied FROM, only mutated. */
function resolveCopy(ctx: ActionContext): Verdict {
  return targetClipIds(ctx).length === 0 ? { enabled: false, reason: NO_CLIP } : OK;
}

function resolvePaste(ctx: ActionContext): Verdict {
  if (!ctx.hasClipboard) return { enabled: false, reason: "Clipboard is empty" };
  const trackId = targetTrackId(ctx);
  if (!trackId) return { enabled: false, reason: "Select a track first" };
  const track = ctx.project?.tracks.find((t) => t.id === trackId);
  if (track?.locked) return { enabled: false, reason: lockedReason(track.name) };
  return OK;
}

function resolveGroup(ctx: ActionContext): Verdict {
  const ids = targetClipIds(ctx);
  if (ids.length < 2) return { enabled: false, reason: "Select at least two clips" };
  const locked = lockedTrackName(ctx.project, ids);
  if (locked) return { enabled: false, reason: lockedReason(locked) };
  return OK;
}

function resolveUngroup(ctx: ActionContext): Verdict {
  const groupId = targetGroupId(ctx);
  if (!groupId) return { enabled: false, reason: "Select a clip in a group" };
  const memberIds = (ctx.project?.clips ?? []).filter((c) => c.group_id === groupId).map((c) => c.id);
  const locked = lockedTrackName(ctx.project, memberIds);
  if (locked) return { enabled: false, reason: lockedReason(locked) };
  return OK;
}

function resolveReorder(ctx: ActionContext, direction: "earlier" | "later"): Verdict {
  const target = requireUnlockedTargetClip(ctx);
  if (!("clip" in target)) return target;
  const { clip } = target;
  const sameTrack = (ctx.project?.clips ?? [])
    .filter((c) => c.track_id === clip.track_id)
    .sort((a, b) => a.start_ms - b.start_ms);
  const idx = sameTrack.findIndex((c) => c.id === clip.id);
  if (direction === "earlier" && idx <= 0) {
    return { enabled: false, reason: "Already the first clip on this track" };
  }
  if (direction === "later" && idx >= sameTrack.length - 1) {
    return { enabled: false, reason: "Already the last clip on this track" };
  }
  return OK;
}

function resolveUndo(ctx: ActionContext): Verdict {
  return ctx.snapshot?.canUndo ? OK : { enabled: false, reason: "Nothing to undo" };
}
function resolveRedo(ctx: ActionContext): Verdict {
  return ctx.snapshot?.canRedo ? OK : { enabled: false, reason: "Nothing to redo" };
}
function resolveProjectGated(ctx: ActionContext): Verdict {
  return ctx.snapshot ? OK : { enabled: false, reason: NO_PROJECT };
}
function resolveAlways(): Verdict {
  return OK;
}
function resolveRender(): Verdict {
  return { enabled: false, reason: RENDER_REASON };
}

const RESOLVERS: Partial<Record<ActionId, (ctx: ActionContext) => Verdict>> = {
  split: resolveSplit,
  delete: resolveClipMutation,
  deleteClose: resolveClipMutation,
  undo: resolveUndo,
  redo: resolveRedo,
  copy: resolveCopy,
  cut: resolveClipMutation,
  paste: resolvePaste,
  duplicate: resolveClipMutation,
  group: resolveGroup,
  ungroup: resolveUngroup,
  earlier: (ctx) => resolveReorder(ctx, "earlier"),
  later: (ctx) => resolveReorder(ctx, "later"),
  save: resolveProjectGated,
  render: resolveRender,
  checks: resolveProjectGated,
  help: resolveAlways,
  importMedia: resolveProjectGated,
  webcam: resolveProjectGated,
  toggleLibrary: resolveAlways,
  toggleInspector: resolveAlways,
  focusPreview: resolveAlways,
};

function labelFor(actionId: ActionId, ctx: ActionContext): string {
  if (actionId === "undo") return ctx.snapshot?.undoLabel ? `Undo ${ctx.snapshot.undoLabel}` : "Undo";
  if (actionId === "redo") return ctx.snapshot?.redoLabel ? `Redo ${ctx.snapshot.redoLabel}` : "Redo";
  return ACTION_LABELS[actionId];
}

/**
 * Resolve every action id against one context — the toolbar/shortcuts/
 * context-menu's single source for "enabled, and if not, why". An action
 * whose `ACTION_KIND` names a still-unimplemented wire kind is gated FIRST,
 * before any of its own selection/lock checks run — a kind Rust rejects
 * outright is disabled unconditionally, never "disabled for the wrong
 * reason" because nothing was selected.
 */
export function resolveActions(ctx: ActionContext): Record<ActionId, ResolvedAction> {
  const out = {} as Record<ActionId, ResolvedAction>;
  for (const actionId of ACTION_IDS) {
    const kind = ACTION_KIND[actionId];
    const gateReason = kind && UNIMPLEMENTED_KINDS.has(kind) ? `${kind} is not available yet` : null;
    const verdict = gateReason
      ? { enabled: false, reason: gateReason }
      : (RESOLVERS[actionId]?.(ctx) ?? { enabled: false, reason: null });
    out[actionId] = {
      enabled: verdict.enabled,
      reason: verdict.reason,
      label: labelFor(actionId, ctx),
      shortcut: SHORTCUT_DISPLAY[actionId] ?? null,
    };
  }
  return out;
}

// ---- command builders (one per action that has a wire command) ------------
// Table-driven, the `RESOLVERS` precedent above: a ~30-branch switch here
// once pushed `commandFor` over the fallow complexity ceiling (25) by
// itself, so it stays a plain lookup instead.
//
// None of these re-check the target/lock/clipboard preconditions their own
// `RESOLVERS` entry already checked: `commandFor` calls a builder only
// AFTER confirming `resolveActions(ctx)[actionId].enabled`, and every
// resolver above is the SAME pure lookup a builder repeats over the same
// `ctx` -- re-deriving it can only re-confirm what already held. A second
// silent-`null` guard for a precondition that can no longer fail is not
// defensive, it is an untestable branch masquerading as one; trusting the
// invariant is what Rust's own `find_clip(...).expect(...)` calls do
// (`core::editor::commands::clips.rs`) for exactly this shape of guarantee.

type Builder = (ctx: ActionContext, actionId: ActionId) => EditorCommand;

function buildSplit(ctx: ActionContext): EditorCommand {
  const clip = primaryTargetClip(ctx) as Clip;
  return { kind: "splitClip", clipId: clip.id, atMs: ctx.pointerTarget?.timeMs ?? ctx.playheadMs };
}

function buildDelete(ctx: ActionContext, actionId: ActionId): EditorCommand {
  return { kind: "deleteClips", clipIds: targetClipIds(ctx), closeGap: actionId === "deleteClose" };
}

function buildCut(ctx: ActionContext): EditorCommand {
  // Ripple by default (closeGap: true) -- a cut removes its content from
  // the timeline the way delete-and-ripple does, distinct from the
  // leave-a-gap default of a plain Delete keypress.
  return { kind: "cutClips", clipIds: targetClipIds(ctx), closeGap: true };
}

function buildDuplicate(ctx: ActionContext): EditorCommand {
  const ids = targetClipIds(ctx);
  const project = ctx.project as Project;
  // Offset by the LONGEST target clip's own output duration, so every
  // duplicate lands clear of its own original regardless of which target
  // clip is longest -- Rust's own `duplicateClips` adds this one offsetMs
  // to every selected clip's start_ms uniformly and then refuses on any
  // resulting overlap (`check_no_overlap`).
  const offsetMs = ids.reduce((max, id) => {
    const clip = project.clips.find((c) => c.id === id);
    return clip ? Math.max(max, clipOutputDuration(clip.in_ms, clip.out_ms, clip.speed ?? 1)) : max;
  }, 0);
  return { kind: "duplicateClips", clipIds: ids, offsetMs };
}

function buildGroup(ctx: ActionContext): EditorCommand {
  return { kind: "groupClips", clipIds: targetClipIds(ctx) };
}

function buildUngroup(ctx: ActionContext): EditorCommand {
  return { kind: "ungroupClips", groupId: targetGroupId(ctx) as string };
}

function buildReorder(ctx: ActionContext, actionId: ActionId): EditorCommand {
  const clip = primaryTargetClip(ctx) as Clip;
  return { kind: "reorderClip", clipId: clip.id, direction: actionId as "earlier" | "later" };
}

function buildUndo(): EditorCommand {
  return { kind: "undo" };
}
function buildRedo(): EditorCommand {
  return { kind: "redo" };
}

function buildPaste(ctx: ActionContext): EditorCommand {
  return {
    kind: "pasteFragment",
    fragment: ctx.clipboardFragment as ClipboardFragment,
    trackId: targetTrackId(ctx) as string,
    atMs: ctx.pointerTarget?.timeMs ?? ctx.playheadMs,
  };
}

const BUILDERS: Partial<Record<ActionId, Builder>> = {
  split: buildSplit,
  delete: buildDelete,
  deleteClose: buildDelete,
  cut: buildCut,
  duplicate: buildDuplicate,
  group: buildGroup,
  ungroup: buildUngroup,
  earlier: buildReorder,
  later: buildReorder,
  undo: buildUndo,
  redo: buildRedo,
  paste: buildPaste,
};

/**
 * Build the `EditorCommand` for one action, or `null` when it has no wire
 * command (falls through `BUILDERS` — see the module doc), is currently
 * DISABLED per `resolveActions` (a locked track, a clip boundary, too few
 * clips, an empty clipboard — every verdict this shares with the toolbar/
 * context menu), or the context names no valid target. A caller must treat
 * `null` as "nothing to send", never retry with a guessed target. `atMs`
 * always prefers the POINTER's own time over the playhead (A14) — the one
 * rule this task's mutation check exists to pin.
 */
export function commandFor(actionId: ActionId, ctx: ActionContext): EditorCommand | null {
  if (!resolveActions(ctx)[actionId].enabled) return null;
  return BUILDERS[actionId]?.(ctx, actionId) ?? null;
}
