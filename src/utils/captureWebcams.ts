import type { CaptureWebcamInfo } from "../types";

/**
 * Decode `list_capture_webcams`' reply. A VIEW, so it degrades: anything that
 * is not a list is "no webcam", and a row that is not a `webcam:` id with a
 * string label is dropped — rendered, it would become an option whose value
 * Start sends back as a device Rust refuses ("Unknown webcam.").
 */
export function webcamsFrom(raw: unknown): CaptureWebcamInfo[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((row: unknown) => {
    if (typeof row !== "object" || row === null) return [];
    const { id, label } = row as Record<string, unknown>;
    if (typeof id !== "string" || !id.startsWith("webcam:")) return [];
    if (typeof label !== "string") return [];
    return [{ id, label }];
  });
}
