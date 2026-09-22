/**
 * The tutorial editor's clipboard (Task 21; F-12). `actionContext.ts`'s own
 * module doc named this exactly: "Clipboard wiring is NOT part of this task
 * … A later task that wires the clipboard replaces these two literals, not
 * the callers." This is that later task, and the promise is kept literally —
 * `baseActionContext` now reads `clipboardFragment` below instead of the
 * hardcoded `false`/`null`, and every existing caller (`PreviewToolbar.vue`,
 * `TimelineToolbar.vue`, `TimelineView.vue`'s context menu) needed no change
 * at all.
 *
 * Deliberately WINDOW-LOCAL, in-memory, per-webview state: a plain
 * module-scoped `ref`, never sent to Rust and never persisted through
 * `editorWorkspace`'s `workspace.json` (`core::editor::workspace::Workspace`
 * has no clipboard field, and it should not grow one — a fragment naming
 * clips from a PREVIOUS session would be nonsense the instant the session
 * changes, so surviving a reopen is not a feature here). A plain ref rather
 * than a Pinia store: nothing here needs actions/getters beyond "read" and
 * "replace", and `baseActionContext` is a plain, store-free function that
 * has to read this synchronously.
 */
import { ref } from "vue";

import type { ClipboardFragment } from "../editorTypes";
import type { ActionId } from "./actionMeta";
import type { ActionContext } from "./actions";
import { commandFor, resolveActions, targetClipIds } from "./actions";
import type { EditorCommand } from "./editorCommandTypes";
import { buildFragment } from "./fragment";

/** The current clipboard contents, or `null` when nothing has been copied
 * yet this session. Read directly by `actionContext.ts`. */
export const clipboardFragment = ref<ClipboardFragment | null>(null);

export function setClipboard(fragment: ClipboardFragment): void {
  clipboardFragment.value = fragment;
}

/** Test-only reset — production code has no reason to ever clear the
 * clipboard back to empty (there is no "empty clipboard" verb in the
 * registry, only "replace it with a new copy"). */
export function clearClipboardForTest(): void {
  clipboardFragment.value = null;
}

/**
 * Activate one action from ANY surface that reads the shared registry (the
 * keyboard dispatcher in `EditorShell.vue`, the context menu in
 * `TimelineView.vue`) — the single place "copy"/"cut" and every real wire
 * command converge, so the two surfaces can never diverge on what Copy/Cut
 * actually do. Mirrors `actions.ts`'s own precondition-trusting discipline
 * (see `commandFor`'s own doc): called only after confirming
 * `resolveActions(ctx)[actionId]` is enabled, which this function checks
 * itself so neither caller has to repeat the guard.
 *
 * "copy" has no wire command — `commandFor` returns `null` for it by design
 * (`actions.ts`'s own doc: "commandFor('copy', ctx) always returns null; a
 * caller reads resolveActions(ctx).copy.enabled and builds the fragment
 * itself") — this is the one place that is finally done, by writing the
 * fragment into the window-local clipboard above instead of sending
 * anything to Rust.
 *
 * "cut" does BOTH: it copies the exact same way, THEN sends `cutClips` —
 * the one command that actually removes the clips — as a SEPARATE step from
 * the copy. The copy is local and uncommitted; the delete is the single
 * `editor_execute` call `execute` makes, so a cut is exactly one commit to
 * Rust (one undo step) even though it also touched the clipboard.
 *
 * Returns whether it actually DID something (wrote the clipboard or sent a
 * command). An enabled action with no wire command here (`save`, `help`,
 * `focusPreview` — other surfaces own those) returns `false`, so the
 * keyboard dispatcher can leave that keystroke to bubble instead of
 * swallowing it with nothing done.
 */
export function activateEditorAction(
  actionId: ActionId,
  ctx: ActionContext,
  execute: (command: EditorCommand) => void | Promise<void>,
): boolean {
  if (!resolveActions(ctx)[actionId].enabled) return false;
  let acted = false;
  if ((actionId === "copy" || actionId === "cut") && ctx.project) {
    const ids = targetClipIds(ctx);
    if (ids.length > 0) {
      setClipboard(buildFragment(ctx.project, ids));
      acted = true;
    }
  }
  const command = commandFor(actionId, ctx);
  if (command) void execute(command);
  return acted || command !== null;
}
