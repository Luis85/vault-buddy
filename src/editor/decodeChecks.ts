/**
 * The decoder for `editor_get_checks`' `CheckFinding[]` (Task 54) — its
 * own file so `decode.ts` does not grow toward the 500-line cap (the
 * `decodeRender.ts` precedent). Held to the literal
 * `a_finding_serializes_to_the_contract_literal` pins on the Rust side:
 * every key present, `target.id` and `action` present-even-when-null, and
 * every union closed.
 */
import type { CheckFinding, CheckTarget } from "../editorTypes";
import { asArray, asEnum, asNullableString, asObject, asString, fail } from "./decodePrimitives";
import { CHECK_ACTIONS, CHECK_CODES, CHECK_SEVERITIES, CHECK_TARGET_KINDS } from "./editorCheckTypes";

function decodeTarget(value: unknown, field: string): CheckTarget {
  const v = asObject(value, field);
  const kind = asEnum(v.kind, `${field}.kind`, CHECK_TARGET_KINDS);
  const id = asNullableString(v.id, `${field}.id`);
  // Only the project itself has no id; every other target names one.
  if ((kind === "project") !== (id === null)) fail(`${field}.id must be null exactly for the project`);
  return { kind, id };
}

function decodeFinding(value: unknown, field: string): CheckFinding {
  const v = asObject(value, field);
  return {
    id: asString(v.id, `${field}.id`),
    severity: asEnum(v.severity, `${field}.severity`, CHECK_SEVERITIES),
    code: asEnum(v.code, `${field}.code`, CHECK_CODES),
    message: asString(v.message, `${field}.message`),
    target: decodeTarget(v.target, `${field}.target`),
    action: v.action === null ? null : asEnum(v.action, `${field}.action`, CHECK_ACTIONS),
  };
}

/** `editor_get_checks`' reply, in Rust's order. */
export function decodeCheckFindings(value: unknown): CheckFinding[] {
  return asArray(value, "checks").map((f, i) => decodeFinding(f, `checks[${i}]`));
}
