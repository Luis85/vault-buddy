/**
 * The Save a copy dialog's one status line (Task 39; SCREENS 08;
 * visual-parity Task 21 and its fix round 1). Pure, so every phrase is
 * tested without a dialog:
 *
 * - "Saved to <file name>" only from a matching receipt (the export's own
 *   state, never a timer).
 * - The rename phase is never blank: it reads "Preparing the project
 *   file…" like the export does.
 * - A refused rename (the dialog's own inline `lastError`) or a revision
 *   conflict on it stops the copy, and says so; a refusal that is neither
 *   (no open session) says only that the copy was not saved — never that
 *   the project changed.
 * - Ruling T21-4: a rename KEPT after the copy was dismissed or failed is
 *   stated — it is undoable, and a person must not find it by surprise.
 *   The name is the user's own title, on screen only; never logged.
 */
import type { ExportState } from "../composables/useProjectPackage";

export interface SaveCopyFacts {
  state: ExportState;
  /** The rename is in flight. */
  renaming: boolean;
  /** The dialog's own refused rename (its message). */
  refusal: string | null;
  /** Why the rename stopped the copy, when it was not an inline refusal:
   * its own revision conflict, or a refusal that said nothing. */
  stopped: "conflict" | "refused" | null;
  /** The title this save renamed the tutorial to, if it did. */
  renamedTo: string | null;
}

export interface SaveCopyLine {
  text: string;
  alert: boolean;
}

export const PREPARING = "Preparing the project file…";

function exportLine(s: ExportState): SaveCopyLine {
  switch (s.phase) {
    case "pending":
      return { text: PREPARING, alert: false };
    case "success":
      return { text: `Saved to ${s.fileName}`, alert: false };
    case "failure":
      return { text: `The project file was not saved. ${s.message}`, alert: true };
    case "cancelled":
      return { text: "Nothing was saved — the file dialog was closed.", alert: false };
    default:
      return { text: "", alert: false };
  }
}

/** Ruling T21-4: the copy did not land, the rename did. */
function withRename(line: SaveCopyLine, facts: SaveCopyFacts): SaveCopyLine {
  const missed = facts.state.phase === "cancelled" || facts.state.phase === "failure";
  if (!missed || !facts.renamedTo) return line;
  return { ...line, text: `${line.text} The tutorial was renamed to “${facts.renamedTo}”. Undo restores the old name.` };
}

export function saveCopyLine(facts: SaveCopyFacts): SaveCopyLine {
  if (facts.refusal) return { text: `The copy was not saved. ${facts.refusal}`, alert: true };
  if (facts.stopped === "conflict") return { text: "The project changed while saving the copy. Try again.", alert: true };
  if (facts.stopped === "refused") return { text: "The copy was not saved.", alert: true };
  if (facts.renaming) return { text: PREPARING, alert: false };
  return withRename(exportLine(facts.state), facts);
}
