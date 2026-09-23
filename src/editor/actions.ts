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
 * Static reference data (every `ActionId`, labels, reasons, the wire-kind
 * lookup, `UNIMPLEMENTED_KINDS`) lives in `./actionMeta` (fix round 1, split
 * at this file's own 500-line cap) and is re-exported below, so nothing
 * outside these two files needs to know the split exists.
 *
 * **Which commands this task can send.** Twenty-one `EditorCommand` kinds
 * are implemented in Rust today (`core::editor::commands::mod.rs`'s own
 * count, as of Task 23's five track commands); the other twenty-five are
 * rejected with `invalidRequest` and a message of the
 * shape `"<kind> is not available yet"`
 * (`unimplemented_kinds_are_invalid_request_not_panic`, that module's own
 * test — its own module doc names `UNIMPLEMENTED_KINDS` back as the
 * frontend twin a task implementing a kind must also update). That message
 * shape is Rust's OWN wire-level text, read by nothing user-facing here —
 * `actionMeta.ts`'s `unavailableReason` builds the actual UI copy from the
 * action's own label instead (fix round 1, finding 1).
 *
 * **Actions with no wire command.** `copy`/`save`/`render`/`checks`/`help`/
 * `importMedia`/`webcam`/`toggleLibrary`/`toggleInspector`/`focusPreview`/
 * `ratio` never appear in `ACTION_KIND` — `save` goes through
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
 *
 * **The keyboard dispatcher and the Shift+F10 invoker are NOT this task's
 * job** (controller ruling): `shortcuts.ts` exports the pure map/predicates
 * this task's brief asks for, but wiring a `window` keydown listener and
 * wiring a focused clip's Shift+F10/Menu-key handler are carried to Tasks
 * 20/21, once there is a real timeline/canvas to bind either to.
 */
import type { Clip, ClipboardFragment, ClipSpan, EditorSnapshot, Project } from "../editorTypes";
import type { ActionId } from "./actionMeta";
import {
  ACTION_IDS,
  ACTION_KIND,
  ACTION_LABELS,
  CLIP_BOUNDARY,
  lockedReason,
  NO_CLIP,
  NO_PROJECT,
  RENDER_REASON,
  SHORTCUT_DISPLAY,
  unavailableReason,
  UNIMPLEMENTED_KINDS,
} from "./actionMeta";
import type { EditorCommand } from "./editorCommandTypes";
import { clipOutputEnd } from "./timeMap";

export type { ActionId } from "./actionMeta";
export { ACTION_IDS, SHORTCUT_DISPLAY, UNIMPLEMENTED_KINDS } from "./actionMeta";

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

/**
 * `addTrackVideo`/`addTrackAudio` (Task 26): `addTrack` itself left
 * `UNIMPLEMENTED_KINDS` this task (a real caller sends it now --
 * `TimelineView.vue`'s below-the-last-lane asset drop, `editorProject.
 * execute` directly, the `TrackHeader.vue` precedent, never through this
 * registry), but neither `ActionId` that maps to it has a keyboard/menu/
 * toolbar surface of its own yet. Without an explicit resolver these two
 * would fall through `RESOLVERS[actionId]?.(ctx) ?? {enabled:false,
 * reason:null}` in `resolveActions` below -- disabled with NO reason,
 * which `editorActions.test.ts`'s "every disabled action carries a
 * reason" invariant exists precisely to catch. Reusing `unavailableReason`
 * keeps the user-facing text identical to what the old `UNIMPLEMENTED_KINDS`
 * gate showed, even though the underlying mechanism changed.
 */
function resolveNoTrackSurfaceYet(actionId: ActionId): Verdict {
  return { enabled: false, reason: unavailableReason(actionId) };
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
  addTrackVideo: () => resolveNoTrackSurfaceYet("addTrackVideo"),
  addTrackAudio: () => resolveNoTrackSurfaceYet("addTrackAudio"),
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
    // `kind` (Rust's own wire vocabulary, e.g. "addEffect") is diagnostic
    // only — `unavailableReason` builds the user-facing sentence from the
    // action's own label instead (fix round 1, finding 1: a button
    // labelled "Text" must never show the raw string "addEffect").
    const gateReason = kind && UNIMPLEMENTED_KINDS.has(kind) ? unavailableReason(actionId) : null;
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

/**
 * `duplicate`'s `offsetMs`. **Fix round 1, finding 2**: the first cut used
 * the LONGEST target clip's own output duration, but Rust's
 * `duplicateClips` adds ONE uniform `offsetMs` to every selected clip's
 * `start_ms` and then refuses the whole command if any duplicate overlaps
 * ANY existing clip on its track (`check_no_overlap`) — including the
 * other UNTOUCHED originals in the selection. A per-clip duration can be
 * smaller than the gap one target clip needs to clear another target
 * clip's own original span (this task's fixture: c1 `0..2000`, c2
 * `3000..3500` — offsetting by c1's own 2000ms duration lands c1's
 * duplicate at `2000..4000`, which overlaps c2's UNTOUCHED original at
 * `3000..3500`; Rust would reject it).
 *
 * The correct offset is the SELECTION's own span — `max(every target
 * clip's output end) - min(every target clip's start)` — so the entire
 * duplicated block lands immediately after the last originally-occupied
 * instant across the WHOLE selection, never overlapping any original
 * clip regardless of gaps between the selected clips.
 */
function buildDuplicate(ctx: ActionContext): EditorCommand {
  const ids = targetClipIds(ctx);
  const project = ctx.project as Project;
  const targetClips = ids
    .map((id) => project.clips.find((c) => c.id === id))
    .filter((c): c is Clip => c !== undefined);
  // Task 20's own carried finding: `resolveClipMutation` only checks
  // `targetClipIds(ctx).length > 0` -- it never confirms those ids still
  // RESOLVE against `ctx.project.clips`. A stale id (a delete landing from
  // another surface between resolving actions and this builder running)
  // makes `targetClips` empty while `ids` is not, and `Math.min(...[])` /
  // `Math.max(...[])` are `Infinity`/`-Infinity` -- an `offsetMs` of
  // `-Infinity` sent straight to Rust. `0` for an empty resolved set is
  // honest: nothing here can compute a real offset for zero real clips, and
  // `0` at least fails Rust's own overlap/range validation cleanly instead
  // of shipping a non-finite number over IPC.
  const offsetMs =
    targetClips.length === 0
      ? 0
      : Math.max(...targetClips.map((c) => clipOutputEnd(clipSpanOf(c)))) -
        Math.min(...targetClips.map((c) => c.start_ms));
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
