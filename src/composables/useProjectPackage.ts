/**
 * Portable and lightweight project files (Task 39; F-40; SCREENS 08): the
 * two round trips behind the header's Save project menu. Both dialogs are
 * Rust's own — no path ever crosses this webview — and both go through
 * `editorProject`'s injected port (`invoke` lives only in `port.ts`).
 *
 * `useProjectExport` is the Save dialog's state machine: idle → pending →
 * success | failure | cancelled. SCREENS 08: "Native copy changes only
 * after a matching durable receipt" — success needs a receipt, and one for
 * THIS request (its session, the revision it froze, the format asked for);
 * anything else is reported, never shown as saved. Its error state is its
 * own, never the store's shared `lastError`.
 */
import { ref } from "vue";

import type { EditorError, PackageFormat, PackageReceipt } from "../editorTypes";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";

type ExportState =
  | { phase: "idle" }
  | { phase: "pending" }
  | { phase: "success"; fileName: string }
  | { phase: "failure"; message: string }
  | { phase: "cancelled" };

interface ExportRequest {
  sessionId: string;
  revision: number;
  format: PackageFormat;
}

function settle(receipt: PackageReceipt | null, request: ExportRequest): ExportState {
  if (receipt === null) return { phase: "cancelled" };
  const matches =
    receipt.sessionId === request.sessionId &&
    receipt.savedRevision === request.revision &&
    receipt.format === request.format;
  if (matches) return { phase: "success", fileName: receipt.fileName };
  return { phase: "failure", message: "The reply did not match this project, so the save is not confirmed." };
}

export function useProjectExport() {
  const project = useEditorProjectStore();
  const state = ref<ExportState>({ phase: "idle" });

  async function run(format: PackageFormat): Promise<void> {
    if (state.value.phase === "pending") return;
    const sessionId = project.sessionId;
    const revision = project.snapshot?.revision;
    if (!sessionId || revision === undefined) {
      state.value = { phase: "failure", message: "No project is open." };
      return;
    }
    state.value = { phase: "pending" };
    try {
      const receipt = await project.port.exportPackage(sessionId, revision, format);
      state.value = settle(receipt, { sessionId, revision, format });
    } catch (e) {
      state.value = { phase: "failure", message: toEditorError(e).message };
    }
  }

  /** Back to idle — never while a request is outstanding, whose reply
   * would otherwise land on a dialog that no longer expects it. */
  function reset(): void {
    if (state.value.phase !== "pending") state.value = { phase: "idle" };
  }

  return { state, run, reset };
}

/**
 * `editor_import_package`: Rust opens its own dialog, installs the file as
 * a project and answers with the opened session. Nothing here changes until
 * that reply lands — a dismissed dialog (`"cancelled"`) or a refusal (the
 * returned error) leaves the current session exactly as it was. On success
 * `beforeInstall` runs in the SAME tick as the store's install, so a caller
 * whose own gate names the open project (`EditorRoot`) never renders a frame
 * of a session it has not been told about.
 */
export async function importProjectPackage(
  beforeInstall: (projectId: string) => void,
): Promise<"opened" | "cancelled" | EditorError> {
  const project = useEditorProjectStore();
  let result;
  try {
    result = await project.port.importPackage();
  } catch (e) {
    return toEditorError(e);
  }
  if (!result) return "cancelled";
  beforeInstall(result.snapshot.projectId);
  project.beginOpen();
  project.install(result);
  return "opened";
}
