/**
 * The tutorial editor's feedback (visual-parity Task 7; no-op audit findings
 * 1, 4, 7, 9): what makes a refusal, a revision conflict and a disabled
 * shortcut visible. Mounted ONCE, by `EditorShell`, beside the one toast
 * surface it feeds (`NotificationHost variant="editor"`).
 *
 * - **A refused edit** (and a failed save, open or close) is the store's
 *   `lastError`, which no always-mounted surface read before this — the
 *   audit counted 0 DOM changes after 124 refused edits. Each error OBJECT
 *   toasts once: a save failure is also `saveError` (the header and the
 *   status bar say "Save failed" from that) and must not toast twice. The
 *   message already lost its redaction handle where it entered the webview
 *   (`errorCopy.ts`, via the port and `toEditorError`). The store clears
 *   `lastError` on the next success.
 * - **A revision conflict** parks the edit in `conflictIntent` (never
 *   resent on its own, R20); the toast's Retry is the explicit resend
 *   (`retryConflict`). A later edit that lands clears `conflictIntent`, and
 *   the toast goes with it — a Retry that would do nothing is not offered.
 * - **A disabled shortcut** (S outside a clip, Ctrl+G with one clip) says
 *   the registry's reason, at most once every 1.5 s so a held key does not
 *   stack toasts. The keydown itself stays inert and keeps bubbling.
 *
 * No timers here: the notifications store's own TTL expiry is a plain
 * function (AGENTS.md's Pinia-timer rule), and the rate limit compares
 * clock readings.
 */
import { watch } from "vue";

import type { EditorError } from "../editorTypes";
import { useEditorProjectStore } from "../stores/editorProject";
import { useNotificationsStore } from "../stores/notifications";

const CONFLICT_MESSAGE = "Your edit wasn't applied because the project changed. Retry?";

/** At most one disabled-shortcut toast per this many milliseconds. */
const DISABLED_REASON_INTERVAL_MS = 1_500;

export function useEditorFeedback() {
  const project = useEditorProjectStore();
  const notifications = useNotificationsStore();

  const toasted = new WeakSet<EditorError>();
  watch(
    () => project.lastError,
    (error) => {
      if (!error || toasted.has(error)) return;
      toasted.add(error);
      notifications.error(error.message);
    },
  );

  let conflictToast: number | null = null;
  watch(
    () => project.conflictIntent,
    (command) => {
      if (conflictToast !== null) notifications.dismiss(conflictToast);
      conflictToast = null;
      if (!command) return;
      conflictToast = notifications.notify("warning", CONFLICT_MESSAGE, {
        action: { label: "Retry", run: () => project.retryConflict() },
      });
    },
  );

  let lastReasonAt = Number.NEGATIVE_INFINITY;
  /** Say why a shortcut did nothing — rate-limited. */
  function announceDisabled(reason: string | null): void {
    if (!reason) return;
    const now = Date.now();
    if (now - lastReasonAt < DISABLED_REASON_INTERVAL_MS) return;
    lastReasonAt = now;
    notifications.info(reason);
  }

  return { announceDisabled };
}
