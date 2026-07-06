/**
 * Formats a duration in milliseconds the way RecordingBar's `elapsed` and
 * Transcriptions' `elapsed()` used to inline, independently, in the exact
 * same way: `m:ss` below an hour, rolling over to `h:mm:ss` at/after one
 * hour. Extracted byte-for-byte (Task 9/C2) so both callers render
 * identically and any future caller doesn't grow a third copy. Negative
 * input (a stale/inverted timestamp diff) clamps to "0:00" rather than
 * printing a negative duration.
 */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${m}:${String(s).padStart(2, "0")}`;
}
