import { logWarning } from "../logging";
import type { AggTask } from "../types";
import { dueOf } from "./taskFields";

// The task views' sort machinery: one comparator factory over the user's
// sort preference, plus its per-view localStorage persistence. Pure and
// component-free so the Tasks container stays under the LOC cap and the
// ordering contract is unit-testable without mounting anything.

export type SortKey = "default" | "due" | "priority" | "created" | "title" | "manual";
export type SortDir = "asc" | "desc";
export interface TaskSortPref {
  key: SortKey;
  dir: SortDir;
}

export const SORT_OPTIONS: readonly { value: SortKey; label: string }[] = [
  { value: "default", label: "Default" },
  { value: "due", label: "Due date" },
  { value: "priority", label: "Priority" },
  { value: "created", label: "Created" },
  { value: "title", label: "Title" },
  { value: "manual", label: "Manual" },
];

/** Each key's natural direction — what the toggle starts from when the key
 * changes (due: soonest first; created: newest first; title: A→Z). */
export const NATURAL_DIR: Record<SortKey, SortDir> = {
  default: "asc",
  due: "asc",
  priority: "asc",
  created: "desc",
  title: "asc",
  manual: "asc",
};

/** Default is a fixed multi-key chain and Manual is the user's own hand
 * order — a direction toggle means nothing for either. */
export const directionApplies = (key: SortKey): boolean =>
  key !== "default" && key !== "manual";

const PRIORITY_RANK: Record<string, number> = { high: 0, low: 2 };
const rank = (t: AggTask) => PRIORITY_RANK[t.priority ?? ""] ?? 1;
// "0<date>" < "1" makes valid dues sort ascending ahead of undated.
const dueKey = (t: AggTask) => {
  const d = dueOf(t);
  return d ? `0${d}` : "1";
};

// The pre-selector chain, byte-identical to what the view always did (it
// mirrors core::tasks::list_tasks so an optimistic insert lands where a
// refetch would put it): open first (due asc → priority tier → newest
// created → title), done by newest created → title; both arms tiebreak
// vaultName → path so equal tasks from different vaults order stably.
const tiebreak = (a: AggTask, b: AggTask) =>
  a.title.localeCompare(b.title) ||
  a.vaultName.localeCompare(b.vaultName) ||
  a.path.localeCompare(b.path);
const doneChain = (a: AggTask, b: AggTask) =>
  b.created.localeCompare(a.created) || tiebreak(a, b);
const openChain = (a: AggTask, b: AggTask) =>
  dueKey(a).localeCompare(dueKey(b)) ||
  rank(a) - rank(b) ||
  b.created.localeCompare(a.created) ||
  tiebreak(a, b);
function defaultCompare(a: AggTask, b: AggTask): number {
  return Number(a.done) - Number(b.done) || (a.done ? doneChain(a, b) : openChain(a, b));
}

// The chosen key's own comparison, one small comparator per key. Direction
// flips ONLY the present-value comparison: an absent due and an unranked
// order sort last regardless of direction (flipping "no value" to the top
// serves nobody), and an absent priority stays in the middle tier by
// construction.
type KeyComparator = (a: AggTask, b: AggTask, flip: number) => number;
const KEY_COMPARATORS: Partial<Record<SortKey, KeyComparator>> = {
  due: (a, b, flip) => {
    const da = dueOf(a);
    const db = dueOf(b);
    if (da === null || db === null) return Number(da === null) - Number(db === null);
    return flip * da.localeCompare(db);
  },
  priority: (a, b, flip) => flip * (rank(a) - rank(b)),
  created: (a, b, flip) => flip * a.created.localeCompare(b.created),
  title: (a, b, flip) => flip * a.title.localeCompare(b.title),
  manual: (a, b) => {
    if (a.order === null || b.order === null)
      return Number(a.order === null) - Number(b.order === null);
    return a.order - b.order;
  },
};

/** Comparator for the view's sort preference. Done-last is universal (the
 * grouping modes give Done its own section either way); the chosen key
 * orders within that, and ties fall through to the Default chain so the
 * familiar clustering survives as a stable secondary order. */
export function taskComparator(pref: TaskSortPref): (a: AggTask, b: AggTask) => number {
  const byKey = KEY_COMPARATORS[pref.key];
  if (!byKey) return defaultCompare;
  const flip = pref.dir === "desc" ? -1 : 1;
  return (a, b) =>
    Number(a.done) - Number(b.done) || byKey(a, b, flip) || defaultCompare(a, b);
}

/** localStorage key; a JSON object keyed per view ("all" or a vault id). */
const STORAGE_KEY = "vault-buddy:task-sort";
const DEFAULT_PREF: TaskSortPref = { key: "manual", dir: "asc" };
const SORT_KEYS = new Set(SORT_OPTIONS.map((o) => o.value));

function readAll(): Record<string, unknown> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed))
      return parsed as Record<string, unknown>;
  } catch (e) {
    logWarning(`task sort: load failed: ${String(e)}`);
  }
  return {};
}

/** The persisted sort for a view; a missing/corrupted entry degrades to the
 * Default pref — with a warning, never a throw into the component. */
export function loadSortPref(viewKey: string): TaskSortPref {
  const entry = readAll()[viewKey];
  if (entry && typeof entry === "object") {
    const { key, dir } = entry as { key?: unknown; dir?: unknown };
    if (typeof key === "string" && SORT_KEYS.has(key as SortKey) && (dir === "asc" || dir === "desc")) {
      return { key: key as SortKey, dir };
    }
  }
  return { ...DEFAULT_PREF };
}

export function saveSortPref(viewKey: string, pref: TaskSortPref): void {
  const all = readAll();
  all[viewKey] = pref;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch (e) {
    logWarning(`task sort: save failed: ${String(e)}`);
  }
}
