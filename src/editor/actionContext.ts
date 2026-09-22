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
 * Clipboard wiring (`hasClipboard`/`clipboardFragment`): Task 21 (`./clipboard.ts`)
 * is the "later task" this doc used to point at — both fields now read the
 * real window-local clipboard ref instead of the honest "nothing to paste
 * yet" literals every caller used before it existed, and every existing
 * caller needed no change at all to pick that up. The fragment is scoped
 * to the project it was copied from: another project sees an empty
 * clipboard.
 */
import type { EditorSnapshot, Project } from "../editorTypes";
import type { ActionContext, PointerTarget } from "./actions";
import { clipboardFor } from "./clipboard";

export function baseActionContext(
  project: Project | null,
  snapshot: EditorSnapshot | null,
  playheadMs: number,
  selectedClipIds: string[],
  pointerTarget: PointerTarget | null = null,
): ActionContext {
  // Only a fragment copied from THIS project is offered (fix round 1 —
  // `clipboard.ts`'s own doc says why a cross-project paste is dangerous).
  const fragment = clipboardFor(project?.id);
  return {
    project,
    snapshot,
    playheadMs,
    selectedClipIds,
    pointerTarget,
    hasClipboard: fragment !== null,
    clipboardFragment: fragment,
  };
}
