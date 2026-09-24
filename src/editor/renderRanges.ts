/**
 * The output-time ranges a render starts from (Task 47; F-42), pure so the
 * Render dialog and the preview toolbar's Review compute them one way.
 *
 * All times are OUTPUT milliseconds, half-open `[startMs, endMs)`, clamped
 * inside `[0, durationMs]` — Rust's render plan refuses anything else, and
 * a Review that asked for footage past the end would render nothing.
 */
import type { Project, RenderRange } from "../editorTypes";
import { clipSpanOf } from "./actionTargets";
import { clipOutputEnd } from "./timeMap";

/** How far either side of the playhead a Review reaches with nothing
 * selected (SCREENS 09 / Task 47's brief: "±5 s around the playhead"). */
const REVIEW_REACH_MS = 5_000;

function clamped(startMs: number, endMs: number, durationMs: number): RenderRange | null {
  const start = Math.max(0, Math.min(startMs, durationMs));
  const end = Math.max(0, Math.min(endMs, durationMs));
  return end > start ? { startMs: start, endMs: end } : null;
}

/** The selected clips' combined output span — from the earliest start to
 * the latest end — or `null` when nothing (still) selected exists. */
export function selectionRange(
  project: Project | null,
  selectionClipIds: readonly string[],
  durationMs: number,
): RenderRange | null {
  const ids = new Set(selectionClipIds);
  const clips = (project?.clips ?? []).filter((c) => ids.has(c.id));
  if (clips.length === 0) return null;
  const start = Math.min(...clips.map((c) => c.start_ms));
  const end = Math.max(...clips.map((c) => clipOutputEnd(clipSpanOf(c))));
  return clamped(start, end, durationMs);
}

/** A Review's range: the selection's span, else `REVIEW_REACH_MS` either
 * side of the playhead. `null` when the project has nothing to render. */
export function reviewRange(
  project: Project | null,
  selectionClipIds: readonly string[],
  playheadMs: number,
  durationMs: number,
): RenderRange | null {
  return (
    selectionRange(project, selectionClipIds, durationMs) ??
    clamped(playheadMs - REVIEW_REACH_MS, playheadMs + REVIEW_REACH_MS, durationMs)
  );
}

/** Seconds as the dialog's number fields show them (`1500` → `"1.5"`). */
export function secondsText(ms: number): string {
  return String(ms / 1000);
}

/** A number field's seconds as whole milliseconds, or `null` for anything
 * that is not a finite, non-negative number. */
export function msFromSeconds(text: string): number | null {
  const trimmed = text.trim();
  const seconds = trimmed === "" ? Number.NaN : Number(trimmed);
  return Number.isFinite(seconds) && seconds >= 0 ? Math.round(seconds * 1000) : null;
}

/** `m:ss.s` for a product card's range (`1500` → `"0:01.5"`). */
export function rangeTimeLabel(ms: number): string {
  const tenths = Math.round(ms / 100);
  const minutes = Math.floor(tenths / 600);
  const seconds = (tenths % 600) / 10;
  return `${minutes}:${seconds.toFixed(1).padStart(4, "0")}`;
}
