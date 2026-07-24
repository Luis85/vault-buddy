import { mockIPC } from "@tauri-apps/api/mocks";
import { createPinia,setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";

import { useTaskDetail } from "../src/composables/useTaskDetail";
import type { AggTask } from "../src/types";

const task = (o: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, vaultId: "v1", vaultName: "V", ...o,
});

describe("useTaskDetail", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("save sends description and reflects it locally", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => { calls.push([cmd, args]); return cmd === "update_task" ? null : undefined; });
    const t = ref(task());
    const { save } = useTaskDetail(t);
    await save({ description: "notes" });
    expect(calls[0][0]).toBe("update_task");
    expect(calls[0][1].patch.description).toBe("notes");
    expect(t.value.description).toBe("notes");
  });

  it("remove deletes then navigates back", async () => {
    mockIPC((cmd) => (cmd === "delete_task" ? undefined : undefined));
    const t = ref(task());
    const { remove } = useTaskDetail(t);
    const { useVaultsStore } = await import("../src/stores/vaults");
    const back = vi.spyOn(useVaultsStore(), "back");
    await remove();
    expect(back).toHaveBeenCalled();
  });

  it("save is a no-op for an empty patch (no invoke)", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => { calls.push(cmd); return undefined; });
    const { save } = useTaskDetail(ref(task()));
    expect(await save({})).toBe(true);
    expect(calls).toEqual([]);
  });

  it("save moves the task to a new list and adopts the landed path", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "update_task") return null;
      if (cmd === "move_task_to_list") return { path: "/v/Tasks/Home/t.md", id: "abc" };
      return undefined;
    });
    const t = ref(task({ list: "" }));
    await useTaskDetail(t).save({ title: "New", list: "Home" });
    expect(calls).toContain("move_task_to_list");
    expect(t.value.list).toBe("Home");
    expect(t.value.path).toBe("/v/Tasks/Home/t.md");
  });

  it("save surfaces an error and releases the guard", async () => {
    mockIPC((cmd) => { if (cmd === "update_task") throw new Error("boom"); return undefined; });
    const { save, busy } = useTaskDetail(ref(task()));
    expect(await save({ description: "x" })).toBe(false);
    expect(busy.value).toBe(false);
  });

  it("remove surfaces an error and releases the guard", async () => {
    mockIPC((cmd) => { if (cmd === "delete_task") throw new Error("nope"); return undefined; });
    const { remove, busy } = useTaskDetail(ref(task()));
    await remove();
    expect(busy.value).toBe(false);
  });

  it("duplicate notifies with an Open action that launches the new copy", async () => {
    const calls: Array<[string, any]> = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "duplicate_task") return "/v/Tasks/t (copy).md";
      return undefined;
    });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const notify = vi.spyOn(useNotificationsStore(), "notify");
    await useTaskDetail(ref(task())).duplicate();
    expect(calls[0][0]).toBe("duplicate_task");
    const opts = notify.mock.calls[0][2] as { action: { run: () => Promise<void> } };
    await opts.action.run(); // the toast's "Open" action
    expect(calls.find((c) => c[0] === "open_task")?.[1].path).toBe("/v/Tasks/t (copy).md");
    expect(calls.map((c) => c[0])).toContain("close_panel");
  });

  it("duplicate surfaces an error and releases the guard", async () => {
    mockIPC((cmd) => { if (cmd === "duplicate_task") throw new Error("dupe fail"); return undefined; });
    const { duplicate, busy } = useTaskDetail(ref(task()));
    await duplicate();
    expect(busy.value).toBe(false);
  });

  it("openInObsidian launches the task and closes the panel", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => { calls.push(cmd); return undefined; });
    await useTaskDetail(ref(task())).openInObsidian();
    expect(calls).toContain("open_task");
    expect(calls).toContain("close_panel");
  });

  it("openInObsidian surfaces a launch error without throwing", async () => {
    mockIPC((cmd) => { if (cmd === "open_task") throw new Error("launch fail"); return undefined; });
    await expect(useTaskDetail(ref(task())).openInObsidian()).resolves.toBeUndefined();
  });
});
