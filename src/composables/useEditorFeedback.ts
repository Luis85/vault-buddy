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
 *   (`errorCopy.ts`, via the port and `toEditorError`).
 *   An error toast is sticky (the store gives errors no TTL), so it lives
 *   exactly as long as its error does: once neither `lastError` nor
 *   `saveError` holds that object — the next edit or save succeeded, a
 *   newer error replaced it, the session closed — the toast is dismissed.
 *   Without that a refusal the person already moved past stayed over the
 *   timeline, and a "disk is full" outlived the save that fixed it.
 * - **A revision conflict** parks the edit in `conflictIntent` (never
 *   resent on its own, R20); the toast's Retry is the explicit resend
 *   (`retryConflict`). A later edit that lands clears `conflictIntent`, and
 *   the toast goes with it — a Retry that would do nothing is not offered.
 * - **A disabled shortcut** (S outside a clip, Ctrl+G with one clip) says
 *   the registry's reason, at most once every 1.5 s PER REASON so a held
 *   key does not stack toasts while a different refusal still speaks. The
 *   keydown itself stays inert and keeps bubbling.
 *
 * Everything this raised is dismissed when the shell unmounts (the session
 * is gone with it).
 *
 * No timers here: the notifications store's own TTL expiry is a plain
 * function (AGENTS.md's Pinia-timer rule), and the rate limit compares
 * clock readings.
 */
import { onScopeDispose, watch } from "vue";

import type { EditorError } from "../editorTypes";
import { useEditorProjectStore } from "../stores/editorProject";
import { useNotificationsStore } from "../stores/notifications";

const CONFLICT_MESSAGE = "Your edit wasn't applied because the project changed. Retry?";

/** At most one disabled-shortcut toast per this many milliseconds. */
const DISABLED_REASON_INTERVAL_MS = 1_500;

export function useEditorFeedback() {
  const project = useEditorProjectStore();
  const notifications = useNotificationsStore();

  /** Live error toasts, each with the error object it reports. */
  let errorToasts: { error: EditorError; id: number }[] = [];
  const toasted = new WeakSet<EditorError>();

  /** Dismiss every error toast whose error the store no longer holds —
   * unless a live error shares its (deduped) toast. */
  function dropStaleErrorToasts(): void {
    const live = (e: EditorError) => e === project.lastError || e === project.saveError;
    const kept = errorToasts.filter((t) => live(t.error));
    const keptIds = new Set(kept.map((t) => t.id));
    for (const t of errorToasts) if (!keptIds.has(t.id)) notifications.dismiss(t.id);
    errorToasts = kept;
  }

  watch(
    () => [project.lastError, project.saveError] as const,
    ([error]) => {
      if (error && !toasted.has(error)) {
        toasted.add(error);
        errorToasts.push({ error, id: notifications.error(error.message) });
      }
      dropStaleErrorToasts();
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

  onScopeDispose(() => {
    for (const t of errorToasts) notifications.dismiss(t.id);
    errorToasts = [];
    if (conflictToast !== null) notifications.dismiss(conflictToast);
    conflictToast = null;
  });

  /** When each reason was last said. */
  const lastSaidAt = new Map<string, number>();
  /** Say why a shortcut did nothing — each reason rate-limited on its own. */
  function announceDisabled(reason: string | null): void {
    if (!reason) return;
    const now = Date.now();
    if (now - (lastSaidAt.get(reason) ?? Number.NEGATIVE_INFINITY) < DISABLED_REASON_INTERVAL_MS) return;
    lastSaidAt.set(reason, now);
    notifications.info(reason);
  }

  return { announceDisabled };
}
