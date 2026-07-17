import { createPerViewStore } from "./perViewStore";

// The tasks view's grouping-mode choice (Lists / Dates / Tags), persisted
// per view (localStorage, keyed "all" or a vault id) via the shared
// perViewStore envelope — see perViewStore.ts for the
// load/sanitize/degrade-to-default and save/merge/persist contract this
// rides on.

export type Grouping = "dates" | "tags" | "lists";

const DEFAULT: Grouping = "lists";
const VALID = new Set<Grouping>(["dates", "tags", "lists"]);

const store = createPerViewStore<Grouping>(
  "vault-buddy:task-grouping",
  (raw) => (typeof raw === "string" && VALID.has(raw as Grouping) ? (raw as Grouping) : null),
  DEFAULT,
  "task grouping",
);

/** The persisted grouping for a view; a missing/corrupted entry degrades to
 * "lists" — with a warning, never a throw into the component. */
export function loadGrouping(viewKey: string): Grouping {
  return store.load(viewKey);
}

export function saveGrouping(viewKey: string, value: Grouping): void {
  store.save(viewKey, value);
}
