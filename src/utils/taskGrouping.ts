import { createPerViewStore } from "./perViewStore";

// The tasks view's grouping-mode choice (Lists / Plan / Tags), persisted
// per view (localStorage, keyed "all" or a vault id) via the shared
// perViewStore envelope — see perViewStore.ts for the
// load/sanitize/degrade-to-default and save/merge/persist contract this
// rides on. The "Plan" mode's on-disk/type value stays "dates" (its original
// name) — display-only rename, no migration.

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
 * `defaultOverride ?? "lists"` — with a warning (from the corrupt-parse
 * path), never a throw into the component. A stored choice always wins over
 * `defaultOverride`, so passing one never rewrites a deliberate pick — it
 * only changes what an UNSET view opens on (the aggregate view passes
 * "dates" so a fresh "All tasks" visit opens on Plan). */
export function loadGrouping(viewKey: string, defaultOverride?: Grouping): Grouping {
  return store.load(viewKey, defaultOverride);
}

export function saveGrouping(viewKey: string, value: Grouping): void {
  store.save(viewKey, value);
}
