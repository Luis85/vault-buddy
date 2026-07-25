import { clearMocks,mockIPC } from "@tauri-apps/api/mocks";
import { createPinia,setActivePinia } from "pinia";
import { afterEach,beforeEach, describe, expect, it } from "vitest";
import { ref } from "vue";

import { useTaskActions } from "../src/composables/useTaskActions";
import { useVaultsStore } from "../src/stores/vaults";
import type { AggTask } from "../src/types";

const task = (over: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, parentId: null, parentLink: null, vaultId: "v1", vaultName: "V", ...over,
});

describe("task detail navigation", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("onOpenTask does NOT open detail while the row's own write is in flight (P1 race)", async () => {
    // A pending row write + opening Detail would race two whole-document writers
    // (a late field write could recreate a file Detail just deleted) — Codex P1, PR #76.
    let resolve: (() => void) | undefined;
    mockIPC((cmd) =>
      cmd === "set_task_status"
        ? new Promise((r) => {
            resolve = () => r(null);
          })
        : undefined,
    );
    const s = useVaultsStore();
    const t = task();
    const actions = useTaskActions({ tasks: ref([t]), sortInPlace: () => {} });
    const pending = actions.toggle(t); // slow write → t.path enters the busy set
    await new Promise((r) => setTimeout(r));
    actions.onOpenTask(t, new MouseEvent("click")); // plain click while the row is busy
    expect(s.view).not.toBe("taskDetail"); // suppressed — no race
    resolve?.();
    await pending;
    actions.onOpenTask(t, new MouseEvent("click")); // row settled → opens now
    expect(s.view).toBe("taskDetail");
  });

  it("opens detail keeping the aggregate/per-vault mode, and back() restores it", () => {
    const s = useVaultsStore();
    s.openAllTasks(); // aggregate: tasksVaultId = null
    s.openTaskDetail(task());
    expect(s.view).toBe("taskDetail");
    expect(s.taskDetailTask?.path).toBe("/v/Tasks/t.md");
    expect(s.tasksVaultId).toBeNull(); // NOT cleared by openTaskDetail
    s.back();
    expect(s.view).toBe("tasks");
    expect(s.tasksVaultId).toBeNull(); // aggregate mode preserved
  });

  it("back() from a per-vault detail returns to that vault's tasks", () => {
    const s = useVaultsStore();
    s.openTasks("v1");
    s.openTaskDetail(task({ vaultId: "v1" }));
    s.back();
    expect(s.view).toBe("tasks");
    expect(s.tasksVaultId).toBe("v1");
  });

  it("refresh() keeps Task Detail (not showList) while a detail write is in flight", async () => {
    // A panel auto-hide + reopen mid-write must not reset to the list and let the
    // write finish off-screen against a stale, remounted Tasks list (Codex P2, PR #76).
    mockIPC((cmd) => {
      if (cmd === "take_pending_import") return [];
      if (cmd === "list_vaults") return [];
      return undefined; // take_add_document_request → falsy, counts → none
    });
    const s = useVaultsStore();
    s.openTaskDetail(task());
    s.taskDetailBusy = true;
    await s.refresh();
    expect(s.view).toBe("taskDetail"); // kept
    // Once the write settles, a normal refresh resets to the list.
    s.taskDetailBusy = false;
    await s.refresh();
    expect(s.view).toBe("list");
  });
});
