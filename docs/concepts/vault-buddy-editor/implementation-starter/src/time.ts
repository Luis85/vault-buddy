/** Shared edit-file convention is integer milliseconds. Source ranges are half-open. */
export interface ClipSpan { startMs: number; sourceInMs: number; sourceOutMs: number; speed: number }
export function durationMs(clip: ClipSpan): number {
  if (![clip.startMs,clip.sourceInMs,clip.sourceOutMs].every(v => Number.isSafeInteger(v) && v >= 0) || clip.sourceOutMs <= clip.sourceInMs || !Number.isFinite(clip.speed) || clip.speed < .25 || clip.speed > 4) throw new Error('Invalid clip span');
  return Math.round((clip.sourceOutMs - clip.sourceInMs) / clip.speed);
}
export function sourceAt(clip: ClipSpan, outputMs: number): number|null {
  const length = durationMs(clip);
  if (!Number.isFinite(outputMs) || outputMs < clip.startMs || outputMs >= clip.startMs + length) return null;
  return Math.min(clip.sourceOutMs - 1, Math.floor(clip.sourceInMs + (outputMs-clip.startMs)*clip.speed));
}
/** Media Foundation 100-ns ticks. Use BigInt internally; never send it unencoded over JSON. */
export function frameTimestamp(frame: bigint, fpsNumerator: bigint, fpsDenominator: bigint): bigint {
  if (frame < 0n || fpsNumerator <= 0n || fpsDenominator <= 0n) throw new Error('Invalid frame timebase');
  return frame * 10_000_000n * fpsDenominator / fpsNumerator;
}
