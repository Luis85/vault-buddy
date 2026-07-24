import { invoke } from "@tauri-apps/api/core";
import { type Ref, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { AggTask } from "../types";
import { localToday } from "../utils/taskFields";
import { reflectStampedId } from "../utils/taskMutations";

// The plan-my-day verbs: quick-schedule (set/clear a task's do-date) and
// reschedule-overdue. Optimistic with per-row revert, serialized through the
// SAME busy guard the other row writes share (threaded in from useTaskActions),
// so a schedule can't race a toggle/edit on the same task. update_task stamps
// an id on an id-enabled vault (any non-empty patch), so each write reflects a
// freshly-stamped id like the edit/reorder paths.
export function useTaskSchedule(opts: {
  tasks: Ref<AggTask[]>;
  sortInPlace: () => void;
  busy: Ref<Set<string>>;
}) {
  const { sortInPlace, busy } = opts;
  const notifications = useNotificationsStore();

  // `date` = a YYYY-MM-DD do-date, or null to clear. Optimistic + revert.
  async function quickSchedule(task: AggTask, date: string | null): Promise<void> {
    if (busy.value.has(task.path)) return;
    const prev = task.scheduled;
    task.scheduled = date;
    sortInPlace();
    busy.value.add(task.path);
    try {
      const patch = date === null ? { clearScheduled: true } : { scheduled: date };
      reflectStampedId(
        task,
        await invoke<string | null>("update_task", { id: task.vaultId, path: task.path, patch }),
      );
    } catch (e) {
      task.scheduled = prev;
      sortInPlace();
      notifications.error(String(e));
      logWarning(`quickSchedule failed: ${String(e)}`);
    } finally {
      busy.value.delete(task.path);
    }
  }

  // Batch-level in-flight guard (Codex P2): without it, re-clicking
  // "Reschedule" before the first batch's writes land makes a SECOND call see
  // every first-batch target already parked in the per-row `busy` set below,
  // classify them ALL as skipped, and immediately toast a false "Couldn't
  // reschedule" for writes that are actually in flight and about to succeed.
  // A plain boolean (not a per-batch id) is enough — there is only ever one
  // Overdue bucket, so only one reschedule-all batch can be running at a
  // time. Exposed so the container can disable the trigger while true.
  const reschedulingOverdue = ref(false);

  // Reschedule EVERY task in `overdue` to today — genuinely best-effort:
  // independent per-task writes that do NOT stop on a rejection (unlike the
  // rank materialize's fail-fast batch), reverting only the failed task. A row
  // with a write already in flight can't be safely re-written here (two
  // read-modify-write saves would race), so it's held out of the batch — but it
  // is NOT dropped silently: it's named in the summary alongside any failures,
  // so the "reschedule all" action never reports success while quietly leaving a
  // task overdue (Codex, PR #75). The user can retry once its save lands.
  async function rescheduleOverdue(overdue: AggTask[]): Promise<void> {
    if (reschedulingOverdue.value) return; // a batch is already running — no-op, no toast
    reschedulingOverdue.value = true;
    try {
      const today = localToday();
      const skipped = overdue.filter((t) => busy.value.has(t.path)).map((t) => t.title);
      const targets = overdue.filter((t) => !busy.value.has(t.path));
      if (targets.length > 0) {
        const prev = new Map(targets.map((t) => [t.path, t.scheduled] as const));
        for (const t of targets) {
          t.scheduled = today;
          busy.value.add(t.path);
        }
        sortInPlace();
        for (const t of targets) {
          try {
            reflectStampedId(
              t,
              await invoke<string | null>("update_task", {
                id: t.vaultId, path: t.path, patch: { scheduled: today },
              }),
            );
          } catch (e) {
            t.scheduled = prev.get(t.path) ?? null;
            skipped.push(t.title); // write-failed → still overdue → named
            logWarning(`rescheduleOverdue failed for ${t.title}: ${String(e)}`);
          } finally {
            busy.value.delete(t.path);
          }
        }
        sortInPlace();
      }
      // One honest summary: every task still overdue — write-failed OR held back
      // for a busy save — is named; nothing is silently left behind.
      if (skipped.length > 0) {
        notifications.error(`Couldn't reschedule (still overdue): ${skipped.join(", ")}.`);
      }
    } finally {
      reschedulingOverdue.value = false;
    }
  }

  return { quickSchedule, rescheduleOverdue, reschedulingOverdue };
}
