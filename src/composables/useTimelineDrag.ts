/**
 * The timeline's drag/trim/nudge interaction model (Task 21; F-07, F-08,
 * F-09, F-10, F-11, F-13). `ClipItem.vue` is the one caller: it owns every
 * DOM-touching detail (pointer capture, `event.currentTarget`, focus) the
 * `TimelineRuler.vue` precedent already establishes for this timeline, and
 * calls into this composable with plain numbers so the interaction math
 * itself stays Vue-DOM-free and directly testable (`tests/useTimelineDrag.test.ts`
 * calls every function here with no `mount()` at all).
 *
 * **One instance per `ClipItem`, not one shared instance.** Only one clip is
 * ever actively dragged at a time, so there is no need for a `clipId`-tagged
 * preview object a caller has to filter by identity — each `ClipItem`'s own
 * `movePreview`/`trimPreview` refs are implicitly scoped to the clip THAT
 * instance renders, the same way it already calls `useEditorWorkspaceStore()`
 * per-instance today.
 *
 * **Snap is wired here** (Task 20 shipped `timelineLayout.snap` with no
 * consumer — its own module doc says so): `computeMoveDelta`/`computeTrimStart`/
 * `computeTrimEnd` all snap the DRAGGED EDGE (never the delta directly) against
 * `deps.snapTargets()` when `deps.snapEnabled()` is true, so the toolbar's Snap
 * toggle finally changes something.
 *
 * **The 100 ms minimum is enforced HERE too**, mirroring
 * `core::editor::limits::MIN_CLIP_MS` (F14) — a trim preview that ignored it
 * would let the user watch a clip shrink past zero during the drag and only
 * discover the refusal on release; Rust's own `trimClip`/`splitClip` refusal
 * is still the authority (this is a preview clamp, never a substitute for
 * it).
 */
import type { Ref } from "vue";
import { ref } from "vue";

import type { EditorCommand } from "../editor/editorCommandTypes";
import { LANE_HEIGHT_PX, pxPerMs, snap } from "../editor/timelineLayout";
import { clipOutputDuration, clipOutputEnd, roundHalfAway } from "../editor/timeMap";
import type { Clip } from "../editorTypes";

/** Mirrors `core::editor::mod::limits::MIN_CLIP_MS` (Task 21, F14) — read
 * from the Rust source, never invented (`useInspectorDraft.ts`'s own rule
 * for every client-side mirror of a server limit). */
export const MIN_CLIP_MS = 100;

/** How close (px) a dragged edge must land to a snap target to catch it —
 * no contract value names one, so this is this module's own constant, the
 * `BASE_PX_PER_MS`/`MIN_TICK_PX` precedent in `timelineLayout.ts` for a UX
 * constant with no Rust twin. Exported (Task 26) so `TimelineView.vue`'s
 * own asset-placement drop snaps against the exact same threshold a clip
 * drag/trim already does, rather than a second hand-copied `8`. */
export const SNAP_THRESHOLD_PX = 8;

export interface MovePreview {
  deltaMs: number;
}

export interface TrimPreview {
  startMs: number;
  inMs: number;
  outMs: number;
}

export interface SnapOptions {
  snapEnabled: boolean;
  targets: readonly number[];
  thresholdPx: number;
  zoom: number;
}

function clipSpeed(clip: Clip): number {
  return clip.speed ?? 1;
}

/** Snaps `rawMs` against `opts` when enabled, else returns it unchanged —
 * the one ternary `computeMoveDelta`/`computeTrimStart`/`computeTrimEnd`
 * each repeated inline below. Factored out (Task 26) once `TimelineView`'s
 * own asset-placement drop needed the exact same "snap if the toolbar's
 * Snap toggle is on" behaviour as a THIRD consumer outside this file,
 * rather than growing a second copy of that ternary beside
 * `trackCompat.trackAccepts` (a different rule this same task also reuses
 * rather than re-deriving). */
export function snappedMs(rawMs: number, opts: SnapOptions): number {
  return opts.snapEnabled ? snap(rawMs, opts.targets, opts.thresholdPx, opts.zoom) : rawMs;
}

/**
 * The clamped, optionally-snapped `deltaMs` a body drag should apply to
 * `clip.start_ms` — computed from the clip's OWN new start (never the raw
 * delta directly), so snapping catches the dragged edge landing near a
 * target rather than snapping an offset that has no spatial meaning of its
 * own. Clamped so `clip.start_ms + deltaMs` never goes negative — the same
 * "never below zero" rule `moveClips`' own Rust-side shared-delta clamp
 * enforces, mirrored here so the preview never shows a position Rust would
 * refuse outright.
 */
export function computeMoveDelta(clip: Clip, rawDeltaMs: number, opts: SnapOptions): number {
  const rawNewStart = clip.start_ms + rawDeltaMs;
  const snappedStart = snappedMs(rawNewStart, opts);
  // Whole milliseconds: every `EditorCommand` time is an integer (Rust
  // decodes u64/i64), and a pointer offset at a non-integer px-per-ms is not.
  const clampedStart = Math.round(Math.max(0, snappedStart));
  return clampedStart - clip.start_ms;
}

/**
 * Trimming the START handle: `outMs` stays fixed, `startMs` moves by the
 * (clamped, optionally snapped) OUTPUT delta, and `inMs` is derived from the
 * resulting output duration via the same `out_ms - duration*speed` inverse
 * `time.rs`'s own formula implies — never a second, independently-invented
 * mapping. Clamped so `startMs` never goes below 0, never extends further
 * left than the source has footage (`inMs` reaching 0 — clamping `inMs`
 * alone while `startMs` kept moving would detach the clip's END, which a
 * start-handle trim must keep fixed), and the resulting output duration
 * never drops below `MIN_CLIP_MS` (F14).
 */
export function computeTrimStart(clip: Clip, rawOutputDeltaMs: number, opts: SnapOptions): TrimPreview {
  const speed = clipSpeed(clip);
  const originalEnd = clip.start_ms + clipOutputDuration(clip.in_ms, clip.out_ms, speed);
  const rawNewStart = clip.start_ms + rawOutputDeltaMs;
  const snappedStart = snappedMs(rawNewStart, opts);
  const minStart = Math.max(0, originalEnd - clipOutputDuration(0, clip.out_ms, speed));
  const maxStart = Math.max(minStart, originalEnd - MIN_CLIP_MS);
  const clampedStart = Math.round(Math.min(Math.max(snappedStart, minStart), maxStart));
  const newDuration = originalEnd - clampedStart;
  const newIn = Math.max(clip.out_ms - roundHalfAway(newDuration * speed), 0);
  return { startMs: clampedStart, inMs: newIn, outMs: clip.out_ms };
}

/**
 * Trimming the END handle: `startMs`/`inMs` stay fixed, the clip's OUTPUT
 * end moves by the (clamped, optionally snapped) delta, and `outMs` is
 * derived the same inverse way `computeTrimStart` uses. Clamped so the
 * resulting output duration never drops below `MIN_CLIP_MS` (F14) — there is
 * no client-side upper bound (the asset's own duration), so a preview that
 * extends past it is left to Rust's own refusal on commit, exactly as the
 * brief scopes the preview clamp to the MINIMUM only.
 */
export function computeTrimEnd(clip: Clip, rawOutputDeltaMs: number, opts: SnapOptions): TrimPreview {
  const speed = clipSpeed(clip);
  const originalEnd = clipOutputEnd({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed });
  const rawNewEnd = originalEnd + rawOutputDeltaMs;
  const snappedEnd = snappedMs(rawNewEnd, opts);
  const minEnd = clip.start_ms + MIN_CLIP_MS;
  const clampedEnd = Math.round(Math.max(snappedEnd, minEnd));
  const newDuration = clampedEnd - clip.start_ms;
  const newOut = Math.max(clip.in_ms + roundHalfAway(newDuration * speed), clip.in_ms + 1);
  return { startMs: clip.start_ms, inMs: clip.in_ms, outMs: newOut };
}

/** The clamped fade duration (ms) dragging `edge`'s handle by `rawDeltaMs`
 * should produce — mirrors `core::editor::commands::fades::fade_limit`
 * (`duration_ms / 2`, an integer floor) as a client-side preview clamp,
 * never a substitute for Rust's own refusal on commit. The two handles sit
 * at opposite visual corners, so their drag directions are opposite too:
 * dragging the fade-IN handle RIGHTWARD (a positive `rawDeltaMs`) lengthens
 * it, while dragging the fade-OUT handle LEFTWARD (a negative `rawDeltaMs`,
 * toward the clip's start) lengthens IT — hence the sign flip on `edge ===
 * "out"`. No snapping: SCREENS-AND-INTERACTIONS.md names snap only for
 * move/trim, and a fade is a duration, not a position, so there is no
 * timeline instant to snap onto. */
export function computeFadeDrag(clip: Clip, edge: "in" | "out", rawDeltaMs: number): number {
  const speed = clipSpeed(clip);
  const duration = clipOutputDuration(clip.in_ms, clip.out_ms, speed);
  const limit = Math.floor(duration / 2);
  const start = edge === "in" ? clip.fade_in_ms : clip.fade_out_ms;
  const signedDelta = edge === "in" ? rawDeltaMs : -rawDeltaMs;
  return Math.round(Math.min(Math.max(start + signedDelta, 0), limit));
}

export interface FadePreview {
  edge: "in" | "out";
  ms: number;
}

export interface TimelineDragDeps {
  /** The clip THIS composable instance is scoped to — a getter so a caller
   * that re-renders with a fresh `Clip` object (every commit replaces the
   * project wholesale, `editorProject.ts`'s own discipline) always drives
   * the math off the CURRENT one. */
  clip: () => Clip;
  zoom: () => number;
  snapEnabled: () => boolean;
  snapTargets: () => readonly number[];
  /** The clip ids a body drag or nudge should move TOGETHER with this one —
   * the caller's own "is this clip part of a multi-selection" answer
   * (`[clip.id]` otherwise), the same rule `actions.ts`' `targetClipIds`
   * applies for a pointer-driven mutation. */
  moveTargetClipIds: () => string[];
  /** Lane order (top to bottom) for the cross-track drop hit-test at
   * pointer-up — `TimelineView.vue`'s own `tracks` computed, id-only. */
  trackOrder: () => readonly string[];
  /** This clip's own index into `trackOrder()`. */
  trackIndex: () => number;
  /** Whether a lane may receive THIS clip on a cross-lane drop: the same
   * track kind as the clip's asset, and unlocked — the two rules Rust's
   * `moveClips` enforces on a `trackId` (`clips.rs`), refusing the WHOLE
   * move, horizontal delta included, when either fails. */
  trackAccepts: (trackId: string) => boolean;
  execute: (command: EditorCommand) => unknown;
}

export interface UseTimelineDrag {
  movePreview: Ref<MovePreview | null>;
  trimPreview: Ref<TrimPreview | null>;
  beginBodyDrag: (clientX: number, clientY: number) => void;
  updateBodyDrag: (clientX: number) => void;
  endBodyDrag: (clientY: number) => Promise<void>;
  cancelBodyDrag: () => void;
  beginTrim: (edge: "start" | "end", clientX: number) => void;
  updateTrim: (clientX: number) => void;
  endTrim: () => Promise<void>;
  cancelTrim: () => void;
  fadePreview: Ref<FadePreview | null>;
  beginFade: (edge: "in" | "out", clientX: number) => void;
  updateFade: (clientX: number) => void;
  endFade: () => Promise<void>;
  cancelFade: () => void;
  nudge: (deltaMs: number) => Promise<void>;
}

export function useTimelineDrag(deps: TimelineDragDeps): UseTimelineDrag {
  const movePreview = ref<MovePreview | null>(null);
  const trimPreview = ref<TrimPreview | null>(null);

  function snapOpts(): SnapOptions {
    return {
      snapEnabled: deps.snapEnabled(),
      targets: deps.snapTargets(),
      thresholdPx: SNAP_THRESHOLD_PX,
      zoom: deps.zoom(),
    };
  }

  // ---- body drag ------------------------------------------------------------

  let moveAnchor: { clientX: number; clientY: number } | null = null;

  function beginBodyDrag(clientX: number, clientY: number): void {
    moveAnchor = { clientX, clientY };
    movePreview.value = { deltaMs: 0 };
  }

  function updateBodyDrag(clientX: number): void {
    if (!moveAnchor) return;
    const ppm = pxPerMs(deps.zoom());
    const rawDeltaMs = ppm > 0 ? (clientX - moveAnchor.clientX) / ppm : 0;
    movePreview.value = { deltaMs: computeMoveDelta(deps.clip(), rawDeltaMs, snapOpts()) };
  }

  /**
   * Cross-track detection: a lane-count offset from the drag's own vertical
   * pixel distance (rounded, then clamped into `trackOrder()`'s bounds) —
   * no DOM hit-test against every OTHER `TrackLane`'s own rect is needed,
   * because lanes are laid out in one fixed-height column
   * (`LANE_HEIGHT_PX`) in `trackOrder()`'s own order. A lane that does not
   * accept the clip (`deps.trackAccepts` — wrong kind, or locked) is not a
   * drop target: the clip stays on its own track and the horizontal delta
   * still lands, rather than sending a `trackId` Rust would refuse together
   * with the whole move (fix round 1).
   */
  function targetTrackFor(clip: Clip, clientY: number): string | null {
    const order = deps.trackOrder();
    if (order.length === 0 || !moveAnchor) return null;
    const laneDelta = Math.round((clientY - moveAnchor.clientY) / LANE_HEIGHT_PX);
    const targetIndex = Math.min(Math.max(deps.trackIndex() + laneDelta, 0), order.length - 1);
    const targetId = order[targetIndex];
    if (!targetId || targetId === clip.track_id) return null;
    return deps.trackAccepts(targetId) ? targetId : null;
  }

  async function endBodyDrag(clientY: number): Promise<void> {
    if (!moveAnchor) return;
    const clip = deps.clip();
    const deltaMs = movePreview.value?.deltaMs ?? 0;
    const clipIds = deps.moveTargetClipIds();
    // `trackId` is only ever valid for a single moved clip — the exact rule
    // Rust's own `moveClips` enforces (`clips.rs`: "trackId is only valid
    // when moving a single clip"), checked here so a multi-selection drag
    // never sends a `trackId` Rust would refuse outright.
    const trackId = clipIds.length === 1 ? targetTrackFor(clip, clientY) : null;
    moveAnchor = null;
    movePreview.value = null;
    if (deltaMs === 0 && trackId === null) return;
    await deps.execute({ kind: "moveClips", clipIds, deltaMs, trackId });
  }

  function cancelBodyDrag(): void {
    moveAnchor = null;
    movePreview.value = null;
  }

  // ---- trim -------------------------------------------------------------

  let trimEdge: "start" | "end" | null = null;
  let trimStartClientX = 0;

  function beginTrim(edge: "start" | "end", clientX: number): void {
    trimEdge = edge;
    trimStartClientX = clientX;
    const clip = deps.clip();
    trimPreview.value = { startMs: clip.start_ms, inMs: clip.in_ms, outMs: clip.out_ms };
  }

  function updateTrim(clientX: number): void {
    if (!trimEdge) return;
    const ppm = pxPerMs(deps.zoom());
    const rawOutputDeltaMs = ppm > 0 ? (clientX - trimStartClientX) / ppm : 0;
    const clip = deps.clip();
    trimPreview.value =
      trimEdge === "start"
        ? computeTrimStart(clip, rawOutputDeltaMs, snapOpts())
        : computeTrimEnd(clip, rawOutputDeltaMs, snapOpts());
  }

  async function endTrim(): Promise<void> {
    if (!trimEdge) return;
    const clip = deps.clip();
    const preview = trimPreview.value;
    trimEdge = null;
    trimPreview.value = null;
    if (!preview) return;
    if (preview.startMs === clip.start_ms && preview.inMs === clip.in_ms && preview.outMs === clip.out_ms) {
      return;
    }
    await deps.execute({
      kind: "trimClip",
      clipId: clip.id,
      startMs: preview.startMs,
      inMs: preview.inMs,
      outMs: preview.outMs,
    });
  }

  function cancelTrim(): void {
    trimEdge = null;
    trimPreview.value = null;
  }

  // ---- fade handles (Task 29; F-17, F-18) --------------------------------
  // The `beginTrim`/`updateTrim`/`endTrim`/`cancelTrim` shape exactly: a
  // preview during the drag, ONE `setFades` on release, Escape discards
  // with nothing sent — never a second drag model (the brief's own
  // instruction). Unlike trim, a fade changes exactly one field per drag
  // (`fadeInMs` XOR `fadeOutMs`), so the preview is a single `{edge, ms}`
  // pair rather than a whole clip span.

  const fadePreview = ref<FadePreview | null>(null);
  let fadeEdge: "in" | "out" | null = null;
  let fadeStartClientX = 0;

  function beginFade(edge: "in" | "out", clientX: number): void {
    fadeEdge = edge;
    fadeStartClientX = clientX;
    const clip = deps.clip();
    fadePreview.value = { edge, ms: edge === "in" ? clip.fade_in_ms : clip.fade_out_ms };
  }

  function updateFade(clientX: number): void {
    if (!fadeEdge) return;
    const ppm = pxPerMs(deps.zoom());
    const rawDeltaMs = ppm > 0 ? (clientX - fadeStartClientX) / ppm : 0;
    fadePreview.value = { edge: fadeEdge, ms: computeFadeDrag(deps.clip(), fadeEdge, rawDeltaMs) };
  }

  async function endFade(): Promise<void> {
    if (!fadeEdge) return;
    const clip = deps.clip();
    const preview = fadePreview.value;
    const edge = fadeEdge;
    fadeEdge = null;
    fadePreview.value = null;
    if (!preview) return;
    const current = edge === "in" ? clip.fade_in_ms : clip.fade_out_ms;
    if (preview.ms === current) return;
    await deps.execute(
      edge === "in"
        ? { kind: "setFades", clipId: clip.id, fadeInMs: preview.ms }
        : { kind: "setFades", clipId: clip.id, fadeOutMs: preview.ms },
    );
  }

  function cancelFade(): void {
    fadeEdge = null;
    fadePreview.value = null;
  }

  // ---- keyboard nudge -----------------------------------------------------

  /** One `moveClips` per key press, no preview step — a nudge commits
   * immediately (there is nothing to preview: the whole point is a discrete,
   * already-decided step, not a drag the user is still watching). */
  async function nudge(deltaMs: number): Promise<void> {
    await deps.execute({ kind: "moveClips", clipIds: deps.moveTargetClipIds(), deltaMs, trackId: null });
  }

  return {
    movePreview,
    trimPreview,
    beginBodyDrag,
    updateBodyDrag,
    endBodyDrag,
    cancelBodyDrag,
    beginTrim,
    updateTrim,
    endTrim,
    cancelTrim,
    fadePreview,
    beginFade,
    updateFade,
    endFade,
    cancelFade,
    nudge,
  };
}
