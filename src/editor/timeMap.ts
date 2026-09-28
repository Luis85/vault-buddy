/**
 * TypeScript half of the shared editor time-mapping table (R15, R4).
 *
 * Rust (`core::editor::session`) is authoritative for every WRITE — every
 * edit command executes there and installs a new project graph. This
 * module is the READ-side twin the preview, the drag preview and the
 * ruler consult while the user is just looking at the timeline; it is
 * held to Rust's `core::editor::time` by one shared fixture table,
 * `tests/fixtures/editor-time-cases.json`, read by both
 * `core/src/editor/time.rs` (`include_str!`) and
 * `tests/editorTimeFixtures.test.ts`. Moving or deleting that file breaks
 * the RUST build, not just a TypeScript import — the discipline
 * `timeline-cases.json` used while it had a TypeScript reader (GAP-136), so
 * a divergence between the two languages can never pass with every test in
 * the repo green.
 *
 * Half-open everywhere, exactly as the Rust twin: a clip's output span is
 * `[start_ms, end_ms)` and its source span is `[in_ms, out_ms)`.
 *
 * `frame_timestamp` has NO TypeScript twin — it is Rust-only (the render
 * plan's Media Foundation timestamps never leave the backend), so it is
 * not part of this module.
 */
import type { ClipSpan } from "../editorTypes";

/**
 * Round half away from zero. `Math.round` already rounds ties toward
 * +Infinity, which for every non-negative value this module ever rounds
 * (a duration, an elapsed time, or a ratio of two of them) IS "away from
 * zero" — so this is deliberately a plain `Math.round` and not a
 * "more correct" banker's-rounding helper. The shared fixture's
 * `rounding_boundary_half` case exists to catch exactly that regression:
 * swapping this for round-half-to-even makes its `durationMs: 3` (from
 * `round(5/2)`, i.e. `round(2.5)`) read `2` instead.
 */
export function roundHalfAway(x: number): number {
  return Math.round(x);
}

/** `round((out_ms - in_ms) / speed)` (`DATA-MODEL.md` § Timing rules). */
export function clipOutputDuration(inMs: number, outMs: number, speed: number): number {
  return roundHalfAway((outMs - inMs) / speed);
}

/** `start_ms + clipOutputDuration(in_ms, out_ms, speed)`. */
export function clipOutputEnd(clip: ClipSpan): number {
  return clip.start_ms + clipOutputDuration(clip.in_ms, clip.out_ms, clip.speed);
}

/** `start_ms <= t < end_ms` — half-open, so the end instant itself is not
 * part of the clip's active span. */
export function clipIsActive(clip: ClipSpan, t: number): boolean {
  return t >= clip.start_ms && t < clipOutputEnd(clip);
}

/** Maps an output instant to the source instant it plays, or `null`
 * outside the clip's active `[start_ms, end_ms)` span. Clamped to
 * `[in_ms, out_ms - 1]` — the source range is half-open, so `out_ms`
 * itself is never a playable source frame. */
export function sourceAt(clip: ClipSpan, t: number): number | null {
  if (!clipIsActive(clip, t)) return null;
  const raw = (t - clip.start_ms) * clip.speed;
  const s = clip.in_ms + roundHalfAway(raw);
  return Math.min(clip.out_ms - 1, Math.max(clip.in_ms, s));
}

/** The source instant output `t` plays on `clip`, clamped to the INCLUSIVE
 * `[in_ms, out_ms]` — for a teaching cue's END, which may sit exactly on
 * `out_ms` (`cues.rs`'s `check_cue_range`), where `sourceAt` stops at
 * `out_ms - 1`. The effect inspector's commit path (Task 35). TS-only, like
 * `frame_timestamp` is Rust-only: the wire already carries source times, so
 * Rust never makes this conversion and there is no twin to fixture. */
export function sourceAtClamped(clip: ClipSpan, t: number): number {
  const s = clip.in_ms + roundHalfAway((t - clip.start_ms) * clip.speed);
  return Math.min(clip.out_ms, Math.max(clip.in_ms, s));
}

/** Maps a source instant back to the output instant it appears at, for
 * `source` in the clip's half-open `[in_ms, out_ms)` range; `null`
 * outside it. */
export function outputAt(clip: ClipSpan, source: number): number | null {
  if (source < clip.in_ms || source >= clip.out_ms) return null;
  const raw = (source - clip.in_ms) / clip.speed;
  return clip.start_ms + roundHalfAway(raw);
}

/** The output-timeline span a source-time cue `[cueStartSrc, cueEndSrc)`
 * maps to, once clipped to the clip's own `[in_ms, out_ms)` source range;
 * `null` when the clipped intersection is empty. */
export function cueOutputSpan(
  clip: ClipSpan,
  cueStartSrc: number,
  cueEndSrc: number,
): [number, number] | null {
  const start = Math.max(cueStartSrc, clip.in_ms);
  const end = Math.min(cueEndSrc, clip.out_ms);
  if (start >= end) return null;
  // `start` is guaranteed inside `[in_ms, out_ms)` here (`start < end <=
  // out_ms` and `start >= in_ms`), so this is the exact mapping
  // `outputAt` performs — written out directly so this function's return
  // type never has to unwrap a `null` that cannot occur.
  const outStart = clip.start_ms + roundHalfAway((start - clip.in_ms) / clip.speed);
  // `end`, unlike `start`, may legitimately equal `out_ms` when the cue
  // was clipped there — `outputAt` would reject that boundary as out of
  // range, so the end is mapped by the same raw formula rather than
  // through it.
  const outEnd = clip.start_ms + roundHalfAway((end - clip.in_ms) / clip.speed);
  return [outStart, outEnd];
}
