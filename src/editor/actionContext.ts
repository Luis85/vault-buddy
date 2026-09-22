/**
 * The action registry's base `ActionContext` (Task 17's `project`/`snapshot`/
 * `playheadMs`/`selectedClipIds`/clipboard fields), shared by every caller
 * that resolves actions from a surface with no pointer target of its own —
 * `PreviewToolbar.vue` and `TimelineToolbar.vue` (Task 20).
 *
 * Task 17 shipped `PreviewToolbar` with this hardcoded (`playheadMs: 0,
 * selectedClipIds: []`) because `editorWorkspace` — the store that actually
 * OWNS playhead/selection — didn't land until Task 18, one task later; its
 * own module doc says so explicitly ("Until then this toolbar's context is
 * built from `editorProject` alone … Task 18+ wires the real context in
 * without this component's own logic changing"). Task 20 is that wiring:
 * both toolbars now read the real values, and this tiny pure function is the
 * one place that assembly happens so the two components can't drift on it.
 *
 * `pointerTarget` defaults to `null` (neither toolbar has one) but stays a
 * parameter rather than being dropped: a clip's own right-click/Shift+F10
 * context menu (`TimelineView.vue`) needs the exact same base fields plus a
 * real target, and building that from scratch a third time is the drift this
 * function exists to prevent.
 *
 * Clipboard wiring (`hasClipboard`/`clipboardFragment`) is NOT part of this
 * task — `fragment.ts`'s `buildFragment` exists but nothing here calls it,
 * so both fields stay the same honest "nothing to paste yet" default every
 * caller already used before real selection/playhead existed. A later task
 * that wires the clipboard replaces these two literals, not the callers.
 */
import type { EditorSnapshot, Project } from "../editorTypes";
import type { ActionContext, PointerTarget } from "./actions";

export function baseActionContext(
  project: Project | null,
  snapshot: EditorSnapshot | null,
  playheadMs: number,
  selectedClipIds: string[],
  pointerTarget: PointerTarget | null = null,
): ActionContext {
  return {
    project,
    snapshot,
    playheadMs,
    selectedClipIds,
    pointerTarget,
    hasClipboard: false,
    clipboardFragment: null,
  };
}
