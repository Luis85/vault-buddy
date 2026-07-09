import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Tasks from "../src/components/Tasks.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import type { TaskItem } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const sample: TaskItem[] = [
  { path: "C:/v/Tasks/2026-07-08-b.md", title: "B open", status: "new", created: "2026-07-08", done: false },
  { path: "C:/v/Tasks/2026-07-06-a.md", title: "A done", status: "done", created: "2026-07-06", done: true },
];

function mountView(handlers: Partial<Record<string, (args: unknown) => unknown>> = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  // Per-item clone, not just a new array: toggle() mutates task.done/status in
  // place on the object it's handed, and sample's objects are shared across
  // tests — a shallow array copy would leak state between tests.
  let list = sample.map((t) => ({ ...t }));
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "get_tasks_config") return { tasksFolder: null };
    if (cmd === "list_tasks") return list;
    if (cmd === "add_task") {
      const created = { path: "C:/v/Tasks/2026-07-08-new.md", title: (args as { title: string }).title, status: "new", created: "2026-07-08", done: false };
      list = [created, ...list];
      return created;
    }
    if (cmd === "set_task_status") return null;
    if (cmd === "set_tasks_config") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: "v1" } });
  return { wrapper, calls };
}

describe("Tasks", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("loads config and tasks for the vault on mount", async () => {
    const { calls } = mountView();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "list_tasks")).toEqual({ cmd: "list_tasks", args: { id: "v1" } });
    expect(calls.find((c) => c.cmd === "get_tasks_config")).toBeTruthy();
  });

  it("renders open tasks before done ones", async () => {
    const { wrapper } = mountView();
    await flushPromises();
    const rows = wrapper.findAll('[data-testid="task-row"]');
    expect(rows[0].text()).toContain("B open");
    expect(rows[1].text()).toContain("A done");
  });

  it("adds a task from the input", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-input"]').setValue("Ship it");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({ cmd: "add_task", args: { id: "v1", title: "Ship it" } });
    expect(wrapper.text()).toContain("Ship it");
  });

  it("toggles a task via set_task_status", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    const call = calls.find((c) => c.cmd === "set_task_status");
    expect(call?.args).toMatchObject({ id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", done: true });
  });

  it("ignores a re-toggle while the row's write is still in flight", async () => {
    // A slow set_task_status: the second change on the same row must not fire a
    // second concurrent write (which could land out of order vs the first).
    let resolve: (() => void) | undefined;
    const { wrapper, calls } = mountView({
      set_task_status: () => new Promise<null>((r) => {
        resolve = () => r(null);
      }),
    });
    await flushPromises();
    const checkbox = wrapper.get('[data-testid="task-checkbox"]');
    await checkbox.trigger("change"); // first toggle — write pending
    await checkbox.trigger("change"); // re-toggle while pending — must be ignored
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "set_task_status")).toHaveLength(1);
    expect((checkbox.element as HTMLInputElement).disabled).toBe(true);
    resolve?.();
    await flushPromises();
    expect((checkbox.element as HTMLInputElement).disabled).toBe(false);
  });

  it("reverts the checkbox and notifies on toggle failure", async () => {
    const notifications = useNotificationsStore();
    const { wrapper } = mountView({
      set_task_status: () => {
        throw new Error("disk full");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    const checkbox = wrapper.findAll('[data-testid="task-checkbox"]')[0];
    expect((checkbox.element as HTMLInputElement).checked).toBe(false);
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });

  it("does not add a task when the title is empty or whitespace", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-input"]').setValue("   ");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toBeUndefined();
  });

  it("submits a new task on Enter", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-input"]').setValue("Ship it");
    await wrapper.get('[data-testid="task-input"]').trigger("keydown.enter");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({ cmd: "add_task", args: { id: "v1", title: "Ship it" } });
  });

  it("saves a new tasks folder", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Inbox/Tasks");
    await wrapper.get('[data-testid="tasks-folder-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
      cmd: "set_tasks_config",
      args: { id: "v1", tasksFolder: "Inbox/Tasks" },
    });
  });
});
