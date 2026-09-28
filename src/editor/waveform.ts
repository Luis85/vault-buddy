/**
 * Waveform geometry (Task 28; F-26; visual-parity Task 18, concept spec
 * §6.5) — PURE functions from the peaks Rust derived
 * (`editor_media_peaks`, one full-scale max-abs value in `0..=1` per
 * bucket over an asset's WHOLE source) to the bars an audio clip draws.
 *
 * A clip shows only its own source range (`in_ms..out_ms`), stretched over
 * its width — speed changes the width, never which sound is drawn. The
 * concept's `waveform`: one rounded vertical bar about every 5 px, at most
 * 130 a clip, centred on y 13 of a 27 px band, scaled to the asset's own
 * loudest bucket. Each bar folds every bucket it covers to their max (the
 * rule Rust's fold uses), so a two-hour asset on a narrow clip costs at
 * most 130 bars, never 4000, and no loud bucket falls between two bars.
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

/** The max of `values[from..to)` (0 when empty). */
function maxOf(values: readonly number[], from: number, to: number): number {
  let max = 0;
  for (let i = from; i < to; i += 1) max = Math.max(max, values[i]);
  return max;
}

/** At most this many bars a clip, one per this many px (concept §6.5). */
const MAX_BARS = 130;
const MIN_BARS = 5;
const PX_PER_BAR = 5;
/** The band is 27 px tall; bars are centred on y 13 and reach 10 px up and
 * down at the asset's loudest, never shorter than 0.6 px either way. */
const MID_Y = 13;
const MAX_HALF = 10;
const MIN_HALF = 0.6;
/** Quieter than this is scaled as if it were this loud, so near-silence
 * never fills the band. */
const MIN_LOUDEST = 0.04;

function fixed1(n: number): string {
  return n.toFixed(1);
}

/** The buckets bar `i` of `count` covers: `inMs..outMs` of a `durationMs`
 * source, as `[from, to)` indices into `n` buckets, never empty. */
function barRange(i: number, count: number, n: number, span: { durationMs: number; inMs: number; outMs: number }) {
  const at = (k: number) => ((span.inMs + (k / count) * (span.outMs - span.inMs)) / span.durationMs) * n;
  const from = Math.max(0, Math.min(n - 1, Math.floor(at(i))));
  return { from, to: Math.max(from + 1, Math.min(n, Math.floor(at(i + 1)))) };
}

/**
 * The SVG path `d` for one clip's bars (`M x top v height` per bar) in a
 * `widthPx` × 27 box — `""` when there is nothing to draw.
 */
export function waveformBars(
  peaks: readonly number[],
  durationMs: number,
  inMs: number,
  outMs: number,
  widthPx: number,
): string {
  if (peaks.length === 0 || durationMs <= 0 || widthPx <= 0 || outMs <= inMs) return "";
  const count = Math.min(MAX_BARS, Math.max(MIN_BARS, Math.round(widthPx / PX_PER_BAR)));
  const loudest = Math.max(MIN_LOUDEST, maxOf(peaks, 0, peaks.length));
  const step = widthPx / count;
  const bars: string[] = [];
  for (let i = 0; i < count; i += 1) {
    const { from, to } = barRange(i, count, peaks.length, { durationMs, inMs, outMs });
    const half = Math.max(MIN_HALF, (maxOf(peaks, from, to) / loudest) * MAX_HALF);
    bars.push(`M${fixed1(i * step + 2)} ${fixed1(MID_Y - half)}v${fixed1(half * 2)}`);
  }
  return bars.join(" ");
}
