import type { AggTask } from "../types";
import { dueOf } from "./taskFields";

// The grouping-section builders for the Tasks view (dates / tags / lists),
// extracted from the container so each mode is a pure, unit-testable
// function and the component stays under the LOC cap. Order WITHIN every
// section is the caller's global sort, untouched.

export type Bucket = { key: string; label: string | null; tasks: AggTask[] };

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
 * Headers always render in list mode. */
export function listSections(
  tasks: AggTask[],
  knownLists: string[],
  listOrder: string[],
  opts: { includeEmpty: boolean },
): Bucket[] {
  const byList = new Map<string, { label: string; tasks: AggTask[] }>();
  const ensure = (label: string) => {
    const key = label.toLowerCase();
    const entry = byList.get(key) ?? { label, tasks: [] };
    byList.set(key, entry);
    return entry;
  };
  if (opts.includeEmpty) for (const l of knownLists) ensure(l);
  const nolist: AggTask[] = [];
  const done: AggTask[] = [];
  for (const t of tasks) {
    if (t.done) done.push(t);
    else if (t.list === "") nolist.push(t);
    else ensure(t.list).tasks.push(t);
  }
  // listOrder names first (case-insensitive match against what exists),
  // then the rest alphabetically by label.
  const ordered: Bucket[] = [];
  const taken = new Set<string>();
  for (const name of listOrder) {
    const key = name.toLowerCase();
    const entry = byList.get(key);
    if (entry && !taken.has(key)) {
      taken.add(key);
      ordered.push({ key: `list:${key}`, label: entry.label, tasks: entry.tasks });
    }
  }
  const rest = [...byList.entries()]
    .filter(([key]) => !taken.has(key))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, { label, tasks }]) => ({ key: `list:${key}`, label, tasks }));
  const sections = [...ordered, ...rest];
  if (nolist.length > 0) sections.push({ key: "nolist", label: "No list", tasks: nolist });
  if (done.length > 0) sections.push({ key: "done", label: "Done", tasks: done });
  return sections;
}
