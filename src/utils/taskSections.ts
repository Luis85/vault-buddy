import type { AggTask } from "../types";
import { dueOf } from "./taskFields";

// The grouping-section builders for the Tasks view (dates / tags / lists),
// extracted from the container so each mode is a pure, unit-testable
// function and the component stays under the LOC cap. Order WITHIN every
// section is the caller's global sort, untouched.

export type Bucket = { key: string; label: string | null; tasks: AggTask[]; list?: string };

/** Date buckets: Overdue / Today / Upcoming / No date / Done. Headers render
 * only once a dated open task exists — a vault that never uses due dates
 * keeps the flat list it always had. */
export function dateBuckets(tasks: AggTask[], today: string): Bucket[] {
  const groups: Record<string, AggTask[]> = {
    overdue: [],
    today: [],
    upcoming: [],
    nodate: [],
    done: [],
  };
  for (const t of tasks) {
    if (t.done) groups.done.push(t);
    else {
      const d = dueOf(t);
      if (!d) groups.nodate.push(t);
      else if (d < today) groups.overdue.push(t);
      else if (d === today) groups.today.push(t);
      else groups.upcoming.push(t);
    }
  }
  const showHeaders =
    groups.overdue.length + groups.today.length + groups.upcoming.length > 0;
  return [
    { key: "overdue", label: "Overdue" },
    { key: "today", label: "Today" },
    { key: "upcoming", label: "Upcoming" },
    { key: "nodate", label: "No date" },
    { key: "done", label: "Done" },
  ]
    .map(({ key, label }) => ({ key, label: showHeaders ? label : null, tasks: groups[key] }))
    .filter((b) => b.tasks.length > 0);
}

/** One section per tag (alphabetical, case-insensitive), a task under EACH
 * of its tags — Obsidian tag-pane semantics; then No tags, then Done.
 * Headers always render in tag mode. */
export function tagSections(tasks: AggTask[]): Bucket[] {
  const byTag = new Map<string, { label: string; tasks: AggTask[] }>();
  const notags: AggTask[] = [];
  const done: AggTask[] = [];
  for (const t of tasks) {
    if (t.done) {
      done.push(t);
      continue;
    }
    if (t.tags.length === 0) {
      notags.push(t);
      continue;
    }
    for (const tag of t.tags) {
      const key = tag.toLowerCase();
      const entry = byTag.get(key) ?? { label: tag, tasks: [] };
      entry.tasks.push(t);
      byTag.set(key, entry);
    }
  }
  const sections: Bucket[] = [...byTag.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, { label, tasks }]) => ({ key: `tag:${key}`, label: `#${label}`, tasks }));
  if (notags.length > 0) sections.push({ key: "notags", label: "No tags", tasks: notags });
  if (done.length > 0) sections.push({ key: "done", label: "Done", tasks: done });
  return sections;
}

/** One section per list — `listOrder` names first (those that exist), the
 * remaining lists alphabetical — then No list (open tasks at the tasks
 * root), then Done. Same-named lists merge case-insensitively with the
 * first-seen casing as the label (the tags precedent), which is how the
 * aggregate view unifies "Next" across vaults. `includeEmpty` renders
 * task-less known lists as empty sections (per-vault mode, so a fresh list
 * is visible); the aggregate passes false to avoid cross-vault noise.
 * `archived` (Task 8) hides a list's section entirely AND drops its open
 * tasks from the grouping rather than bucketing them — the folder + tasks
 * still exist on disk, and the SAME task still shows under Dates/Tags
 * grouping (this only scopes the Lists view); a done task is unaffected
 * either way since Done already ignores list assignment. Each list bucket
 * carries its raw `list` name (used by callers — e.g. a future section
 * menu, or a cross-list drop target — to identify which list a section is).
 * Headers always render in list mode. */
export function listSections(
  tasks: AggTask[],
  knownLists: string[],
  listOrder: string[],
  opts: { includeEmpty: boolean; archived: string[] },
): Bucket[] {
  const archived = new Set(opts.archived.map((a) => a.toLowerCase()));
  const byList = new Map<string, { label: string; tasks: AggTask[] }>();
  const ensure = (label: string) => {
    const key = label.toLowerCase();
    const entry = byList.get(key) ?? { label, tasks: [] };
    byList.set(key, entry);
    return entry;
  };
  if (opts.includeEmpty)
    for (const l of knownLists) if (!archived.has(l.toLowerCase())) ensure(l);
  const nolist: AggTask[] = [];
  const done: AggTask[] = [];
  for (const t of tasks) {
    if (t.done) done.push(t);
    else if (t.list === "") nolist.push(t);
    else if (archived.has(t.list.toLowerCase())) continue; // hidden with its list
    else ensure(t.list).tasks.push(t);
  }
  // Explicitly Bucket[] (not inferred from the map below): the nolist/done
  // pushes further down carry no `list`, which would otherwise conflict with
  // the narrower `{ list: string }` shape TS would infer from this map's
  // return value alone.
  const sections: Bucket[] = orderLists(
    [...byList.values()].map((e) => e.label),
    listOrder,
  ).map((label) => {
    const key = label.toLowerCase();
    return { key: `list:${key}`, label, list: label, tasks: byList.get(key)?.tasks ?? [] };
  });
  if (nolist.length > 0) sections.push({ key: "nolist", label: "No list", tasks: nolist });
  if (done.length > 0) sections.push({ key: "done", label: "Done", tasks: done });
  return sections;
}

/** Display order for list names: `listOrder` entries first (case-insensitive
 * match against what exists, order names without a match ignored), the rest
 * alphabetical. Shared by the sections above and the pickers so a list never
 * sits in two different places on one screen. */
export function orderLists(names: string[], listOrder: string[]): string[] {
  const byKey = new Map<string, string>();
  for (const n of names) {
    const k = n.toLowerCase();
    if (!byKey.has(k)) byKey.set(k, n);
  }
  const ordered: string[] = [];
  const taken = new Set<string>();
  for (const name of listOrder) {
    const key = name.toLowerCase();
    const label = byKey.get(key);
    if (label !== undefined && !taken.has(key)) {
      taken.add(key);
      ordered.push(label);
    }
  }
  const rest = [...byKey.entries()]
    .filter(([key]) => !taken.has(key))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([, label]) => label);
  return [...ordered, ...rest];
}

/** The list a cross-section drop targets under Lists grouping: the
 * over-section's `list` name, `""` for the No-list section, or `null` when the
 * drop is a within-section reorder (same section) or onto a section that isn't
 * a list target (Done carries no `list`). Pure so the drag-to-move decision
 * stays testable and out of the component's branch budget. */
export function dropTargetList(over: Bucket | undefined, sectionKey: string): string | null {
  if (!over || over.key === sectionKey) return null;
  if (over.list !== undefined) return over.list;
  return over.key === "nolist" ? "" : null;
}

/** The section key a cross-list DROP would land on during a drag — the over
 * section's key when it's a different, valid list target under Lists grouping,
 * else `null`. Drives the target-section highlight and suppresses the origin's
 * now-misleading drop line. Pure wrapper over `dropTargetList` so the view
 * binds one key comparison. */
export function crossListDropTargetKey(
  drag: { sectionKey: string; overSectionKey: string | null } | null,
  grouping: string,
  buckets: Bucket[],
): string | null {
  if (!drag || grouping !== "lists") return null;
  const over = buckets.find((b) => b.key === drag.overSectionKey);
  return dropTargetList(over, drag.sectionKey) !== null ? drag.overSectionKey : null;
}
