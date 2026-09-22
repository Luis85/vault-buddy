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
import { invoke } from "@tauri-apps/api/core";

import type {
  CloseDisposition,
  EditorError,
  EditorOpenResult,
  EditorProjection,
  ExecuteRequest,
  ProjectSummaryDto,
  SaveReceipt,
} from "../editorTypes";
import {
  decodeEditorError,
  decodeOpenResult,
  decodeProjection,
  decodeProjectSummaries,
  decodeSaveReceipt,
  isEditorError,
} from "./decode";

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
    return new EditorPortError(decodeEditorError(e));
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
  };
}
