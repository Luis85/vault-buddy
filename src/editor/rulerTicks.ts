/**
 * The timeline ruler's ticks (visual-parity Task 16; concept spec §6.3):
 * the step is the first of the concept's steps whose spacing reaches 70px
 * (`session-safety.js`: `steps.find(n => n * scale >= 70)`), and a label
 * reads `MM:SS`, or `MM:SS.d` when the step is under a second
 * (`drawing-primitives.js`' `fmt`).
 *
 * Pure, in pixels per SECOND (the concept's `scale`). The ruler passes
 * `pxPerMs(zoom) * 1000`: this app's scale comes from the zoom alone
 * (`timelineLayout.BASE_PX_PER_MS`, 50 px/s at zoom 1), not from the
 * window width the concept divides by.
 */

/** Seconds between ticks, finest first. */
export const TICK_STEPS_S: readonly number[] = [0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600];

const MIN_TICK_SPACING_PX = 70;
/** Absorbs float noise at the boundary (`15 × 70/15` is not always 70). */
const EPSILON_PX = 1e-9;
/** A pathological zoom/width pair degrades to sparse ticks, never a runaway loop. */
const MAX_TICKS = 2000;

const WIDEST_STEP_S = TICK_STEPS_S[TICK_STEPS_S.length - 1];

export function tickStep(pps: number): number {
  if (!(pps > 0)) return WIDEST_STEP_S;
  return TICK_STEPS_S.find((step) => step * pps >= MIN_TICK_SPACING_PX - EPSILON_PX) ?? WIDEST_STEP_S;
}

const pad2 = (n: number) => String(n).padStart(2, "0");

export function formatTick(ms: number, step: number): string {
  const t = Math.max(0, Math.round(ms));
  const seconds = Math.floor(t / 1000);
  const base = `${pad2(Math.floor(seconds / 60))}:${pad2(seconds % 60)}`;
  return step < 1 ? `${base}.${Math.floor((t % 1000) / 100)}` : base;
}

export interface RulerTick {
  ms: number;
  x: number;
  label: string;
}

/** Every tick from 0 across `widthPx`, each on a whole millisecond. */
export function rulerTicks(pps: number, widthPx: number): RulerTick[] {
  const step = tickStep(pps);
  const spacing = step * pps;
  const count = spacing > 0 ? Math.min(MAX_TICKS, Math.floor(Math.max(0, widthPx) / spacing) + 1) : 1;
  const out: RulerTick[] = [];
  for (let i = 0; i < count; i += 1) {
    const ms = Math.round(i * step * 1000);
    out.push({ ms, x: (ms / 1000) * pps, label: formatTick(ms, step) });
  }
  return out;
}
