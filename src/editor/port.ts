/**
 * The editor's IPC port (F31, R14) — the ONLY file in `src/` that calls
 * `invoke("editor_*", …)`. Every other editor surface (composables,
 * components) goes through `EditorPort`, never `invoke` directly, so the
 * wire shape and its decode discipline live in exactly one place; a
 * source-scan test (`tests/editorPort.test.ts`) enforces this by grepping
 * every `src/**` file for `invoke("editor_` outside this one.
 *
 * Each method's Tauri command argument names are the Rust command's own
 * parameter names, camelCased (Tauri's own IPC convention — see e.g.
 * `src/stores/screenCapture.ts`'s `source_id` -> `sourceId`), NOT the
 * struct field renames those parameters carry once inside a payload —
 * `editor_execute`'s single parameter is named `request`, so it is sent as
 * `{ request }`, not unwrapped.
 *
 * Every reply is decoded before a caller ever sees it (`decode.ts`); every
 * rejection is converted into a thrown `EditorPortError` wrapping the
 * decoded `EditorError`, so a caller can branch on `error.error.code`
 * rather than parsing a message string.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  CaptionImportResult,
  CloseDisposition,
  EditorError,
  EditorOpenResult,
  EditorProjection,
  ExecuteRequest,
  JobProgressDto,
  JobRecordDto,
  JobStarted,
  MediaRef,
  PackageFormat,
  PackageReceipt,
  ProductDto,
  ProjectSummaryDto,
  RelinkReport,
  RenderRequest,
  RenderStarted,
  SaveReceipt,
  Workspace,
} from "../editorTypes";
import { logWarning } from "../logging";
import {
  decodeCaptionImportResult,
  decodeEditorError,
  decodeJobProgress,
  decodeJobRecords,
  decodeJobStarted,
  decodeMediaPath,
  decodeMediaPeaks,
  decodeNullableOpenResult,
  decodeNullablePackageReceipt,
  decodeOpenResult,
  decodeProjection,
  decodeProjectSummaries,
  decodeSaveReceipt,
  decodeWorkspace,
  isEditorError,
} from "./decode";
import { decodeRelinkReport } from "./decodeRelink";
import { decodeProducts, decodeRenderStarted } from "./decodeRender";

/** Thrown by every `EditorPort` method on a rejected invoke — `error` is
 * the decoded `EditorError`, so a caller reads `err.error.code` rather
 * than parsing `err.message`. */
export class EditorPortError extends Error {
  readonly error: EditorError;

  constructor(error: EditorError) {
    super(error.message);
    this.name = "EditorPortError";
    this.error = error;
  }
}

/** Converts a caught invoke rejection into `EditorPortError`. A rejection
 * that is NOT shaped like an `EditorError` (a Tauri transport failure, a
 * plain thrown `Error`) is never misread as one — it is wrapped in a
 * synthetic `internal` error instead, so every `EditorPort` method still
 * only ever throws `EditorPortError`. */
function toPortError(e: unknown): EditorPortError {
  if (isEditorError(e)) {
    // `isEditorError` only checks code/message/retryable/operationId —
    // `decodeEditorError` can still throw `ProtocolError` on its own if
    // the otherwise well-shaped payload carries a malformed
    // `retainedAssetIds` (fix round 1, finding 3). That throw must not
    // escape past THIS function as a bare `ProtocolError` in place of the
    // `EditorPortError` every `EditorPort` method promises, so it falls
    // back to the four fields `isEditorError`'s own type guard already
    // proved correct, dropping only the field that failed to decode.
    try {
      return new EditorPortError(decodeEditorError(e));
    } catch {
      const { code, message, retryable, operationId } = e;
      return new EditorPortError({ code, message, retryable, operationId });
    }
  }
  const message = e instanceof Error ? e.message : String(e);
  return new EditorPortError({
    code: "internal",
    message,
    retryable: false,
    operationId: "port-local",
  });
}

async function call<T>(cmd: string, args: Record<string, unknown> | undefined, decode: (v: unknown) => T): Promise<T> {
  try {
    return decode(await invoke<unknown>(cmd, args));
  } catch (e) {
    throw toPortError(e);
  }
}

/** The editor's IPC surface, one method per registered `editor_*` command
 * (`src-tauri/src/lib.rs`'s `generate_handler!`). */
export interface EditorPort {
  openStaged(base: string): Promise<EditorOpenResult>;
  openProject(id: string, useRecovery: boolean): Promise<EditorOpenResult>;
  listProjects(): Promise<ProjectSummaryDto[]>;
  getSnapshot(sessionId: string, knownRevision: number | null): Promise<EditorProjection>;
  execute(req: ExecuteRequest): Promise<EditorProjection>;
  save(sessionId: string, expectedRevision: number): Promise<SaveReceipt>;
  closeSession(sessionId: string, disposition: CloseDisposition): Promise<void>;
  hideWindow(): Promise<void>;
  /** `editor_get_workspace` — reads (and sanitizes) `workspace.json`;
   * missing/malformed degrades to an object with every field absent. */
  getWorkspace(sessionId: string): Promise<Workspace>;
  /** `editor_save_workspace` — sanitizes and writes `workspace.json`.
   * Never touches the session's revision or its undo/redo history. */
  saveWorkspace(sessionId: string, workspace: Workspace): Promise<void>;
  /** `editor_media_url` — the absolute path of a REGISTERED asset or
   * product (`unauthorizedSource` otherwise, `sourceMissing` when its file
   * is gone), for `convertFileSrc`. The frontend never builds a path. */
  mediaUrl(sessionId: string, ref: MediaRef): Promise<string>;
  /** `editor_media_peaks` — a registered asset's waveform, `buckets`
   * full-scale max-abs values in `0..=1` over its whole source. Rust
   * decodes it with ffmpeg on a miss (a cancelable `peaks` job) and caches
   * it; `encoderUnavailable` when ffmpeg is missing and nothing is cached. */
  mediaPeaks(sessionId: string, assetId: string, buckets: number): Promise<number[]>;
  /** `editor_media_thumbnail` — the absolute path of a 160 px wide JPEG of
   * a registered asset near `atMs` (Rust quantizes to 250 ms), inside the
   * project's `cache\`, for `convertFileSrc`. */
  mediaThumbnail(sessionId: string, assetId: string, atMs: number): Promise<string>;
  /** `editor_import_media` — Rust opens its OWN native multi-file dialog
   * (no path ever leaves this webview) and answers `{ jobId }` at once;
   * every progress message, decoded, reaches `onProgress` through a
   * per-job `Channel`, never an app-wide event. A message that fails to
   * decode is logged and dropped — the store's `reconcile` (the job
   * registry) recovers whatever it would have said. Messages can arrive
   * BEFORE this promise resolves; the caller must be ready for that. */
  importMedia(sessionId: string, onProgress: (message: JobProgressDto) => void): Promise<JobStarted>;
  /** `editor_cancel_job` — stops a job's FUTURE work; finished work stays. */
  cancelJob(sessionId: string, jobId: string): Promise<void>;
  /** `editor_get_jobs` — the authoritative job states, oldest first. */
  getJobs(sessionId: string): Promise<JobRecordDto[]>;
  /** `editor_import_captions` — Rust opens its OWN native dialog (SRT,
   * WebVTT, `.txt`), reads the file bounded and imports it onto `clipId`
   * as ONE edit; `replace` drops that clip's existing cues in the same
   * step. `null` when the dialog was cancelled. */
  importCaptions(sessionId: string, clipId: string, replace: boolean): Promise<CaptionImportResult | null>;
  /** `editor_export_package` — Rust freezes `expectedRevision`, opens its
   * OWN save dialog and writes the file; `null` when the dialog was
   * dismissed. The receipt is the only proof the file exists. */
  exportPackage(sessionId: string, expectedRevision: number, format: PackageFormat): Promise<PackageReceipt | null>;
  /** `editor_import_package` — Rust opens its OWN open dialog, validates
   * the whole file and installs it as a project (a copy when its id is
   * taken); `null` when the dialog was dismissed. */
  importPackage(): Promise<EditorOpenResult | null>;
  /** `editor_relink_media` (Task 40) — Rust opens its OWN open dialog (one
   * file for one asset, many for a batch), matches each picked file to a
   * missing original by hash, or by size + length + kind, and reconnects
   * only unique matches; `confirmReplace` (one asset) accepts a file that
   * is not the original. `null` when the dialog was dismissed. */
  relinkMedia(sessionId: string, assetIds: string[], confirmReplace: boolean): Promise<RelinkReport | null>;
  /** `editor_start_render` (Task 46) — Rust freezes `expectedRevision`
   * (`revisionConflict` for a pending edit), refuses what it cannot render
   * and answers `{ jobId, revision }` at once; every decoded progress
   * message reaches `onProgress` on the job's own Channel, the terminal
   * one carrying `productId`. One render per session. */
  startRender(request: RenderRequest, onProgress: (message: JobProgressDto) => void): Promise<RenderStarted>;
  /** `editor_get_products` — the project's product ledger. */
  getProducts(sessionId: string): Promise<ProductDto[]>;
  /** `editor_restore_product` — the product's frozen edit becomes the live
   * one: a new revision, one undo step; the product is never touched. */
  restoreProduct(
    sessionId: string,
    expectedRevision: number,
    productId: string,
    commandId: string,
  ): Promise<EditorProjection>;
}

/** The per-job Channel (Tauri's ordered, subscriber-scoped delivery) —
 * created here so no other file ever touches the IPC transport. */
function progressChannel(onProgress: (message: JobProgressDto) => void): Channel<unknown> {
  const channel = new Channel<unknown>();
  channel.onmessage = (raw) => {
    let message: JobProgressDto;
    try {
      message = decodeJobProgress(raw);
    } catch (e) {
      logWarning(`editor job progress dropped (undecodable): ${String(e)}`);
      return;
    }
    onProgress(message);
  };
  return channel;
}

export function createTauriEditorPort(): EditorPort {
  return {
    openStaged(base) {
      return call("editor_open_staged", { stagedBase: base }, decodeOpenResult);
    },
    openProject(id, useRecovery) {
      return call("editor_open_project", { projectFileId: id, useRecovery }, decodeOpenResult);
    },
    listProjects() {
      return call("editor_list_projects", undefined, decodeProjectSummaries);
    },
    getSnapshot(sessionId, knownRevision) {
      return call(
        "editor_get_snapshot",
        { sessionId, knownRevision: knownRevision ?? null },
        decodeProjection,
      );
    },
    execute(req) {
      return call("editor_execute", { request: req }, decodeProjection);
    },
    save(sessionId, expectedRevision) {
      return call("editor_save_project", { sessionId, expectedRevision }, decodeSaveReceipt);
    },
    async closeSession(sessionId, disposition) {
      await call("editor_close_session", { sessionId, disposition }, () => undefined);
    },
    async hideWindow() {
      await call("editor_hide_window", undefined, () => undefined);
    },
    getWorkspace(sessionId) {
      return call("editor_get_workspace", { sessionId }, decodeWorkspace);
    },
    async saveWorkspace(sessionId, workspace) {
      await call("editor_save_workspace", { sessionId, workspace }, () => undefined);
    },
    mediaUrl(sessionId, ref) {
      return call("editor_media_url", { sessionId, ref }, decodeMediaPath);
    },
    mediaPeaks(sessionId, assetId, buckets) {
      return call("editor_media_peaks", { sessionId, assetId, buckets }, decodeMediaPeaks);
    },
    mediaThumbnail(sessionId, assetId, atMs) {
      return call("editor_media_thumbnail", { sessionId, assetId, atMs }, decodeMediaPath);
    },
    importMedia(sessionId, onProgress) {
      const onProgressChannel = progressChannel(onProgress);
      return call(
        "editor_import_media",
        { sessionId, onProgress: onProgressChannel },
        decodeJobStarted,
      );
    },
    async cancelJob(sessionId, jobId) {
      await call("editor_cancel_job", { sessionId, jobId }, () => undefined);
    },
    getJobs(sessionId) {
      return call("editor_get_jobs", { sessionId }, decodeJobRecords);
    },
    importCaptions(sessionId, clipId, replace) {
      return call("editor_import_captions", { sessionId, clipId, replace }, decodeCaptionImportResult);
    },
    exportPackage(sessionId, expectedRevision, format) {
      return call(
        "editor_export_package",
        { sessionId, expectedRevision, format },
        decodeNullablePackageReceipt,
      );
    },
    importPackage() {
      return call("editor_import_package", undefined, decodeNullableOpenResult);
    },
    relinkMedia(sessionId, assetIds, confirmReplace) {
      return call("editor_relink_media", { sessionId, assetIds, confirmReplace }, decodeRelinkReport);
    },
    startRender(request, onProgress) {
      const onProgressChannel = progressChannel(onProgress);
      return call("editor_start_render", { request, onProgress: onProgressChannel }, decodeRenderStarted);
    },
    getProducts(sessionId) {
      return call("editor_get_products", { sessionId }, decodeProducts);
    },
    restoreProduct(sessionId, expectedRevision, productId, commandId) {
      return call(
        "editor_restore_product",
        { sessionId, expectedRevision, productId, commandId },
        decodeProjection,
      );
    },
  };
}
