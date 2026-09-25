/**
 * The decoder for `editor_relink_media`'s reply (Task 40) — its own file so
 * `decode.ts` does not grow toward the 500-line cap. Every list must be
 * present (Rust always sends them); a missing or wrong-typed one throws
 * `ProtocolError` rather than reading as "nothing happened".
 */
import type { RelinkReport } from "../editorTypes";
import { decodeMissingMedia, decodeProjection } from "./decode";
import { asArray, asId, asObject, asString } from "./decodePrimitives";

function files(value: unknown, field: string): { assetId: string; file: string }[] {
  return asArray(value, field).map((raw, i) => {
    const v = asObject(raw, `${field}[${i}]`);
    return { assetId: asId(v.assetId, `${field}[${i}].assetId`), file: asString(v.file, `${field}[${i}].file`) };
  });
}

/** `null` for a dismissed dialog, else the report. */
export function decodeRelinkReport(value: unknown): RelinkReport | null {
  if (value === null) return null;
  const v = asObject(value, "relink");
  return {
    projection: decodeProjection(v.projection),
    missing: asArray(v.missing, "relink.missing").map((m, i) => decodeMissingMedia(m, i)),
    matched: files(v.matched, "relink.matched"),
    replaced: files(v.replaced, "relink.replaced"),
    ambiguous: asArray(v.ambiguous, "relink.ambiguous").map((raw, i) => {
      const a = asObject(raw, `relink.ambiguous[${i}]`);
      return {
        assetId: asId(a.assetId, `relink.ambiguous[${i}].assetId`),
        files: asArray(a.files, `relink.ambiguous[${i}].files`).map((f, j) =>
          asString(f, `relink.ambiguous[${i}].files[${j}]`),
        ),
      };
    }),
    unmatched: asArray(v.unmatched, "relink.unmatched").map((id, i) => asId(id, `relink.unmatched[${i}]`)),
    mismatched: asArray(v.mismatched, "relink.mismatched").map((raw, i) => {
      const m = asObject(raw, `relink.mismatched[${i}]`);
      const at = `relink.mismatched[${i}]`;
      return { assetId: asId(m.assetId, `${at}.assetId`), file: asString(m.file, `${at}.file`), reason: asString(m.reason, `${at}.reason`) };
    }),
    failed: asArray(v.failed, "relink.failed").map((raw, i) => {
      const f = asObject(raw, `relink.failed[${i}]`);
      const at = `relink.failed[${i}]`;
      return { assetId: asId(f.assetId, `${at}.assetId`), file: asString(f.file, `${at}.file`), error: asString(f.error, `${at}.error`) };
    }),
    unused: asArray(v.unused, "relink.unused").map((n, i) => asString(n, `relink.unused[${i}]`)),
    excluded: asArray(v.excluded, "relink.excluded").map((raw, i) => {
      const e = asObject(raw, `relink.excluded[${i}]`);
      return { assetId: asId(e.assetId, `relink.excluded[${i}].assetId`), reason: asString(e.reason, `relink.excluded[${i}].reason`) };
    }),
    perFile: asArray(v.perFile, "relink.perFile").map((raw, i) => {
      const p = asObject(raw, `relink.perFile[${i}]`);
      return { name: asString(p.name, `relink.perFile[${i}].name`), error: asString(p.error, `relink.perFile[${i}].error`) };
    }),
  };
}
