/**
 * The preview's sample peak, readable from outside the preview
 * (visual-parity Task 20). `PreviewSurface` owns the non-reactive
 * `PreviewController` (never in Pinia, never a `ref`); the mixer's peak
 * meter used to get a reader from it as a prop, when the mixer sat in the
 * preview's own transport row. Its home is now the timeline footer, a
 * sibling the surface cannot hand a prop to, so the surface registers its
 * reader here while it is mounted and the mixer reads through
 * `readPreviewPeak` — a function, never the controller, and nothing is
 * stored but that function.
 */

type PeakReader = () => number | null;

let reader: PeakReader | null = null;

/** Called by `PreviewSurface` on mount. */
export function setPreviewPeakReader(next: PeakReader): void {
  reader = next;
}

/** Called by `PreviewSurface` on unmount: forgets `mine` only, so a
 * surface mounted meanwhile keeps its own. */
export function clearPreviewPeakReader(mine: PeakReader): void {
  if (reader === mine) reader = null;
}

/** The preview's current sample peak (linear), or `null` with no preview
 * mounted or nothing measured. */
export function readPreviewPeak(): number | null {
  return reader ? reader() : null;
}
