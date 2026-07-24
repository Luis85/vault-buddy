import { mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";

import { useTaskSchedule } from "../src/composables/useTaskSchedule";
import { useNotificationsStore } from "../src/stores/notifications";
import type { AggTask } from "../src/types";
import { localToday } from "../src/utils/taskFields";

function agg(p: Partial<AggTask>): AggTask {
  return {
    path: "p", title: "t", status: "new", created: "2026-07-01", done: false,
    due: null, scheduled: null, priority: null, tags: [], list: "", order: null, id: null,
    description: null, vaultId: "v", vaultName: "V", ...p,
  };
}

beforeEach(() => setActivePinia(createPinia()));
afterEach(() => vi.restoreAllMocks());

describe("quickSchedule", () => {
  it("writes the do-date optimistically", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => { if (cmd === "update_task") { calls.push(args); return null; } });
    const t = agg({ path: "a" });
    const tasks = ref<AggTask[]>([t]);
    const { quickSchedule } = useTaskSchedule({ tasks, sortInPlace: () => {}, busy: ref(new Set()) });
    await quickSchedule(t, "2026-07-20");
    expect(t.scheduled).toBe("2026-07-20");
    expect(calls[0]).toMatchObject({ patch: { scheduled: "2026-07-20" } });
  });
  it("reverts on failure", async () => {
    mockIPC((cmd) => { if (cmd === "update_task") throw new Error("nope"); });
    const t = agg({ path: "a", scheduled: "2026-07-01" });
    const { quickSchedule } = useTaskSchedule({ tasks: ref([t]), sortInPlace: () => {}, busy: ref(new Set()) });
    await quickSchedule(t, "2026-07-20");
    expect(t.scheduled).toBe("2026-07-01"); // reverted
  });
});

describe("rescheduleOverdue", () => {
  it("stamps today on all, best-effort — one failure reverts only its own task", async () => {
    mockIPC((cmd, args) => {
      if (cmd !== "update_task") return;
      if ((args as { path: string }).path === "bad") throw new Error("nope");
      return null;
    });
    const ok = agg({ path: "ok", title: "OK", scheduled: "2026-07-01" });
    const bad = agg({ path: "bad", title: "BAD", scheduled: "2026-07-02" });
    const { rescheduleOverdue } = useTaskSchedule({ tasks: ref([ok, bad]), sortInPlace: () => {}, busy: ref(new Set()) });
    await rescheduleOverdue([ok, bad]);
    expect(ok.scheduled).toBe(localToday()); // landed
    expect(bad.scheduled).toBe("2026-07-02"); // reverted (only this one)
  });
  it("holds back a busy row and never silently drops it", async () => {
    const calls: string[] = [];
    mockIPC((cmd, args) => { if (cmd === "update_task") { calls.push((args as { path: string }).path); return null; } });
    const free = agg({ path: "free", title: "Free", scheduled: "2026-07-01" });
    const busyRow = agg({ path: "busy", title: "Busy", scheduled: "2026-07-02" });
    const busy = ref(new Set(["busy"]));
    const { rescheduleOverdue } = useTaskSchedule({ tasks: ref([free, busyRow]), sortInPlace: () => {}, busy });
    await rescheduleOverdue([free, busyRow]);
    expect(calls).toEqual(["free"]); // the busy row is NOT written…
    expect(free.scheduled).toBe(localToday());
    expect(busyRow.scheduled).toBe("2026-07-02"); // …left untouched (still overdue) and named in the toast
  });

  // GAP-73c: a row merely OPEN in the inline editor isn't in `busy` (drafting,
  // not yet saving), so without this exclusion the batch would reschedule it
  // out from under the user, re-bucketing the row to Today and unmounting the
  // editor — silently discarding whatever the user had typed. Mirrors the
  // busy-row hold-and-name test above exactly, but via the editingPath arg.
  it("holds back the row open in the inline editor and never silently drops it (GAP-73c)", async () => {
    const calls: string[] = [];
    mockIPC((cmd, args) => { if (cmd === "update_task") { calls.push((args as { path: string }).path); return null; } });
    const free = agg({ path: "free", title: "Free", scheduled: "2026-07-01" });
    const editingRow = agg({ path: "editing", title: "Editing", scheduled: "2026-07-02" });
    const { rescheduleOverdue } = useTaskSchedule({ tasks: ref([free, editingRow]), sortInPlace: () => {}, busy: ref(new Set()) });
    await rescheduleOverdue([free, editingRow], "editing");
    expect(calls).toEqual(["free"]); // the editing row is NOT written…
    expect(free.scheduled).toBe(localToday());
    expect(editingRow.scheduled).toBe("2026-07-02"); // …left untouched (still overdue)
    const notifications = useNotificationsStore();
    expect(notifications.items.some((n) => n.kind === "error" && n.message.includes("Editing"))).toBe(true); // …and named in the toast
  });

  // Fold-in #3 (Codex P2, plan L1124): a batch-level in-flight guard. Without
  // it, re-clicking "Reschedule" before the first batch's writes land makes
  // the SECOND call see every first-batch target already in the per-row
  // `busy` set, classify them ALL as skipped, and immediately toast a false
  // "Couldn't reschedule" for writes that are actually in flight and about to
  // succeed.
  it("guards re-entrancy: a second call while the first batch is running is a silent no-op", async () => {
    let resolveFirst: (() => void) | undefined;
    let updateCalls = 0;
    mockIPC((cmd) => {
      if (cmd !== "update_task") return;
      updateCalls++;
      if (updateCalls === 1) {
        return new Promise<null>((r) => {
          resolveFirst = () => r(null);
        });
      }
      return null;
    });
    const a = agg({ path: "a", title: "A", scheduled: "2026-07-01" });
    const b = agg({ path: "b", title: "B", scheduled: "2026-07-02" });
    const { rescheduleOverdue, reschedulingOverdue } = useTaskSchedule({
      tasks: ref([a, b]), sortInPlace: () => {}, busy: ref(new Set()),
    });
    const notifications = useNotificationsStore();

    const firstCall = rescheduleOverdue([a, b]); // not awaited — still in flight
    expect(reschedulingOverdue.value).toBe(true);
    const toastsBefore = notifications.items.length;

    await rescheduleOverdue([a, b]); // re-entrant call — must be a pure no-op
    expect(updateCalls).toBe(1); // no second write wave started
    expect(notifications.items.length).toBe(toastsBefore); // no false "Couldn't reschedule" toast

    resolveFirst?.();
    await firstCall;
    expect(reschedulingOverdue.value).toBe(false); // cleared once the real batch finishes
    expect(a.scheduled).toBe(localToday());
    expect(b.scheduled).toBe(localToday());
  });
});
