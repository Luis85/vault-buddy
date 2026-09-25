/**
 * Shared primitive checks `decode.ts`/`decodeProject.ts` build every
 * decoder from (F31 split — kept out of both so neither file re-declares
 * the same field-by-field checks). Every function either returns a
 * narrowed value or throws `ProtocolError`; none of them guess or coerce.
 */

/** Thrown by every decoder in this module family — a malformed or
 * version-skewed `editor_*` reply, never a caller bug. */
export class ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

/** `core::editor::limits::MAX_DURATION_MS` — the two-hour reference safety
 * bound applied to every `*Ms` field this module decodes, not only a
 * project's own total duration: no ms-typed field anywhere in the
 * interchange document can legitimately exceed it either. Not exported:
 * only `asMs`, in this same file, needs it. */
const MAX_DURATION_MS = 7_200_000;

/** `core::editor::limits`'s ID regex `^[a-zA-Z0-9_-]{1,100}$`
 * (`core::editor::ids::is_valid_id`). */
const ID_PATTERN = /^[a-zA-Z0-9_-]{1,100}$/;

export function fail(message: string): never {
  throw new ProtocolError(message);
}

export function asObject(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail(`${field} must be an object`);
  }
  return value as Record<string, unknown>;
}

export function asArray(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) fail(`${field} must be an array`);
  return value;
}

export function asString(value: unknown, field: string): string {
  if (typeof value !== "string") fail(`${field} must be a string`);
  return value;
}

export function asOptionalString(value: unknown, field: string): string | undefined {
  if (value === undefined) return undefined;
  return asString(value, field);
}

/** `null` stays `null`; an absent key is a decoding error, not "none" —
 * every nullable field in the Contract reference is an always-present key
 * (the `EditorOpenResult`/`EditorSnapshot` module docs both say so). */
export function asNullableString(value: unknown, field: string): string | null {
  if (value === null) return null;
  return asString(value, field);
}

/** A validated entity id — `core::editor::ids::is_valid_id`'s pattern,
 * checked here so a malformed id (wrong charset, empty, over 100 chars)
 * fails at the decode boundary rather than surfacing as a mysterious
 * lookup miss three components later. */
export function asId(value: unknown, field: string): string {
  const s = asString(value, field);
  if (!ID_PATTERN.test(s)) fail(`${field} is not a valid entity id: ${s}`);
  return s;
}

export function asOptionalId(value: unknown, field: string): string | undefined {
  if (value === undefined) return undefined;
  return asId(value, field);
}

export function asBoolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") fail(`${field} must be a boolean`);
  return value;
}

export function asOptionalBoolean(value: unknown, field: string): boolean | undefined {
  if (value === undefined) return undefined;
  return asBoolean(value, field);
}

export function asNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) fail(`${field} must be a finite number`);
  return value;
}

export function asOptionalNumber(value: unknown, field: string): number | undefined {
  if (value === undefined) return undefined;
  return asNumber(value, field);
}

/** A JSON-safe integer — `Number.isSafeInteger`, so a `2**53` from a
 * hostile or corrupted reply is rejected rather than silently losing
 * precision. */
export function asInteger(value: unknown, field: string): number {
  const n = asNumber(value, field);
  if (!Number.isSafeInteger(n)) fail(`${field} must be a safe integer`);
  return n;
}

// Not exported: used only by `asMs` below, in this same file — no other
// module needs a bare "non-negative integer, no ms bound" check yet.
function asNonNegativeInteger(value: unknown, field: string): number {
  const n = asInteger(value, field);
  if (n < 0) fail(`${field} must not be negative`);
  return n;
}

/** A millisecond timestamp/duration, bounded at `MAX_DURATION_MS`. */
export function asMs(value: unknown, field: string): number {
  const n = asNonNegativeInteger(value, field);
  if (n > MAX_DURATION_MS) fail(`${field} exceeds the two-hour reference safety bound`);
  return n;
}

/** Checks `value` is a string drawn from `members`, returning it narrowed
 * to `T` — every enum in the interchange document is closed, so an
 * unrecognized member is always a decoding error, never a silent pass-through. */
export function asEnum<T extends string>(value: unknown, field: string, members: readonly T[]): T {
  const s = asString(value, field);
  if (!(members as readonly string[]).includes(s)) {
    fail(`${field} is not a recognized value: ${s}`);
  }
  return s as T;
}

export function asOptionalEnum<T extends string>(
  value: unknown,
  field: string,
  members: readonly T[],
): T | undefined {
  if (value === undefined) return undefined;
  return asEnum(value, field, members);
}
