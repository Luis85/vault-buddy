import { invoke } from "@tauri-apps/api/core";
import { type Ref, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { AggTask } from "../types";
import type { Grouping } from "../utils/taskGrouping";
import { planReorder } from "../utils/taskOrder";
import { type Bucket, dropTargetList } from "../utils/taskSections";

// Reflect a freshly-stamped task id (update_task's return) onto the row so the
// editor's copy-id affordance shows without a reload. No-op when ids are off
// (the command returns null). Module-level + branchless-at-the-call-site so the
// reorder writers don't each carry the extra branch.
function reflectStampedId(task: AggTask, id: string | null) {
  if (id) task.id = id;
}

// The write side of manual reordering + drag-to-move: turns a dropped slot (or
// a cross-list drop) into the `order`/`move` vault writes, optimistic with
// revert. Split out of Tasks.vue (LOC + churn hotspot) — state + IPC, no
// rendering; the interaction state machine stays in useTaskReorder, which calls
// `commitReorder` as its commit. Every write serializes view-wide through
// `reordering` (shared with the busy guard) so a second reorder can't compute a
// rank against an optimistic, not-yet-persisted position.
export function useTaskReorderCommit(opts: {
  busy: Ref<Set<string>>;
  sortInPlace: () => void;
  buckets: Ref<Bucket[]>;
  grouping: Ref<Grouping>;
}) {
  const notifications = useNotificationsStore();
  const { busy, sortInPlace, buckets, grouping } = opts;
  const reordering = ref(false);

  async function commitReorder(
    sectionKey: string,
    fromIndex: number,
    toIndex: number,
    overSectionKey: string | null,
  ) {
    const origin = buckets.value.find((b) => b.key === sectionKey);
    const tasks = origin?.tasks ?? [];
    const task = tasks[fromIndex];
    // A release over a DIFFERENT list section is a cross-list move (Lists
    // grouping only); everything else is a within-section rank reorder.
    const targetList = dropTargetList(
      buckets.value.find((b) => b.key === overSectionKey),
      sectionKey,
    );
    if (task && targetList !== null && grouping.value === "lists") {
      await moveTaskToList(task, targetList);
      return;
    }
    const plan = planReorder(tasks, fromIndex, toIndex);
    if (!plan) return;
    if (plan.kind === "single") await writeSingleRank(tasks[fromIndex], plan.order);
    else await materializeRanks(tasks, plan.orders);
  }

  // Cross-list move via drag: optimistic list change (the row jumps to the
  // target section), then adopt the landed path (move_task_to_list may add a
  // ` (N)` collision suffix), revert + toast on failure. Serialized view-wide
  // through `reordering` like the rank writes.
  async function moveTaskToList(task: AggTask, list: string) {
    if (busy.value.has(task.path) || task.list === list) return;
    const prevPath = task.path;
    const prevList = task.list;
    task.list = list;
    sortInPlace();
    busy.value.add(prevPath);
    reordering.value = true;
    try {
      // move_task_to_list returns the landed path (possibly ` (N)`-suffixed)
      // AND the task's id (freshly stamped when the vault opts in and it lacked
      // one) — adopt both so a moved legacy task reveals copy-id without a
      // reload, like the reorder/edit paths (Codex, PR #59).
      const moved = await invoke<{ path: string; id: string | null }>("move_task_to_list", {
        id: task.vaultId,
        path: prevPath,
        list,
      });
      task.path = moved.path;
      reflectStampedId(task, moved.id);
      sortInPlace();
    } catch (e) {
      task.list = prevList;
      sortInPlace();
      notifications.error(String(e));
      logWarning(`move_task_to_list failed: ${String(e)}`);
    } finally {
      busy.value.delete(prevPath);
      reordering.value = false;
    }
  }

  // One midpoint write, optimistic with revert — the common drop.
  async function writeSingleRank(task: AggTask, order: number) {
    if (busy.value.has(task.path)) return;
    const prev = task.order;
    task.order = order;
    sortInPlace();
    busy.value.add(task.path);
    // The view-level guard (shared with materialization) makes every grip inert
    // until this write resolves: a second reorder would compute its rank against
    // this optimistic, not-yet-persisted position and diverge if this write
    // later fails and reverts. Serialize reorders view-wide instead.
    reordering.value = true;
    try {
      // update_task returns the task's current id (freshly stamped when this
      // order-only reorder is the first edit on an id-enabled vault) — reflect
      // it so the editor's copy-id row shows without a reload, the same reason
      // applyFieldPatch captures it (Codex, PR #59).
      reflectStampedId(
        task,
        await invoke<string | null>("update_task", { id: task.vaultId, path: task.path, patch: { order } }),
      );
    } catch (e) {
      task.order = prev;
      sortInPlace();
      notifications.error(String(e));
      logWarning(`reorder failed: ${String(e)}`);
    } finally {
      busy.value.delete(task.path);
      reordering.value = false;
    }
  }

  // Materialization: seed spaced ranks across the section — optimistic for
  // the whole batch, serialized writes (each its own file, possibly across
  // vaults in the aggregate). The view-level guard keeps a second reorder from
  // interleaving.
  async function materializeRanks(section: AggTask[], orders: Map<string, number>) {
    const affected = section.filter((t) => orders.has(t.path));
    // Abort if ANY affected row already has an in-flight write (e.g. a slow
    // status toggle on a neighbor in this section). Materialize must write EVERY
    // affected row to establish the section's total order, so it can't just skip
    // the busy one — and writing order to that file mid-save would race the
    // in-flight write (both are read-modify-write frontmatter edits, so whichever
    // lands last drops the other's change). Bail and let the user retry once the
    // save lands — the same silent no-op writeSingleRank does for its one busy
    // row (Codex, PR #53 re-review).
    if (affected.some((t) => busy.value.has(t.path))) return;
    reordering.value = true;
    // No affected row is busy (asserted above), so guard them all and — because
    // this batch owns every one of their guards — clear them all in `finally`.
    // Its update_task(order) and a toggle/edit/archive on the same row are both
    // read-modify-write frontmatter saves, so leaving the row controls live would
    // let a concurrent write clobber the order (or vice versa).
    affected.forEach((t) => busy.value.add(t.path));
    const prevOrders = new Map(affected.map((t) => [t.path, t.order] as const));
    for (const t of affected) t.order = orders.get(t.path) ?? t.order;
    sortInPlace();
    // The writes are serialized and non-atomic across files, so a mid-batch
    // failure leaves earlier files already written. Track what landed and
    // revert ONLY the tasks that never reached disk — reverting the whole batch
    // would desync the UI from a partially-written section (the mismatch would
    // surface on the next reload as a phantom partial reorder). Same "keep what
    // succeeded, name what failed" posture as the editor's field-then-move save.
    const written = new Set<string>();
    try {
      for (const t of affected) {
        // Reflect a freshly-stamped id here too (see writeSingleRank) — a
        // materialize can be the first edit on several previously-unranked
        // legacy tasks at once.
        reflectStampedId(
          t,
          await invoke<string | null>("update_task", { id: t.vaultId, path: t.path, patch: { order: t.order } }),
        );
        written.add(t.path);
      }
    } catch (e) {
      // `?? null` (not `?? t.order`): a previous order of null means the task
      // was UNRANKED and must revert to unranked — `null ?? t.order` would
      // wrongly keep the new optimistic rank. Every affected path is a key in
      // prevOrders, so a genuinely missing entry can't occur here.
      for (const t of affected) {
        if (!written.has(t.path)) t.order = prevOrders.get(t.path) ?? null;
      }
      sortInPlace();
      notifications.error(`Couldn't save the new order: ${String(e)}`);
      logWarning(`reorder materialization failed: ${String(e)}`);
    } finally {
      affected.forEach((t) => busy.value.delete(t.path));
      reordering.value = false;
    }
  }

  return { reordering, commitReorder };
}
