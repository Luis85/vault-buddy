/**
 * Runtime decoders for every editor IPC reply (F31/R14). `port.ts` is the
 * only file that calls `invoke("editor_*", …)`; every response it gets back
 * comes through one of these before a caller ever sees it, so a malformed
 * or version-skewed reply throws a `ProtocolError` instead of quietly
 * becoming `undefined.something` three components later.
 *
 * The starter's decoders (`docs/concepts/vault-buddy-editor/
 * implementation-starter/src/contracts.ts`) are the model: read a raw
 * `unknown`, check its shape field by field, and never trust a cast. This
 * module widens that pattern to the FULL Contract reference envelope set
 * this task owns, plus the project graph itself (`decodeProject`, split
 * into its own file below the 500-line cap — see `decodeProject.ts`).
 *
 * Every `*Ms` field is bounded at `MAX_DURATION_MS` (7,200,000 —
 * `core::editor::limits::MAX_DURATION_MS`, the two-hour reference safety
 * bound) and every integer field is required to be a `Number.
 * isSafeInteger` — a `2**53` from a hostile or corrupted reply is not a
 * number this app's arithmetic can trust.
 */
import type {
  EditorError,
  EditorErrorCode,
  EditorOpenResult,
  EditorProjection,
  EditorSnapshot,
  JobPhase,
  JobProgressDto,
  JobTerminal,
  MissingMedia,
  ProjectSummaryDto,
  SaveReceipt,
  Workspace,
} from "../editorTypes";
import {
  asArray,
  asBoolean,
  asId,
  asInteger,
  asMs,
  asNullableString,
  asObject,
  asString,
  fail,
  ProtocolError,
} from "./decodePrimitives";
import { decodeProject } from "./decodeProject";

export { decodeProject,ProtocolError };

const ERROR_CODES: readonly EditorErrorCode[] = [
  "invalidRequest",
  "invalidProject",
  "revisionConflict",
  "sessionGone",
  "unauthorizedSource",
  "sourceMissing",
  "unsupportedMedia",
  "deviceUnavailable",
  "permissionDenied",
  "diskFull",
  "writeDenied",
  "destinationUnavailable",
  "encoderUnavailable",
  "cancelled",
  "internal",
];

const JOB_PHASES: readonly JobPhase[] = [
  "queued",
  "preparing",
  "rendering",
  "publishing",
  "complete",
  "cancelled",
  "failed",
];

function asEditorErrorCode(value: unknown, field: string): EditorErrorCode {
  const s = asString(value, field);
  if (!(ERROR_CODES as readonly string[]).includes(s)) {
    fail(`${field} is not a recognized error code: ${s}`);
  }
  return s as EditorErrorCode;
}

/** `core::editor::error::EditorError`. `retainedAssetIds` is present only
 * when an operation left assets behind — omitted, never `null`. */
export function decodeEditorError(value: unknown): EditorError {
  const v = asObject(value, "error");
  const retainedAssetIds = v.retainedAssetIds;
  return {
    code: asEditorErrorCode(v.code, "error.code"),
    message: asString(v.message, "error.message"),
    retryable: asBoolean(v.retryable, "error.retryable"),
    operationId: asString(v.operationId, "error.operationId"),
    ...(retainedAssetIds === undefined
      ? {}
      : {
          retainedAssetIds: asArray(retainedAssetIds, "error.retainedAssetIds").map((id, i) =>
            asString(id, `error.retainedAssetIds[${i}]`),
          ),
        }),
  };
}

/** True when `value` is shaped like a rejected `editor_*` invoke's payload
 * — `port.ts` uses this to decide whether a caught rejection is the
 * editor's own typed error (and worth `decodeEditorError`-ing) or some
 * other failure (a Tauri transport error, a thrown `Error`) that must not
 * be misread as one. */
export function isEditorError(e: unknown): e is EditorError {
  if (!e || typeof e !== "object" || Array.isArray(e)) return false;
  const v = e as Record<string, unknown>;
  return (
    typeof v.code === "string" &&
    (ERROR_CODES as readonly string[]).includes(v.code) &&
    typeof v.message === "string" &&
    typeof v.retryable === "boolean" &&
    typeof v.operationId === "string"
  );
}

/** `core::editor::session::EditorSnapshot`. */
export function decodeSnapshot(value: unknown): EditorSnapshot {
  const v = asObject(value, "snapshot");
  const revision = asInteger(v.revision, "snapshot.revision");
  const persistedRevision =
    v.persistedRevision === null ? null : asInteger(v.persistedRevision, "snapshot.persistedRevision");
  // MUTATION CHECK (this task's brief): dropping this comparison lets a
  // save receipt that raced ahead of the session's own revision through
  // undetected — the frontend would then treat an edit as already saved.
  if (persistedRevision !== null && persistedRevision > revision) {
    fail("snapshot.persistedRevision must not exceed snapshot.revision");
  }
  return {
    sessionId: asId(v.sessionId, "snapshot.sessionId"),
    projectId: asId(v.projectId, "snapshot.projectId"),
    revision,
    persistedRevision,
    title: asString(v.title, "snapshot.title"),
    durationMs: asMs(v.durationMs, "snapshot.durationMs"),
    canUndo: asBoolean(v.canUndo, "snapshot.canUndo"),
    canRedo: asBoolean(v.canRedo, "snapshot.canRedo"),
    undoLabel: asNullableString(v.undoLabel, "snapshot.undoLabel"),
    redoLabel: asNullableString(v.redoLabel, "snapshot.redoLabel"),
  };
}

/** `core::editor::projection::EditorProjection` — `editor_execute`/
 * `editor_get_snapshot`'s reply. */
export function decodeProjection(value: unknown): EditorProjection {
  const v = asObject(value, "projection");
  return {
    snapshot: decodeSnapshot(v.snapshot),
    project: decodeProject(v.project),
  };
}

function decodeWorkspace(value: unknown): Workspace {
  // Rust already sanitizes this blob field by field on the way out
  // (`core::editor::workspace::sanitize`) — every key is optional and a
  // wrong-typed one is simply absent, so the frontend does not re-validate
  // each of the 18 fields, only that the envelope itself is an object.
  return asObject(value, "workspace") as unknown as Workspace;
}

function decodeMissingMedia(value: unknown, index: number): MissingMedia {
  const v = asObject(value, `missing[${index}]`);
  return {
    assetId: asId(v.assetId, `missing[${index}].assetId`),
    name: asString(v.name, `missing[${index}].name`),
    expectedSize: asInteger(v.expectedSize, `missing[${index}].expectedSize`),
    expectedDurationMs: asMs(v.expectedDurationMs, `missing[${index}].expectedDurationMs`),
  };
}

/** `core::editor::projection::EditorOpenResult` — `editor_open_staged`/
 * `editor_open_project`'s reply. */
export function decodeOpenResult(value: unknown): EditorOpenResult {
  const v = asObject(value, "openResult");
  const missingRaw = v.missing;
  if (!Array.isArray(missingRaw)) fail("openResult.missing must be an array");
  return {
    snapshot: decodeSnapshot(v.snapshot),
    project: decodeProject(v.project),
    workspace: decodeWorkspace(v.workspace),
    missing: missingRaw.map((m, i) => decodeMissingMedia(m, i)),
    sourceBase: asNullableString(v.sourceBase, "openResult.sourceBase"),
    recovered: asBoolean(v.recovered, "openResult.recovered"),
  };
}

/** `save_commands::SaveReceipt` — `editor_save_project`'s reply. */
export function decodeSaveReceipt(value: unknown): SaveReceipt {
  const v = asObject(value, "saveReceipt");
  return {
    sessionId: asId(v.sessionId, "saveReceipt.sessionId"),
    savedRevision: asInteger(v.savedRevision, "saveReceipt.savedRevision"),
    projectFileId: asId(v.projectFileId, "saveReceipt.projectFileId"),
  };
}

function decodeProjectSummary(value: unknown, index: number): ProjectSummaryDto {
  const v = asObject(value, `projects[${index}]`);
  return {
    projectFileId: asId(v.projectFileId, `projects[${index}].projectFileId`),
    title: asString(v.title, `projects[${index}].title`),
    updatedAt: asString(v.updatedAt, `projects[${index}].updatedAt`),
    persistedRevision: asInteger(v.persistedRevision, `projects[${index}].persistedRevision`),
    hasRecovery: asBoolean(v.hasRecovery, `projects[${index}].hasRecovery`),
    sourceBase: asNullableString(v.sourceBase, `projects[${index}].sourceBase`),
  };
}

/** `store_io::ProjectSummaryDto[]` — `editor_list_projects`'s reply. */
export function decodeProjectSummaries(value: unknown): ProjectSummaryDto[] {
  if (!Array.isArray(value)) fail("projects must be an array");
  return value.map((p, i) => decodeProjectSummary(p, i));
}

function decodeJobTerminal(value: unknown): JobTerminal {
  const v = asObject(value, "jobProgress.terminal");
  const perFileRaw = v.perFile;
  return {
    ...(v.productId === undefined ? {} : { productId: asString(v.productId, "terminal.productId") }),
    ...(v.assetIds === undefined
      ? {}
      : {
          assetIds: asArray(v.assetIds, "terminal.assetIds").map((id, i) =>
            asString(id, `terminal.assetIds[${i}]`),
          ),
        }),
    ...(perFileRaw === undefined
      ? {}
      : {
          perFile: asArray(perFileRaw, "terminal.perFile").map((row, i) => {
            const r = asObject(row, `terminal.perFile[${i}]`);
            return {
              name: asString(r.name, `terminal.perFile[${i}].name`),
              error: asString(r.error, `terminal.perFile[${i}].error`),
            };
          }),
        }),
    ...(v.error === undefined ? {} : { error: decodeEditorError(v.error) }),
  };
}

/** Contract reference `JobProgressDto`. No Rust command emits this yet
 * (declared ahead of that later task per this task's own brief), so this
 * decoder is checked against the Contract's own shape, not a Rust literal
 * pin. */
export function decodeJobProgress(value: unknown): JobProgressDto {
  const v = asObject(value, "jobProgress");
  const phase = asString(v.phase, "jobProgress.phase");
  if (!(JOB_PHASES as readonly string[]).includes(phase)) {
    fail(`jobProgress.phase is not a recognized phase: ${phase}`);
  }
  const fraction = v.fraction;
  if (typeof fraction !== "number" || !Number.isFinite(fraction) || fraction < 0 || fraction > 1) {
    fail("jobProgress.fraction must be a number in [0, 1]");
  }
  const kind = asString(v.kind, "jobProgress.kind");
  if (!["import", "render", "peaks", "publish"].includes(kind)) {
    fail(`jobProgress.kind is not a recognized kind: ${kind}`);
  }
  return {
    sessionId: asId(v.sessionId, "jobProgress.sessionId"),
    jobId: asId(v.jobId, "jobProgress.jobId"),
    kind: kind as JobProgressDto["kind"],
    sequence: asInteger(v.sequence, "jobProgress.sequence"),
    phase: phase as JobPhase,
    fraction,
    // Present-even-when-null, the `EditorOpenResult.sourceBase` posture:
    // an absent key is a decoding error, not "no terminal outcome yet".
    terminal: v.terminal === null ? null : decodeJobTerminal(v.terminal),
  };
}
