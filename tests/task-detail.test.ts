import { mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { createPinia,setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import TaskListPicker from "../src/components/TaskListPicker.vue";
import { useTaskDetail } from "../src/composables/useTaskDetail";
import { logWarning } from "../src/logging";
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

describe("TaskDetail.vue", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("renders the description and gates delete behind a confirm", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "update_task") return null;
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ description: "hello" }) } });
    await new Promise((r) => setTimeout(r));
    expect((wrapper.find('[data-testid="task-detail-description"]').element as HTMLTextAreaElement).value).toBe("hello");
    // First delete click reveals the confirm; the command is not sent yet.
    await wrapper.find('[data-testid="task-detail-delete"]').trigger("click");
    expect(calls.some((c) => c[0] === "delete_task")).toBe(false);
    await wrapper.find('[data-testid="task-detail-delete-confirm"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(calls.some((c) => c[0] === "delete_task")).toBe(true);
  });

  it("save sends a description change in the patch", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "update_task") return null;
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ description: null }) } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-description"]').setValue("new notes");
    await wrapper.get('[data-testid="task-detail-save"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    const call = calls.find((c) => c[0] === "update_task");
    expect(call[1].patch).toEqual({ description: "new notes" });
  });

  it("save clears the description when it's emptied out", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "update_task") return null;
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ description: "hello" }) } });
    await new Promise((r) => setTimeout(r));
    // Whitespace-only counts as emptied (trimmed before the emptiness check).
    await wrapper.get('[data-testid="task-detail-description"]').setValue("   ");
    await wrapper.get('[data-testid="task-detail-save"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    const call = calls.find((c) => c[0] === "update_task");
    expect(call[1].patch).toEqual({ clearDescription: true });
  });

  it("returns Save to disabled after a whitespace-clear save (no repeated no-op writes)", async () => {
    // A whitespace-only draft is equivalent to no description: after the clear
    // lands (task.description → null), the draft and the task agree so dirty is
    // false and Save disables, instead of emitting clearDescription forever
    // (Codex P2, PR #76).
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "update_task") return null;
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ description: "hello" }) } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-description"]').setValue("   ");
    await wrapper.get('[data-testid="task-detail-save"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(calls.filter((c) => c[0] === "update_task")).toHaveLength(1); // exactly one clear write
    expect((wrapper.find('[data-testid="task-detail-save"]').element as HTMLButtonElement).disabled).toBe(true);
  });

  it("disables Save when the draft is unchanged", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    expect((wrapper.find('[data-testid="task-detail-save"]').element as HTMLButtonElement).disabled).toBe(true);
  });

  it("disables Save when the title is blank, even though another field is dirty", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-due"]').setValue("2026-08-01");
    expect((wrapper.find('[data-testid="task-detail-save"]').element as HTMLButtonElement).disabled).toBe(false);
    await wrapper.get('[data-testid="task-detail-title"]').setValue("   ");
    expect((wrapper.find('[data-testid="task-detail-save"]').element as HTMLButtonElement).disabled).toBe(true);
    // Belt-and-suspenders: onSave's own guard blocks the write too, even if a
    // disabled button were somehow actuated (it isn't, in a real browser).
    await wrapper.get('[data-testid="task-detail-save"]').trigger("click");
    await new Promise((r) => setTimeout(r));
  });

  it("save sends the scheduled (do) date and tags in the patch", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "update_task") return null;
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-scheduled"]').setValue("2026-08-02");
    await wrapper.get('[data-testid="task-detail-tags"]').setValue("work, home");
    await wrapper.get('[data-testid="task-detail-save"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    const call = calls.find((c) => c[0] === "update_task");
    expect(call[1].patch).toEqual({ scheduled: "2026-08-02", tags: ["work", "home"] });
  });

  it("onMounted defaults to no archived lists when the config omits the field", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return ["Home"];
      // Older cached config shape predating archivedLists (AGENTS.md notes this
      // field is optional for exactly this reason) — must fall back to [].
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual(["Home"]);
  });

  it("a non-Escape key while the delete confirm is open leaves it open", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
    await wrapper.get('[data-testid="task-detail-delete-confirm"]').trigger("keydown", { key: "Enter" });
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(true);
  });

  it("clicking a priority button updates the selection and dirties the draft", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ priority: "high" }) } });
    await new Promise((r) => setTimeout(r));
    const highBtn = wrapper.get('[data-testid="task-detail-priority-high"]');
    const lowBtn = wrapper.get('[data-testid="task-detail-priority-low"]');
    expect(highBtn.attributes("aria-checked")).toBe("true"); // seeded from the task's priority
    await lowBtn.trigger("click");
    expect(lowBtn.attributes("aria-checked")).toBe("true");
    expect(highBtn.attributes("aria-checked")).toBe("false");
    expect((wrapper.find('[data-testid="task-detail-save"]').element as HTMLButtonElement).disabled).toBe(false);
  });

  it("Duplicate calls duplicate_task", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "duplicate_task") return "/v/Tasks/t (copy).md";
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-duplicate"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(calls).toContain("duplicate_task");
  });

  it("Open in Obsidian launches the task and closes the panel", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-open"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(calls).toContain("open_task");
    expect(calls).toContain("close_panel");
  });

  it("onMounted keeps the task's own list even when it's archived", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return ["Home", "Old"];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: ["Old"] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ list: "Old" }) } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual(["Home", "Old"]);
  });

  it("onMounted drops archived lists other than the task's own", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return ["Home", "Old"];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: ["Old"] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ list: "" }) } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual(["Home"]);
  });

  it("onMounted logs and leaves an empty picker when list_task_lists rejects", async () => {
    (logWarning as ReturnType<typeof vi.fn>).mockClear();
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") throw new Error("boom");
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual([]);
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("task detail: could not load task lists"),
    );
  });

  it("Cancel dismisses the delete confirm without deleting", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(true);
    await wrapper.get('[data-testid="task-detail-delete-cancel"]').trigger("click");
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(false);
    expect(calls).not.toContain("delete_task");
  });

  it("Escape closes the delete confirm without deleting", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(true);
    await wrapper.get('[data-testid="task-detail-delete-confirm"]').trigger("keydown", { key: "Escape" });
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(false);
    expect(calls).not.toContain("delete_task");
  });

  it("changing the list dirties the draft and moves the task on save", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return ["Home", "Work"];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "move_task_to_list") return { path: "/v/Tasks/Home/t.md", id: null };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ list: "" }) } });
    try {
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-detail-list"]').trigger("click");
      await new Promise((r) => setTimeout(r));
      (document.body.querySelector('[data-testid="task-detail-list-option-Home"]') as HTMLElement).click();
      await new Promise((r) => setTimeout(r));
      expect((wrapper.find('[data-testid="task-detail-save"]').element as HTMLButtonElement).disabled).toBe(false);
      await wrapper.get('[data-testid="task-detail-save"]').trigger("click");
      await new Promise((r) => setTimeout(r));
      expect(calls.some((c) => c[0] === "move_task_to_list")).toBe(true);
    } finally {
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("shares one busy guard: a slow write disables every detail write control", async () => {
    // The serialize-all-writes invariant (Codex P2): while ANY write is in
    // flight, a DIFFERENT write control must also be disabled so a second write
    // can't race the first. Manually-resolved-pending idiom from tasks.test.ts.
    let resolveDup: (() => void) | undefined;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "duplicate_task")
        return new Promise<string>((r) => {
          resolveDup = () => r("/v/Tasks/t (copy).md");
        });
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    const del = () => wrapper.find('[data-testid="task-detail-delete"]').element as HTMLButtonElement;
    const dup = () => wrapper.find('[data-testid="task-detail-duplicate"]').element as HTMLButtonElement;
    expect(del().disabled).toBe(false);
    await wrapper.get('[data-testid="task-detail-duplicate"]').trigger("click"); // slow write starts
    await new Promise((r) => setTimeout(r));
    expect(dup().disabled).toBe(true);
    expect(del().disabled).toBe(true); // a DIFFERENT control, disabled by the shared guard
    resolveDup?.();
    await new Promise((r) => setTimeout(r));
    expect(del().disabled).toBe(false);
    expect(dup().disabled).toBe(false);
  });

  it("focuses the confirm button when the delete confirm opens", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() }, attachTo: document.body });
    try {
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
      await new Promise((r) => setTimeout(r)); // openConfirm awaits a tick before focusing
      expect(document.activeElement).toBe(
        wrapper.get('[data-testid="task-detail-delete-confirm"]').element,
      );
    } finally {
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("swallows Escape only while the confirm is open, letting it bubble otherwise", async () => {
    // Confirm CLOSED → Escape must reach the document so PanelRoot's window
    // handler can close the panel like every other view; OPEN → swallowed and
    // steps back one level (reviewer + Codex P2, PR #76).
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() }, attachTo: document.body });
    const seen: string[] = [];
    const onDocKeydown = (e: Event) => seen.push((e as KeyboardEvent).key);
    document.addEventListener("keydown", onDocKeydown);
    try {
      await new Promise((r) => setTimeout(r));
      // Confirm closed → Escape bubbles all the way to the document.
      await wrapper.get('[data-testid="task-detail-title"]').trigger("keydown", { key: "Escape" });
      expect(seen).toContain("Escape");
      // Open the confirm, then Escape is swallowed (never reaches the document)
      // and closes the confirm.
      seen.length = 0;
      await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-detail-delete-confirm"]').trigger("keydown", { key: "Escape" });
      expect(seen).not.toContain("Escape");
      expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(false);
    } finally {
      document.removeEventListener("keydown", onDocKeydown);
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("onMounted orders lists by the vault's listOrder then alphabetical", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return ["Alpha", "Zebra", "Middle"];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: ["Zebra", "Middle"], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ list: "" }) } });
    await new Promise((r) => setTimeout(r));
    // listOrder first (Zebra, Middle), then the unordered rest alphabetically.
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual(["Zebra", "Middle", "Alpha"]);
  });
});
