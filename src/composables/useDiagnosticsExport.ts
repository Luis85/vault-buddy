/**
 * Export diagnostics (Task 58; F-50): Rust's own save dialog writes counts,
 * capabilities and error codes — never project content — to a new file.
 * Help → Export diagnostics and the Checks dialog's footer (visual-parity
 * Task 21, concept spec §9.4) share this one path, so the two can never
 * say it differently. A dismissed dialog says nothing; a refusal says why.
 */
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";
import { useNotificationsStore } from "../stores/notifications";

export function useDiagnosticsExport() {
  const editorProject = useEditorProjectStore();
  const notifications = useNotificationsStore();

  async function exportDiagnostics(): Promise<void> {
    try {
      const name = await editorProject.port.exportDiagnostics();
      if (name) {
        notifications.success(`Saved diagnostics to ${name}. It holds counts and error codes, never project content.`);
      }
    } catch (e) {
      notifications.error(`The diagnostics could not be saved. ${toEditorError(e).message}`);
    }
  }

  return { exportDiagnostics };
}
