import { computed, type Ref, ref, watch } from "vue";

import type { AggTask } from "../types";
import { localToday } from "../utils/taskFields";
import { type Grouping, loadGrouping, saveGrouping } from "../utils/taskGrouping";
import { archivedMatcher, type Bucket, dateBuckets, listSections, tagSections } from "../utils/taskSections";
import { loadSortPref, NATURAL_DIR, saveSortPref, type SortKey, taskComparator, type TaskSortPref } from "../utils/taskSort";

// The read side of the Tasks view: how the flat task list is filtered (title +
// tag), sorted (persisted per view), and grouped into displayed sections
// (Lists / Dates / Tags → buckets). State + derived computeds, no IPC, no
// rendering — split out of Tasks.vue (its churn hotspot). `sortInPlace` is
// shared with the write composables (useTaskActions / useTaskReorderCommit),
// which re-sort after an optimistic mutation, so it must be created here and
// threaded in.
export function useTaskDisplay(opts: {
  tasks: Ref<AggTask[]>;
  isAggregate: Ref<boolean>;
  knownLists: Ref<string[]>;
  listOrder: Ref<string[]>;
  archivedLists: Ref<string[]>;
  sortViewKey: string;
}) {
  const { tasks, isAggregate, knownLists, listOrder, archivedLists, sortViewKey } = opts;

  // Grouping choice, persisted per view. A fresh/unset view opens on Lists (the
  // DEFAULT inside taskGrouping.ts); a return visit recalls the last choice.
  const grouping = ref<Grouping>(loadGrouping(sortViewKey));
  watch(grouping, (g) => saveGrouping(sortViewKey, g));

  // The tasks the CURRENT grouping actually shows, before the title/tag filter:
  // Lists grouping hides OPEN tasks in archived lists (done ones still show in
  // the Done bucket; No list always shows), mirroring listSections; Dates/Tags
  // show everything. The progress bar and the >5 filter-row threshold count
  // from THIS set, not the raw list, so a vault whose only open tasks sit in an
  // archived list doesn't report phantom progress or a stray filter row (Codex,
  // PR #59). Filter-independent, so showFilter → filteredTasks stays acyclic.
  const visibleTasks = computed(() => {
    if (grouping.value !== "lists") return tasks.value;
    const isArchived = archivedMatcher(archivedLists.value);
    return tasks.value.filter((t) => t.done || t.list === "" || !isArchived(t.list));
  });

  const filter = ref("");
  // Same threshold as the vault list: a filter only earns its row above 5.
  const showFilter = computed(() => visibleTasks.value.length > 5);
  // One active tag filter at a time, set by clicking a row chip. Matching is
  // case-insensitive and exact per tag (nested tags are distinct strings).
  // Independent of the >5 title-filter threshold: it can only be activated by
  // clicking an existing chip, and its dismiss chip is always visible while
  // active, so it can never strand the user.
  const tagFilter = ref<string | null>(null);

  const filteredTasks = computed(() => {
    const q = filter.value.trim().toLowerCase();
    const tag = tagFilter.value?.toLowerCase() ?? null;
    return tasks.value.filter((t) => {
      if (tag && !t.tags.some((x) => x.toLowerCase() === tag)) return false;
      // Gate the title query on showFilter too: archiving below the threshold
      // hides the INPUT, and a stale query with no visible control would
      // strand the user on a narrowed/empty list until remount.
      if (q && showFilter.value && !t.title.toLowerCase().includes(q)) return false;
      return true;
    });
  });
  // Whether a filter is actually narrowing the list (matches filteredTasks'
  // own gates — including the showFilter gate, so stale hidden filter text
  // counts as INACTIVE): Lists grouping consults it to drop empty lists while
  // filtering, and the view's reorderView gates the drag grips on it (a
  // narrowed list must not rank against invisible neighbors; an unfiltered
  // one may reorder freely).
  const filterActive = computed(
    () => tagFilter.value !== null || (filter.value.trim() !== "" && showFilter.value),
  );

  // The user's sort choice for this view, persisted per view key ("all" for
  // the aggregate). The comparator lives in utils/taskSort (mirroring
  // core::tasks::list_tasks for Default) so an optimistic insert/edit lands
  // where a refetch would put it.
  const sortPref = ref<TaskSortPref>(loadSortPref(sortViewKey));

  function sortInPlace() {
    tasks.value.sort(taskComparator(sortPref.value));
  }

  // Picking a key resets direction to that key's natural one (due: soonest
  // first, created: newest first) instead of inheriting the previous key's
  // toggle state, which reads as arbitrary.
  function setSortKey(key: SortKey) {
    sortPref.value = { key, dir: NATURAL_DIR[key] };
    saveSortPref(sortViewKey, sortPref.value);
    sortInPlace();
  }

  function flipSortDir() {
    sortPref.value = { ...sortPref.value, dir: sortPref.value.dir === "asc" ? "desc" : "asc" };
    saveSortPref(sortViewKey, sortPref.value);
    sortInPlace();
  }

  const buckets = computed<Bucket[]>(() => {
    if (grouping.value === "tags") return tagSections(filteredTasks.value);
    if (grouping.value === "lists")
      // Per-vault mode surfaces empty (fresh) lists; the aggregate skips them
      // to avoid cross-vault noise.
      return listSections(filteredTasks.value, knownLists.value, listOrder.value, {
        includeEmpty: !isAggregate.value && !filterActive.value,
        archived: archivedLists.value,
      });
    return dateBuckets(filteredTasks.value, localToday());
  });

  // A fresh per-vault list folder has no tasks yet; keep the grouping control
  // reachable so it shows via Lists instead of hiding behind "No tasks yet" (the
  // aggregate omits empty lists) (Codex, PR #53 re-review).
  const hasDisplayableLists = computed(() => !isAggregate.value && knownLists.value.length > 0);

  // done / total of the VISIBLE tasks (archived-hidden ones excluded); drives
  // the progress bar so it matches what the Lists view actually shows.
  const progress = computed(() => {
    const total = visibleTasks.value.length;
    const done = visibleTasks.value.filter((t) => t.done).length;
    return { total, done, pct: total === 0 ? 0 : Math.round((done / total) * 100) };
  });

  // filteredTasks stays internal (only buckets consumes it) — exporting it
  // would invite a caller to bypass the grouping pipeline.
  return {
    filter,
    tagFilter,
    showFilter,
    filterActive,
    sortPref,
    sortInPlace,
    setSortKey,
    flipSortDir,
    grouping,
    buckets,
    hasDisplayableLists,
    progress,
  };
}
