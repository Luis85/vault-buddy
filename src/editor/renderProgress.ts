/**
 * What a render's progress may claim (Task 47; F-41; R20: "no progress
 * that completes without a terminal record").
 *
 * `fraction: 1` means ffmpeg reached the end of the output — NOT that a
 * product exists: the file is still to be moved into `products\` and
 * recorded in the ledger, and either step can fail. So the bar tops out at
 * 99 % until the job's own `complete` terminal lands, and only that
 * terminal reads as done.
 */
import type { JobPhase, JobTerminal } from "../editorTypes";

/** The part of a job record the bar reads (`editorJobs`' `JobView`). */
export interface RenderProgressView {
  phase: JobPhase;
  fraction: number;
  terminal: JobTerminal | null;
}

/** Is this the job's `complete` terminal — the only completion there is? */
export function isComplete(job: RenderProgressView): boolean {
  return job.phase === "complete" && job.terminal !== null;
}

/** The bar's whole percent: 100 only for `isComplete`, else at most 99. */
export function renderPercent(job: RenderProgressView): number {
  if (isComplete(job)) return 100;
  const fraction = Number.isFinite(job.fraction) ? Math.min(1, Math.max(0, job.fraction)) : 0;
  return Math.min(99, Math.floor(fraction * 100));
}

/** Each phase in words (SCREENS 09: "progress with phases"). */
export const PHASE_LABELS: Record<JobPhase, string> = {
  queued: "Waiting to start…",
  preparing: "Preparing the render…",
  rendering: "Rendering…",
  publishing: "Saving the rendered file…",
  complete: "Render complete",
  cancelled: "Render cancelled",
  failed: "Render failed",
};
