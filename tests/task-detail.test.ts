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
});
