import { logWarning } from "../logging";

/** The one clipboard write behind every copy affordance (MCP snippets/token,
 * the task editor's copy-id). Optional-chained so an unavailable Clipboard
 * API degrades to the log line instead of a TypeError, and fire-and-forget:
 * copy buttons have no failure UI by design, so the log is the only trace a
 * silently-dead button leaves — `context` names which one. */
export function copyToClipboard(text: string, context: string): void {
  void navigator.clipboard
    ?.writeText(text)
    .catch((e) => logWarning(`${context}: clipboard copy failed: ${String(e)}`));
}
