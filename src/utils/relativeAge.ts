/**
 * How long ago something happened, in the coarsest unit that fits.
 *
 * Written for the resume-or-discard list (spec §10: "each with its source,
 * duration and age"), which is how a user finds work they abandoned — so the
 * useful answer is "yesterday-ish", never a timestamp they have to subtract
 * in their head.
 *
 * `nowMs` is a parameter rather than a `Date.now()` call inside, so a test
 * can pin an age without owning the clock and every caller in one render
 * measures against the same instant.
 */

/** Largest first, so the first unit that fits is the one to report. */
const UNITS: ReadonlyArray<readonly [ms: number, suffix: string]> = [
  [86_400_000, "d"],
  [3_600_000, "h"],
  [60_000, "m"],
];

export function relativeAgeLabel(recordedAt: string, nowMs: number): string {
  const then = Date.parse(recordedAt);
  // The staging sidecar is hand-editable, so this string is not a guarantee
  // (the same posture `StagedCaptureDetail.timeline` documents). An unparsed
  // date must render as NOTHING — a bare subtraction prints the literal
  // "NaN ago", which reads as a corrupted recording rather than as an
  // unknown timestamp.
  if (Number.isNaN(then)) return "";
  // Clamped: a clock that moved backwards between the recording and this
  // render would otherwise report footage made in the future.
  const ms = Math.max(0, nowMs - then);
  for (const [unit, suffix] of UNITS) {
    if (ms >= unit) return `${Math.floor(ms / unit)}${suffix} ago`;
  }
  return "just now";
}
