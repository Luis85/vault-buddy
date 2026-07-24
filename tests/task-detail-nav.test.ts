import { createPinia,setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { useVaultsStore } from "../src/stores/vaults";
import type { AggTask } from "../src/types";

const task = (over: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, vaultId: "v1", vaultName: "V", ...over,
});

describe("task detail navigation", () => {
  beforeEach(() => setActivePinia(createPinia()));

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
});
