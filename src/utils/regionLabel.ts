/** The picker row for a selected region.
 *
 * Deliberately matches the Rust side's own title for the same source
 * (`src-tauri/screen/src/source.rs`, the `Region` arm of `resolve`, which
 * formats `Region on {monitor}`). The capture bar and the staging sidecar
 * read the Rust string; this row reads this one. Two spellings of the same
 * capture is a support problem, so if you change one, change both.
 *
 * ASCII `x` rather than a multiplication sign, matching the existing
 * `{width}x{height}` detail line that `list_capture_sources` produces for a
 * monitor. */
export function regionTitle(monitorTitle: string): string {
  return `Region on ${monitorTitle}`;
}

export function regionDetail(r: {
  x: number;
  y: number;
  width: number;
  height: number;
}): string {
  return `${r.width}x${r.height} at (${r.x}, ${r.y})`;
}
