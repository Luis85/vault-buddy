/**
 * The one rename path (visual-parity Task 21 fix round 1): the Rename
 * tutorial dialog and Save a copy's Project name both go through here, so
 * the two cannot trim, compare or send a rename differently. An empty
 * title is refused before anything is sent; an unchanged one sends
 * nothing; otherwise ONE `rename` edit through `editorProject.execute` —
 * whose refusal surfaces as `lastError` (or, on a revision conflict, as
 * `conflictIntent`), like every edit.
 */
import { useEditorProjectStore } from "../stores/editorProject";

export type RenameOutcome = "empty" | "unchanged" | "renamed" | "refused";

export async function renameIfChanged(draft: string): Promise<RenameOutcome> {
  const project = useEditorProjectStore();
  const title = draft.trim();
  if (!title) return "empty";
  if (title === project.snapshot?.title) return "unchanged";
  return (await project.execute({ kind: "rename", title })) ? "renamed" : "refused";
}
