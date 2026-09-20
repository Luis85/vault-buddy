import type { TimelineDto } from "../types";

/** The output duration: the sum of the segments' own lengths, NOT the span
 * of the source they were cut from. Everything the strip renders is a
 * fraction of this. */
export function outputDurationMs(t: TimelineDto): number {
  return t.segments.reduce((sum, s) => sum + (s.sourceEndMs - s.sourceStartMs), 0);
}

/** Each segment's share of the strip, as a percentage. Empty in, empty out —
 * an empty timeline must not divide by zero. */
export function segmentWidths(t: TimelineDto): number[] {
  const total = outputDurationMs(t);
  if (total === 0) return [];
  return t.segments.map((s) => ((s.sourceEndMs - s.sourceStartMs) / total) * 100);
}

/** Which segment an output time falls in, or `null` past the end.
 *
 * A boundary belongs to the segment it STARTS. The alternative puts the
 * playhead in the segment that just ended, which makes the preview seek
 * backwards at every cut. */
export function segmentAtOutputMs(t: TimelineDto, outputMs: number): number | null {
  if (outputMs < 0) return null;
  let acc = 0;
  for (let i = 0; i < t.segments.length; i += 1) {
    acc += t.segments[i].sourceEndMs - t.segments[i].sourceStartMs;
    if (outputMs < acc) return i;
  }
  return null;
}

/** Output time → source time. `null` past the end.
 *
 * This is the function the preview seeks on, and the reason the editor can
 * cut at all: the two clocks are different, and only the timeline knows the
 * mapping. Mirrors `core::timeline::to_source_ms`; keep them in step. */
export function toSourceMs(t: TimelineDto, outputMs: number): number | null {
  if (outputMs < 0) return null;
  let acc = 0;
  for (const s of t.segments) {
    const len = s.sourceEndMs - s.sourceStartMs;
    if (outputMs < acc + len) return s.sourceStartMs + (outputMs - acc);
    acc += len;
  }
  return null;
}

/** Source time -> output time, or `null` when that source moment was cut
 * out and is nowhere in the output.
 *
 * The inverse of `toSourceMs`, and the preview depends on it: the `<video>`
 * element reports SOURCE time while everything the user sees — the strip,
 * the playhead, the scrubber — speaks output time. Without this the two
 * clocks drift apart at the first cut. */
export function toOutputMs(t: TimelineDto, sourceMs: number): number | null {
  let acc = 0;
  for (const s of t.segments) {
    if (sourceMs >= s.sourceStartMs && sourceMs < s.sourceEndMs) {
      return acc + (sourceMs - s.sourceStartMs);
    }
    acc += s.sourceEndMs - s.sourceStartMs;
  }
  return null;
}

/** Where a drop at `fractionX` (0-1 along the strip) inserts.
 *
 * Midpoint rule: past a block's centre means after it. Returns a value in
 * `[0, widths.length]` — the upper bound is "dropped at the end", which is a
 * real destination, not an overflow. */
export function dropIndex(widths: number[], fractionX: number): number {
  const x = fractionX * 100;
  let acc = 0;
  for (let i = 0; i < widths.length; i += 1) {
    if (x < acc + widths[i] / 2) return i;
    acc += widths[i];
  }
  return widths.length;
}
