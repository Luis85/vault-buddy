/**
 * What `ReconnectDialog` says about one missing original after a reconnect
 * (Task 40) — a pure function so each wording is stated once. Names are
 * display names; nothing here ever holds a path.
 */
import type { ReconnectOutcome } from "../composables/useMediaReconnect";

export function describeOutcome(outcome: ReconnectOutcome | undefined): string {
  switch (outcome?.kind) {
    case "matched":
      return `Reconnected to “${outcome.file}”.`;
    case "replaced":
      return `Replaced with “${outcome.file}”.`;
    case "ambiguous": {
      if (outcome.files.length === 1) {
        return `“${outcome.files[0]}” fits more than one missing original, so it was not used. Choose the right file for this one.`;
      }
      const files = outcome.files.map((f) => `“${f}”`).join(", ");
      return `${outcome.files.length} chosen files match equally well: ${files}. Choose the right one.`;
    }
    case "mismatched":
      return `“${outcome.file}” is not this original — ${outcome.reason}. Choose another file, or replace it: you pick the file again, and your edit stays as it is.`;
    case "failed":
      return `“${outcome.file}” is this original, but it could not be brought in: ${outcome.error}`;
    case "excluded":
      return outcome.reason;
    case "unmatched":
      return "None of the chosen files is this original.";
    default:
      return "Missing.";
  }
}
