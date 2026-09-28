/**
 * The user-facing wording of an editor error's message (Task 59, carried
 * from Task 58).
 *
 * Rust never puts a path into an `EditorError`: a file is named by its ROLE
 * ("the project folder", "the recovery journal") plus a `redact_path` /
 * `redact_name` handle — `<path:#1a2b3c4d>` / `<name:#1a2b3c4d>`, the first
 * 8 hex digits of a SHA-256 (`src-tauri/src/editor/redact.rs`). The handle
 * lets a support reader match two LOG lines about the same file; on screen
 * it is noise that reads like a bug. So every message a surface can show
 * keeps its role wording and loses the handle — once, at the three places
 * an `EditorError` enters the webview (`decodeEditorError`, the port's
 * fallback, the store's `toEditorError`), never per surface.
 */

/** A handle, with the one space that separates it from the word before. */
const HANDLE = /\s?<(?:path|name):#[0-9a-f]{8}>/g;

/** `message` without its redaction handles, whitespace tidied. */
export function withoutRedactionHandles(message: string): string {
  if (!message.includes(":#")) return message;
  return message.replace(HANDLE, "").replace(/\s{2,}/g, " ").trim();
}
