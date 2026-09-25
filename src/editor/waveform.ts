/**
 * Waveform geometry (Task 28; F-26) — PURE functions from the peaks Rust
 * derived (`editor_media_peaks`, one full-scale max-abs value in `0..=1`
 * per bucket over an asset's WHOLE source) to the one SVG polyline a clip
 * draws.
 *
 * A clip shows only its own source range (`in_ms..out_ms`), stretched over
 * its width — speed changes the width, never which sound is drawn. The
 * polyline never has more columns than the clip has pixels (several
 * buckets per pixel fold to their max, the same rule Rust's fold uses), so
 * a two-hour asset on a narrow clip costs a few dozen points, not 4000.
 */

/** `core::editor::peaks::MAX_BUCKETS`. */
const MAX_BUCKETS = 4_000;
/** One bucket per this many ms of source (20 per second) until the cap. */
const MS_PER_BUCKET = 50;

/** The bucket count this app asks for an asset of `durationMs` — FIXED per
 * asset (not per zoom), so every clip of one asset shares one decode and
 * one cache file. */
export function peakBucketsFor(durationMs: number): number {
  return Math.min(MAX_BUCKETS, Math.max(1, Math.ceil(durationMs / MS_PER_BUCKET)));
}

function round1(n: number): number {
  return Math.round(n * 10) / 10;
}

/** The max of `values[from..to)` (0 when empty). */
function maxOf(values: readonly number[], from: number, to: number): number {
  let max = 0;
  for (let i = from; i < to; i += 1) max = Math.max(max, values[i]);
  return max;
}

/** The buckets covering `inMs..outMs` of a `durationMs` source. */
function sliceOf(peaks: readonly number[], durationMs: number, inMs: number, outMs: number): number[] {
  const n = peaks.length;
  const from = Math.max(0, Math.min(n, Math.floor((inMs / durationMs) * n)));
  const to = Math.max(from, Math.min(n, Math.ceil((outMs / durationMs) * n)));
  return peaks.slice(from, to);
}

/**
 * The polyline `points` for one clip: the top envelope left to right, then
 * the bottom envelope right to left, around the lane's vertical middle —
 * `""` when there is nothing to draw.
 */
export function waveformPoints(
  peaks: readonly number[],
  durationMs: number,
  inMs: number,
  outMs: number,
  widthPx: number,
  heightPx: number,
): string {
  if (peaks.length === 0 || durationMs <= 0 || widthPx <= 0) return "";
  const slice = sliceOf(peaks, durationMs, inMs, outMs);
  const columns = Math.min(slice.length, Math.max(1, Math.floor(widthPx)));
  if (columns === 0) return "";
  const mid = heightPx / 2;
  const top: string[] = [];
  const bottom: string[] = [];
  for (let c = 0; c < columns; c += 1) {
    const from = Math.floor((c * slice.length) / columns);
    const to = Math.max(from + 1, Math.floor(((c + 1) * slice.length) / columns));
    const peak = maxOf(slice, from, to);
    const x = round1(((c + 0.5) * widthPx) / columns);
    top.push(`${x},${round1(mid - peak * mid)}`);
    bottom.push(`${x},${round1(mid + peak * mid)}`);
  }
  return [...top, ...bottom.reverse()].join(" ");
}
