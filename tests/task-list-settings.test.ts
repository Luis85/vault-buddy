import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

import TaskListSettings from "../src/components/TaskListSettings.vue";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  clearMocks();
  document.body.innerHTML = "";
});

function mountSettings(handlers: Partial<Record<string, (args: unknown) => unknown>> = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "get_tasks_config")
      return { tasksFolder: null, defaultList: "Inbox", listOrder: ["Next"] };
    if (cmd === "list_task_lists") return ["Inbox", "Next", "Waiting"];
    if (cmd === "set_task_lists_config") return null;
  });
  active = mount(TaskListSettings, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TaskListSettings", () => {
  it("loads the vault's lists in effective order and seeds the default", async () => {
    const { wrapper } = mountSettings();
    await flushPromises();
    const rows = wrapper.findAll('[data-testid="list-order-row"]').map((r) => r.text().replace(/[↑↓]/g, "").trim());
    // listOrder ("Next") first, the rest alphabetical.
    expect(rows).toEqual(["Next", "Inbox", "Waiting"]);
    expect(wrapper.get('[data-testid="default-list"]').text()).toContain("Inbox");
  });

  it("moves a list up and saves the full settings object", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    await wrapper.get('[data-testid="list-order-up-2"]').trigger("click"); // Waiting up one
    await wrapper.get('[data-testid="task-lists-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toEqual({
      id: "v1",
      defaultList: "Inbox",
      listOrder: ["Next", "Waiting", "Inbox"],
    });
    expect(wrapper.text()).toContain("Saved");
  });

  it("clearing the default sends null (the tasks root)", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    await wrapper.get('[data-testid="default-list"]').trigger("click");
    await flushPromises();
    (document.body.querySelector('[data-testid="default-list-option-"]') as HTMLElement).click();
    await flushPromises();
    await wrapper.get('[data-testid="task-lists-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toMatchObject({
      defaultList: null,
    });
  });

  it("shows a field-level error when the save fails", async () => {
    const { wrapper } = mountSettings({
      set_task_lists_config: () => {
        throw new Error("List path must stay inside the tasks folder");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-lists-save"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="task-lists-error"]').text()).toContain("inside the tasks folder");
  });

  it("offers a hint when the vault has no lists yet", async () => {
    const { wrapper } = mountSettings({
      list_task_lists: () => [],
      get_tasks_config: () => ({ tasksFolder: null, defaultList: null, listOrder: [] }),
    });
    await flushPromises();
    expect(wrapper.text()).toContain("No lists yet");
    expect(wrapper.findAll('[data-testid="list-order-row"]')).toHaveLength(0);
  });
});
