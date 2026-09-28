/**
 * Why a render cannot start right now (Task 47, R20: a disabled control
 * says why), shared by the header's **Render video** and the library
 * Project section's **Render a new video** (visual-parity Task 10), which
 * open the same dialog and must refuse for the same reasons.
 */
export function renderRefusal(sessionId: string | null, durationMs: number): string | null {
  if (!sessionId) return "No project is open.";
  if (durationMs === 0) return "Place a clip on the timeline to render a video.";
  return null;
}
