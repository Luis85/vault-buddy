/**
 * Decoders for the webcam take replies (Task 49) — their own file so
 * `decode.ts` does not grow toward the 500-line cap (the `decodeRender.ts`
 * precedent). Held to the literals `webcam_commands_tests.rs` pins.
 */
import type { TakeDto, TakeStarted } from "../editorTypes";
import { asBoolean, asId, asInteger, asMs, asObject, fail } from "./decodePrimitives";

/** `editor_webcam_begin`'s `{ takeId }`. */
export function decodeTakeStarted(value: unknown): TakeStarted {
  const v = asObject(value, "takeStarted");
  return { takeId: asId(v.takeId, "takeStarted.takeId") };
}

/** A take's frame size: a take always has a picture, so 0 is not a size. */
function asDimension(value: unknown, field: string): number {
  const n = asInteger(value, field);
  if (n <= 0) fail(`${field} must be positive`);
  return n;
}

/** `editor_webcam_finish`'s `TakeDto`. */
export function decodeTakeDto(value: unknown): TakeDto {
  const v = asObject(value, "take");
  return {
    takeId: asId(v.takeId, "take.takeId"),
    assetId: asId(v.assetId, "take.assetId"),
    durationMs: asMs(v.durationMs, "take.durationMs"),
    width: asDimension(v.width, "take.width"),
    height: asDimension(v.height, "take.height"),
    hasAudio: asBoolean(v.hasAudio, "take.hasAudio"),
  };
}
