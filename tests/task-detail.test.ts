import { mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { createPinia,setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import TaskListPicker from "../src/components/TaskListPicker.vue";
import { useTaskDetail } from "../src/composables/useTaskDetail";
import { logWarning } from "../src/logging";
import type { AggTask, TaskWriteResult } from "../src/types";

// update_task's mocked "the write succeeded, no relationship change" reply
// (Task 7 widened the command's return from a bare id to this object).
const updateTaskOk: TaskWriteResult = { id: null, parentId: null, parentLink: null, idsEnabled: false };

const task = (o: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, parentId: null, parentLink: null, vaultId: "v1", vaultName: "V", ...o,
});

describe("useTaskDetail", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("save sends description and reflects it locally", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => { calls.push([cmd, args]); return cmd === "update_task" ? updateTaskOk : undefined; });
    const t = ref(task());
    const { save } = useTaskDetail(t);
    await save({ description: "notes" });
    expect(calls[0][0]).toBe("update_task");
    expect(calls[0][1].patch.description).toBe("notes");
    expect(t.value.description).toBe("notes");
  });

  it("remove deletes then navigates back", async () => {
    mockIPC(() => undefined);
    const t = ref(task());
    const { remove } = useTaskDetail(t);
    const { useVaultsStore } = await import("../src/stores/vaults");
    const store = useVaultsStore();
    store.view = "taskDetail"; // remove() only navigates while still on the detail view
    const back = vi.spyOn(store, "back");
    await remove();
    expect(back).toHaveBeenCalled();
  });

  it("remove does NOT navigate again if the user already left the detail view", async () => {
    // Slow delete + the user clicks header Back first: the view already moved to
    // tasks, so remove()'s completion back() must NOT run (it would over-advance
    // to the vault list) — Codex P2, PR #76.
    let resolveDelete: (() => void) | undefined;
    mockIPC((cmd) =>
      cmd === "delete_task"
        ? new Promise<void>((r) => {
            resolveDelete = () => r();
          })
        : undefined,
    );
    const t = ref(task());
    const { remove } = useTaskDetail(t);
    const { useVaultsStore } = await import("../src/stores/vaults");
    const store = useVaultsStore();
    store.view = "taskDetail";
    const back = vi.spyOn(store, "back");
    const pending = remove(); // delete in flight
    await new Promise((r) => setTimeout(r));
    store.view = "tasks"; // the user navigated away during the slow delete
    resolveDelete?.();
    await pending;
    expect(back).not.toHaveBeenCalled();
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
      if (cmd === "update_task") return updateTaskOk;
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
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    const { save, busy } = useTaskDetail(ref(task()));
    expect(await save({ description: "x" })).toBe(false);
    expect(busy.value).toBe(false);
    // The user-facing error toast is the point of the catch — assert it fired, so
    // dropping notifications.error can't slip past green (whole-branch review, PR #76).
    expect(err).toHaveBeenCalledWith(expect.stringContaining("boom"));
  });

  it("save names the list when a move fails after the fields already saved", async () => {
    // The fields ARE persisted; only the list move didn't land — surface that
    // specifically rather than a bare error (final review, PR #76).
    mockIPC((cmd) => {
      if (cmd === "update_task") return updateTaskOk; // fields save OK
      if (cmd === "move_task_to_list") throw new Error("move boom"); // the move fails
      return undefined;
    });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    const { save } = useTaskDetail(ref(task({ list: "" })));
    expect(await save({ title: "New title", list: "Home" })).toBe(false);
    expect(err).toHaveBeenCalledWith(
      expect.stringContaining('Saved fields, but couldn\'t move to "Home"'),
    );
  });

  it("remove surfaces an error and releases the guard", async () => {
    mockIPC((cmd) => { if (cmd === "delete_task") throw new Error("nope"); return undefined; });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    const { remove, busy } = useTaskDetail(ref(task()));
    await remove();
    expect(busy.value).toBe(false);
    expect(err).toHaveBeenCalledWith(expect.stringContaining("nope"));
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
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    const { duplicate, busy } = useTaskDetail(ref(task()));
    await duplicate();
    expect(busy.value).toBe(false);
    expect(err).toHaveBeenCalledWith(expect.stringContaining("dupe fail"));
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
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    await expect(useTaskDetail(ref(task())).openInObsidian()).resolves.toBeUndefined();
    // Non-throwing is necessary but not sufficient — the failure must reach the user.
    expect(err).toHaveBeenCalledWith(expect.stringContaining("launch fail"));
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
      if (cmd === "update_task") return updateTaskOk;
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
      if (cmd === "update_task") return updateTaskOk;
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
      if (cmd === "update_task") return updateTaskOk;
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
      if (cmd === "update_task") return updateTaskOk;
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
  });

  it("save sends the scheduled (do) date and tags in the patch", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "update_task") return updateTaskOk;
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

  it("drops a now-non-current archived list from the picker after the task moves out of it", async () => {
    // The archived "Old" list is retained ONLY as the task's current list; once
    // the task moves to a visible list, the options must recompute and drop it,
    // so it can't be re-selected into a hidden list (Codex P2, PR #76).
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return ["Home", "Old"];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: ["Old"] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task({ list: "Old" }) } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual(["Home", "Old"]);
    // Task moves out of "Old" — the options must reactively drop the archived list.
    await wrapper.setProps({ task: task({ list: "Home" }) });
    expect(wrapper.findComponent(TaskListPicker).props("lists")).toEqual(["Home"]);
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

  it("disables the delete Cancel button while the delete is in flight", async () => {
    // A slow delete: Cancel must NOT stay clickable — hiding the confirm wouldn't
    // cancel the pending unlink (the delete still completes), so the UI must not
    // present a Cancel that does nothing (Codex P2, PR #76).
    let resolveDelete: (() => void) | undefined;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "delete_task")
        return new Promise<void>((r) => {
          resolveDelete = () => r();
        });
      return undefined;
    });
    const { useVaultsStore } = await import("../src/stores/vaults");
    useVaultsStore().view = "taskDetail";
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click"); // open confirm
    await wrapper.get('[data-testid="task-detail-delete-confirm"]').trigger("click"); // slow delete starts
    await new Promise((r) => setTimeout(r));
    expect((wrapper.find('[data-testid="task-detail-delete-cancel"]').element as HTMLButtonElement).disabled).toBe(true);
    expect((wrapper.find('[data-testid="task-detail-delete-confirm"]').element as HTMLButtonElement).disabled).toBe(true);
    resolveDelete?.();
    await new Promise((r) => setTimeout(r));
  });

  it("keeps the delete confirm open when Escape is pressed mid-delete", async () => {
    // The keyboard path must match the disabled Cancel button: Escape can't
    // cancel the in-flight unlink, so it must not hide the warning (Codex P2, PR #76).
    let resolveDelete: (() => void) | undefined;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "delete_task")
        return new Promise<void>((r) => {
          resolveDelete = () => r();
        });
      return undefined;
    });
    const { useVaultsStore } = await import("../src/stores/vaults");
    useVaultsStore().view = "taskDetail";
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
    await wrapper.get('[data-testid="task-detail-delete-confirm"]').trigger("click"); // slow delete
    await new Promise((r) => setTimeout(r));
    await wrapper
      .get('[data-testid="task-detail-delete-confirm"]')
      .trigger("keydown", { key: "Escape" });
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(true);
    resolveDelete?.();
    await new Promise((r) => setTimeout(r));
  });

  it("disables Open in Obsidian while a detail write is in flight", async () => {
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
    const open = () => wrapper.find('[data-testid="task-detail-open"]').element as HTMLButtonElement;
    expect(open().disabled).toBe(false);
    await wrapper.get('[data-testid="task-detail-duplicate"]').trigger("click"); // slow write
    await new Promise((r) => setTimeout(r));
    expect(open().disabled).toBe(true);
    resolveDup?.();
    await new Promise((r) => setTimeout(r));
    expect(open().disabled).toBe(false);
  });

  it("drives store.taskDetailBusy while a detail write is in flight (gates the header Back)", async () => {
    let resolveDup: (() => void) | undefined;
    mockIPC((cmd) =>
      cmd === "duplicate_task"
        ? new Promise<string>((r) => {
            resolveDup = () => r("/v/Tasks/t (copy).md");
          })
        : undefined,
    );
    const { useVaultsStore } = await import("../src/stores/vaults");
    const store = useVaultsStore();
    const { duplicate } = useTaskDetail(ref(task()));
    expect(store.taskDetailBusy).toBe(false);
    const pending = duplicate();
    await new Promise((r) => setTimeout(r));
    expect(store.taskDetailBusy).toBe(true);
    resolveDup?.();
    await pending;
    expect(store.taskDetailBusy).toBe(false);
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

  it("moves focus into the labelled region on mount (keeps keyboard focus in-panel)", async () => {
    // The list's title trigger unmounts when this opens, so without this focus
    // falls to <body>: a keyboard user restarts from the panel top and a screen
    // reader announces nothing (frontend review, PR #76).
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() }, attachTo: document.body });
    try {
      await new Promise((r) => setTimeout(r));
      const root = wrapper.get('[aria-label="Task detail"]');
      expect(root.attributes("role")).toBe("region");
      expect(root.attributes("tabindex")).toBe("-1");
      expect(document.activeElement).toBe(root.element); // focus landed in-panel, not on <body>
    } finally {
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("hides Save/Open/Duplicate while the delete confirm is open (a focused confirm)", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.find('[data-testid="task-detail-save"]').exists()).toBe(true);
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
    // Confirming → the verbs are gone; only Cancel + the irreversible Delete remain.
    expect(wrapper.find('[data-testid="task-detail-save"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-detail-open"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-detail-duplicate"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-detail-delete-confirm"]').exists()).toBe(true);
  });

  it("labels the confirm distinctly and restores focus to the trigger on cancel", async () => {
    // The confirm's accessible name must differ from the "Delete" trigger the
    // user just pressed, and cancelling must return focus to that trigger rather
    // than drop it to <body> (frontend review, PR #76).
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
      expect(wrapper.get('[data-testid="task-detail-delete-confirm"]').attributes("aria-label")).toBe(
        "Delete permanently",
      );
      await wrapper.get('[data-testid="task-detail-delete-cancel"]').trigger("click");
      await new Promise((r) => setTimeout(r)); // cancelConfirm awaits nextTick before focusing
      expect(document.activeElement).toBe(wrapper.get('[data-testid="task-detail-delete"]').element);
    } finally {
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("shows a Deleting… label on the confirm while the unlink is in flight", async () => {
    let resolveDelete: (() => void) | undefined;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "delete_task")
        return new Promise<void>((r) => {
          resolveDelete = () => r();
        });
      return undefined;
    });
    const { useVaultsStore } = await import("../src/stores/vaults");
    useVaultsStore().view = "taskDetail";
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: task() } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-delete"]').trigger("click");
    await wrapper.get('[data-testid="task-detail-delete-confirm"]').trigger("click"); // slow delete
    await new Promise((r) => setTimeout(r));
    expect(wrapper.get('[data-testid="task-detail-delete-confirm"]').text()).toBe("Deleting…");
    resolveDelete?.();
    await new Promise((r) => setTimeout(r));
  });

  it("re-seeds every draft when drilling from one task's detail to another", async () => {
    // The rendered FIELDS must follow the task, not just the store path.
    // openTaskDetail only swaps store.taskDetailTask; TaskDetail seeds its
    // seven draft refs once in setup with no watcher on props.task, so
    // without ActionPanel keying <TaskDetail> by path, drilling from one
    // task's detail to another would leave the OLD task's fields on screen
    // while useTaskDetail's toRef already points at the new path — and Save
    // would write the old values onto the newly opened task (Codex P1, PR #77).
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", title: "Parent", description: "pd" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Child", description: "cd" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, child];
      return undefined;
    });
    // Both dynamic imports resolved BEFORE any store mutation: a first-time
    // import of ActionPanel's whole component graph is slow enough under
    // istanbul instrumentation to open a real async gap, and something in that
    // window (module transform work, observed empirically) can leave
    // getActivePinia() pointing at a stale instance by the time mount() runs —
    // manifesting as ActionPanel rendering with a fresh default-state store
    // instead of the one just configured below. Pre-warming both imports
    // first keeps the store-setup -> mount critical section free of slow
    // awaits, which is the actual fix; it is not merely a speed optimization.
    const { useVaultsStore } = await import("../src/stores/vaults");
    const ActionPanel = (await import("../src/components/ActionPanel.vue")).default;
    const store = useVaultsStore();
    store.openTaskDetail(parent);
    const wrapper = mount(ActionPanel);
    await new Promise((r) => setTimeout(r));
    expect((wrapper.get('[data-testid="task-detail-title"]').element as HTMLInputElement).value).toBe("Parent");
    store.openTaskDetail(child); // drill through
    await new Promise((r) => setTimeout(r));
    expect((wrapper.get('[data-testid="task-detail-title"]').element as HTMLInputElement).value).toBe("Child");
    expect((wrapper.get('[data-testid="task-detail-description"]').element as HTMLTextAreaElement).value).toBe("cd");
    // Explicit timeout: mounting the full ActionPanel tree (the point of the
    // test — a props-only re-mount wouldn't exercise the :key remount at all)
    // reliably exceeds Vitest's 5s default under istanbul coverage
    // instrumentation even with the import pre-warming above.
  }, 15000);
});

describe("TaskDetail.vue Parent row", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("logs and leaves the Parent row at 'No parent' when list_tasks rejects", async () => {
    (logWarning as ReturnType<typeof vi.fn>).mockClear();
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") throw new Error("boom");
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.find('[data-testid="task-detail-parent-chip"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="task-detail-parent-change"]').text()).toBe("Set parent");
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("task detail: could not load the task set"),
    );
  });

  it("shows 'No parent' and a Set-parent control when the task has none", async () => {
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [self];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.find('[data-testid="task-detail-parent-chip"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-detail-parent-clear"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="task-detail-parent-change"]').text()).toBe("Set parent");
  });

  it("shows the parent's title as a chip with Change/Clear when a parent is resolved", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/Tasks/parent.md", title: "Parent Task" });
    const self = task({ vaultId: "v1", id: "s", parentId: "p", path: "/v1/Tasks/self.md", title: "Self" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, self];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.get('[data-testid="task-detail-parent-chip"]').text()).toBe("Parent Task");
    expect(wrapper.get('[data-testid="task-detail-parent-change"]').text()).toBe("Change");
    expect(wrapper.find('[data-testid="task-detail-parent-clear"]').exists()).toBe(true);
  });

  it("clicking the parent chip opens the parent's own detail view", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/Tasks/parent.md", title: "Parent Task" });
    const self = task({ vaultId: "v1", id: "s", parentId: "p", path: "/v1/Tasks/self.md", title: "Self" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, self];
      return undefined;
    });
    const { useVaultsStore } = await import("../src/stores/vaults");
    const store = useVaultsStore();
    const spy = vi.spyOn(store, "openTaskDetail");
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-parent-chip"]').trigger("click");
    expect(spy).toHaveBeenCalledWith(expect.objectContaining({ path: "/v1/Tasks/parent.md" }));
  });

  it("Clear sends clearParent and the row returns to 'No parent'", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/Tasks/parent.md", title: "Parent Task" });
    const self = task({ vaultId: "v1", id: "s", parentId: "p", path: "/v1/Tasks/self.md", title: "Self" });
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, self];
      if (cmd === "update_task") return { id: null, parentId: null, parentLink: null, idsEnabled: false };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-parent-clear"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    const call = calls.find((c) => c[0] === "update_task");
    expect(call[1].patch).toEqual({ clearParent: true });
    expect(wrapper.find('[data-testid="task-detail-parent-chip"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="task-detail-parent-change"]').text()).toBe("Set parent");
  });

  it("Change opens the picker; picking a task sets the parent and closes it", async () => {
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    const other = task({ vaultId: "v1", id: "o", path: "/v1/Tasks/other.md", title: "Other Task" });
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [self, other];
      if (cmd === "update_task") return { id: null, parentId: "o", parentLink: "[[Tasks/other]]", idsEnabled: false };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-parent-change"]').trigger("click");
    expect(wrapper.find('[data-testid="task-parent-picker-filter"]').exists()).toBe(true);
    await wrapper.get(`[data-testid="task-parent-picker-option-${other.path}"]`).trigger("click");
    await new Promise((r) => setTimeout(r));
    const call = calls.find((c) => c[0] === "update_task");
    expect(call[1].patch).toEqual({ parentPath: "/v1/Tasks/other.md" });
    expect(wrapper.find('[data-testid="task-parent-picker-filter"]').exists()).toBe(false); // picker closed
    expect(wrapper.get('[data-testid="task-detail-parent-chip"]').text()).toBe("Other Task");
  });

  it("filters the picker's options by title", async () => {
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    const other = task({ vaultId: "v1", id: "o", path: "/v1/Tasks/other.md", title: "Other Task" });
    const groceries = task({ vaultId: "v1", id: "g", path: "/v1/Tasks/groceries.md", title: "Groceries" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [self, other, groceries];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-parent-change"]').trigger("click");
    await wrapper.get('[data-testid="task-parent-picker-filter"]').setValue("other");
    expect(wrapper.find(`[data-testid="task-parent-picker-option-${other.path}"]`).exists()).toBe(true);
    expect(wrapper.find(`[data-testid="task-parent-picker-option-${groceries.path}"]`).exists()).toBe(false);
  });

  it("disables self and its descendants in the picker as invalid parent choices", async () => {
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    const kid = task({ vaultId: "v1", id: "k", parentId: "s", path: "/v1/Tasks/kid.md", title: "Kid" });
    const other = task({ vaultId: "v1", id: "o", path: "/v1/Tasks/other.md", title: "Other Task" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [self, kid, other];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-parent-change"]').trigger("click");
    expect(
      (wrapper.get(`[data-testid="task-parent-picker-option-${self.path}"]`).element as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (wrapper.get(`[data-testid="task-parent-picker-option-${kid.path}"]`).element as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (wrapper.get(`[data-testid="task-parent-picker-option-${other.path}"]`).element as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it("focuses the picker's search input when Change is clicked", async () => {
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [self];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self }, attachTo: document.body });
    try {
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-detail-parent-change"]').trigger("click");
      await new Promise((r) => setTimeout(r));
      expect(document.activeElement).toBe(wrapper.get('[data-testid="task-parent-picker-filter"]').element);
    } finally {
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("Escape closes the picker without writing, swallowing the key and returning focus to Change", async () => {
    const self = task({ vaultId: "v1", id: "s", path: "/v1/Tasks/self.md", title: "Self" });
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [self];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self }, attachTo: document.body });
    const seen: string[] = [];
    const onDocKeydown = (e: Event) => seen.push((e as KeyboardEvent).key);
    document.addEventListener("keydown", onDocKeydown);
    try {
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-detail-parent-change"]').trigger("click");
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-parent-picker-filter"]').trigger("keydown", { key: "Escape" });
      await new Promise((r) => setTimeout(r));
      expect(seen).not.toContain("Escape"); // swallowed, never reaches the panel's own handler
      expect(wrapper.find('[data-testid="task-parent-picker-filter"]').exists()).toBe(false);
      expect(calls).not.toContain("update_task");
      expect(document.activeElement).toBe(wrapper.get('[data-testid="task-detail-parent-change"]').element);
    } finally {
      document.removeEventListener("keydown", onDocKeydown);
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("adopts the bootstrap write's fresh ids so the Parent row resolves without a remount", async () => {
    // The headline path of the whole subtasks slice: a vault that starts with
    // Task IDs OFF (every fixture row's id is null, the task() default) and
    // the FIRST parent assignment bootstraps ids for the whole vault. On that
    // path setParent skips its cheap two-row patch and relies entirely on
    // reload() — this guards that reload() actually ADOPTS the freshly
    // loaded ids onto the current task instead of discarding them in favor of
    // the pre-write object it already held (the bug: Parent stayed "No
    // parent" until the view was remounted even though the write succeeded).
    const self = task({ vaultId: "v1", path: "/v1/Tasks/self.md", title: "Self" });
    const other = task({ vaultId: "v1", path: "/v1/Tasks/other.md", title: "Other Task" });
    let listTasksCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") {
        listTasksCalls += 1;
        // First call: the initial mount load — still IDs-off, matching the
        // vault on disk. Second call: the reload triggered by setParent's
        // idsEnabled bootstrap — the real backend would have just stamped
        // both rows on disk by the time this fires.
        return listTasksCalls === 1
          ? [self, other]
          : [
              { ...self, id: "sid", parentId: "oid" },
              { ...other, id: "oid" },
            ];
      }
      if (cmd === "update_task") {
        return { id: "sid", parentId: "oid", parentLink: "[[Tasks/other]]", idsEnabled: true };
      }
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-parent-change"]').trigger("click");
    await wrapper.get(`[data-testid="task-parent-picker-option-${other.path}"]`).trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(wrapper.get('[data-testid="task-detail-parent-chip"]').text()).toBe("Other Task");
  });

  it("disables the parent Change/Clear controls while a DIFFERENT detail write is in flight (shared busy guard)", async () => {
    // Same invariant as the composable-level test, checked at the rendered
    // control: Change/Clear must use useTaskDetail's OWN busy ref, not an
    // independent one, or a slow duplicate/save wouldn't disable them.
    let resolveDup: (() => void) | undefined;
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/Tasks/parent.md", title: "Parent Task" });
    const self = task({ vaultId: "v1", id: "s", parentId: "p", path: "/v1/Tasks/self.md", title: "Self" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, self];
      if (cmd === "duplicate_task")
        return new Promise<string>((r) => {
          resolveDup = () => r("/v1/Tasks/self (copy).md");
        });
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: self } });
    await new Promise((r) => setTimeout(r));
    const change = () => wrapper.get('[data-testid="task-detail-parent-change"]').element as HTMLButtonElement;
    const clear = () => wrapper.get('[data-testid="task-detail-parent-clear"]').element as HTMLButtonElement;
    expect(change().disabled).toBe(false);
    expect(clear().disabled).toBe(false);
    await wrapper.get('[data-testid="task-detail-duplicate"]').trigger("click"); // slow write starts
    await new Promise((r) => setTimeout(r));
    expect(change().disabled).toBe(true);
    expect(clear().disabled).toBe(true);
    resolveDup?.();
    await new Promise((r) => setTimeout(r));
    expect(change().disabled).toBe(false);
    expect(clear().disabled).toBe(false);
  });
});

// Task 9: the Subtasks section + Add subtask. Brief's Step 1 + Step 2 tests
// reproduced verbatim, followed by supplementary regression coverage for
// pieces the brief describes but doesn't enumerate a test for (the checkbox
// toggle, the Escape/busy-guard/failure-retains-draft behavior expected of
// every other write control on this surface, and the disclosure note).
describe("TaskDetail.vue Subtasks section", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("lists children with a done/total progress line", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", list: "Work" });
    const kids = [
      task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md", title: "One", done: true, status: "done" }),
      task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md", title: "Two" }),
    ];
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, ...kids];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("1 / 2");
    expect(wrapper.findAll('[data-testid="task-detail-subtask"]')).toHaveLength(2);
  });

  it("Add subtask creates a child with this task as the parent and inherits its List", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", list: "Work" });
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent];
      if (cmd === "add_task") return { ...task({ vaultId: "v1", id: "n", parentId: "p", path: "/v1/n.md" }), idsEnabled: false };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("New child");
    await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
    await new Promise((r) => setTimeout(r));
    const add = calls.find((c) => c[0] === "add_task");
    expect(add[1].parentPath).toBe("/v1/p.md");
    expect(add[1].list).toBe("Work"); // inherits the parent's List
  });

  it("clicking a child's title drills into that child's detail", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Kid" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, child];
      return undefined;
    });
    const { useVaultsStore } = await import("../src/stores/vaults");
    const store = useVaultsStore();
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-subtask-open"]').trigger("click");
    expect(store.taskDetailTask?.path).toBe("/v1/c.md");
  });

  it("stamps the current task's cached id from the created child (IDs-off)", async () => {
    // First hierarchy op in an IDs-off vault: the backend enables IDs and stamps
    // the PARENT, returning the child with parentId. Without copying that onto the
    // parent's cached row, `children` compares against a still-null id and the new
    // subtask (and the progress line) stay invisible until a reload.
    const parent = task({ vaultId: "v1", id: null, path: "/v1/p.md", title: "Parent", list: "" });
    let listCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      // idsEnabled: true takes the RELOAD branch (Task 8), so list_tasks must
      // return the post-enable state on the second call — the stamped parent AND
      // the created child. Returning the original id-less parent forever would
      // make these assertions unreachable (Codex P2, PR #77).
      if (cmd === "list_tasks") {
        listCalls += 1;
        return listCalls === 1
          ? [parent]
          : [{ ...parent, id: "pid" }, task({ vaultId: "v1", id: "cid", parentId: "pid", path: "/v1/c.md", title: "Kid" })];
      }
      if (cmd === "add_task") return { ...task({ vaultId: "v1", id: "cid", parentId: "pid", path: "/v1/c.md", title: "Kid" }), idsEnabled: true };
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("Kid");
    await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
    await new Promise((r) => setTimeout(r));
    expect(listCalls).toBeGreaterThan(1); // the reload branch ran
    expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("0 / 1");
    expect(wrapper.findAll('[data-testid="task-detail-subtask"]')).toHaveLength(1);
  });

  it("stamps the current task's cached id from the created child even when Task IDs were already enabled (cheap-patch branch, no reload)", async () => {
    // A DIFFERENT bootstrap shape than the test above, isolating the copy step
    // from the reload branch: Task IDs are already ON for the vault (idsEnabled:
    // false — this call didn't flip anything), but this parent was hand-authored
    // and never got an id of its own. add_subtask's shared resolve-the-parent
    // path stamps the parent's id unconditionally (core's phase 3a — it runs
    // whether or not THIS call is what enabled ids), so the response still
    // carries the freshly-stamped parentId — but idsEnabled is false, so the
    // CHEAP-PATCH branch runs, never a reload. list_tasks is mocked to keep
    // returning the stale id-less parent forever, so if the create path doesn't
    // copy parentId onto the cached row itself, nothing else here ever will.
    const parent = task({ vaultId: "v1", id: null, path: "/v1/p.md", title: "Parent", list: "" });
    let listCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") {
        listCalls += 1;
        return [parent];
      }
      if (cmd === "add_task") {
        return { ...task({ vaultId: "v1", id: "cid", parentId: "pid", path: "/v1/c.md", title: "Kid" }), idsEnabled: false };
      }
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("Kid");
    await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
    await new Promise((r) => setTimeout(r));
    expect(listCalls).toBe(1); // no reload — the cheap-patch branch ran
    expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("0 / 1");
    expect(wrapper.findAll('[data-testid="task-detail-subtask"]')).toHaveLength(1);
    // Cleared on success, same as every other add-flow draft in this codebase.
    expect((wrapper.get('[data-testid="task-detail-add-subtask"]').element as HTMLInputElement).value).toBe("");
  });

  it("surfaces the Task-IDs-enabled note when Add subtask bootstraps ids for the vault", async () => {
    const parent = task({ vaultId: "v1", id: null, path: "/v1/p.md" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent];
      if (cmd === "add_task") return { ...task({ vaultId: "v1", id: "cid", parentId: "pid", path: "/v1/c.md", title: "Kid" }), idsEnabled: true };
      return undefined;
    });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const notify = vi.spyOn(useNotificationsStore(), "notify");
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("Kid");
    await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
    await new Promise((r) => setTimeout(r));
    expect(notify).toHaveBeenCalledWith("success", expect.stringContaining("Task IDs"), expect.anything());
  });

  it("does NOT surface the Task-IDs-enabled note on an ordinary add (ids already on, nothing bootstrapped)", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent];
      if (cmd === "add_task") return { ...task({ vaultId: "v1", id: "n", parentId: "p", path: "/v1/n.md" }), idsEnabled: false };
      return undefined;
    });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const notify = vi.spyOn(useNotificationsStore(), "notify");
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("Kid");
    await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
    await new Promise((r) => setTimeout(r));
    expect(notify).not.toHaveBeenCalled();
  });

  it("toggles a child's status via the shared busy guard, updating the progress line", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Kid" });
    const calls: any[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, child];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("0 / 1");
    await wrapper.get('[data-testid="task-detail-subtask-checkbox"]').trigger("change");
    await new Promise((r) => setTimeout(r));
    const call = calls.find((c) => c[0] === "set_task_status");
    expect(call[1]).toEqual({ id: "v1", path: "/v1/c.md", status: "done" });
    expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("1 / 1");
  });

  it("reverts the optimistic toggle and surfaces an error when set_task_status fails", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Kid" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, child];
      if (cmd === "set_task_status") throw new Error("locked");
      return undefined;
    });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-subtask-checkbox"]').trigger("change");
    await new Promise((r) => setTimeout(r));
    expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("0 / 1"); // reverted
    expect(err).toHaveBeenCalledWith(expect.stringContaining("locked"));
  });

  it("Escape clears the add-subtask draft without creating a task, and stops it bubbling to the panel", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent];
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent }, attachTo: document.body });
    const seen: string[] = [];
    const onDocKeydown = (e: Event) => seen.push((e as KeyboardEvent).key);
    document.addEventListener("keydown", onDocKeydown);
    try {
      await new Promise((r) => setTimeout(r));
      await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("Half-typed");
      await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Escape" });
      expect(seen).not.toContain("Escape"); // swallowed, never reaches the panel's own close handler
      expect((wrapper.get('[data-testid="task-detail-add-subtask"]').element as HTMLInputElement).value).toBe("");
      expect(calls).not.toContain("add_task");
    } finally {
      document.removeEventListener("keydown", onDocKeydown);
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("keeps the add-subtask draft and surfaces an error when the create fails", async () => {
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent];
      if (cmd === "add_task") throw new Error("disk full");
      return undefined;
    });
    const { useNotificationsStore } = await import("../src/stores/notifications");
    const err = vi.spyOn(useNotificationsStore(), "error");
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("Kid");
    await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
    await new Promise((r) => setTimeout(r));
    expect((wrapper.get('[data-testid="task-detail-add-subtask"]').element as HTMLInputElement).value).toBe("Kid");
    expect(err).toHaveBeenCalledWith(expect.stringContaining("disk full"));
    expect((wrapper.get('[data-testid="task-detail-add-subtask"]').element as HTMLInputElement).disabled).toBe(false);
  });

  it("disables the subtask controls while a DIFFERENT detail write is in flight (shared busy guard)", async () => {
    let resolveDup: (() => void) | undefined;
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Kid" });
    mockIPC((cmd) => {
      if (cmd === "list_task_lists") return [];
      if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
      if (cmd === "list_tasks") return [parent, child];
      if (cmd === "duplicate_task")
        return new Promise<string>((r) => {
          resolveDup = () => r("/v1/p (copy).md");
        });
      return undefined;
    });
    const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
    const wrapper = mount(TaskDetail, { props: { task: parent } });
    await new Promise((r) => setTimeout(r));
    const addInput = () => wrapper.get('[data-testid="task-detail-add-subtask"]').element as HTMLInputElement;
    const checkbox = () => wrapper.get('[data-testid="task-detail-subtask-checkbox"]').element as HTMLInputElement;
    const openBtn = () => wrapper.get('[data-testid="task-detail-subtask-open"]').element as HTMLButtonElement;
    expect(addInput().disabled).toBe(false);
    await wrapper.get('[data-testid="task-detail-duplicate"]').trigger("click"); // slow write starts
    await new Promise((r) => setTimeout(r));
    expect(addInput().disabled).toBe(true);
    expect(checkbox().disabled).toBe(true);
    expect(openBtn().disabled).toBe(true);
    resolveDup?.();
    await new Promise((r) => setTimeout(r));
    expect(addInput().disabled).toBe(false);
  });
});
