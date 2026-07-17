import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TaskListSettings from "../src/components/TaskListSettings.vue";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  // TaskListSettings now uses useAutosave → the settingsStatus store.
  setActivePinia(createPinia());
});
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
    const rows = wrapper
      .findAll('[data-testid="list-order-row"]')
      .map((r) => r.text().replace(/[↑↓]/g, "").trim());
    // listOrder ("Next") first, the rest alphabetical.
    expect(rows).toEqual(["Next", "Inbox", "Waiting"]);
    expect(wrapper.get('[data-testid="default-list"]').text()).toContain("Inbox");
  });

  it("does not save on mount", async () => {
    const { calls } = mountSettings();
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_task_lists_config")).toBe(false);
  });

  it("saves the new order immediately after a reorder (no Save button)", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-lists-save"]').exists()).toBe(false);
    await wrapper.get('[data-testid="list-order-up-2"]').trigger("click"); // Waiting up one
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toEqual({
      id: "v1",
      defaultList: "Inbox",
      listOrder: ["Next", "Waiting", "Inbox"],
      archivedLists: [],
    });
  });

  it("saves immediately when the default list changes", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    await wrapper.get('[data-testid="default-list"]').trigger("click");
    await flushPromises();
    (document.body.querySelector('[data-testid="default-list-option-Waiting"]') as HTMLElement).click();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toMatchObject({
      defaultList: "Waiting",
    });
  });

  it("clearing the default sends null (the tasks root)", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    await wrapper.get('[data-testid="default-list"]').trigger("click");
    await flushPromises();
    (document.body.querySelector('[data-testid="default-list-option-"]') as HTMLElement).click();
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
    await wrapper.get('[data-testid="list-order-up-2"]').trigger("click");
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

  it("lists archived lists and unarchives one, saving the shrunken set", async () => {
    const { wrapper, calls } = mountSettings({
      get_tasks_config: () => ({ tasksFolder: null, defaultList: "Inbox", listOrder: ["Next"], archivedLists: ["Old", "Stale"] }),
    });
    await flushPromises();
    // Both archived lists render with an Unarchive control.
    expect(wrapper.findAll('[data-testid="archived-list-row"]')).toHaveLength(2);
    await wrapper.get('[data-testid="unarchive-Old"]').trigger("click");
    await flushPromises();
    const save = [...calls].reverse().find((c) => c.cmd === "set_task_lists_config");
    // The save carries the remaining archived set (Old removed) alongside the
    // preserved default/order.
    expect((save?.args as { archivedLists: string[] }).archivedLists).toEqual(["Stale"]);
    // The unarchived row disappears from the list.
    expect(wrapper.findAll('[data-testid="archived-list-row"]')).toHaveLength(1);
  });

  it("shows no archived section when nothing is archived", async () => {
    const { wrapper } = mountSettings(); // default config carries no archivedLists
    await flushPromises();
    expect(wrapper.findAll('[data-testid="archived-list-row"]')).toHaveLength(0);
  });

  it("excludes archived lists from the default-list picker options (Codex re-review)", async () => {
    const { wrapper } = mountSettings({
      get_tasks_config: () => ({ tasksFolder: null, defaultList: "Inbox", listOrder: ["Next"], archivedLists: ["Waiting"] }),
    });
    await flushPromises();
    await wrapper.get('[data-testid="default-list"]').trigger("click");
    await flushPromises();
    // A visible list is offered; the archived "Waiting" is not a pickable
    // default — otherwise unpicked adds would land in a hidden list.
    expect(document.body.querySelector('[data-testid="default-list-option-Next"]')).not.toBeNull();
    expect(document.body.querySelector('[data-testid="default-list-option-Waiting"]')).toBeNull();
  });

  it("hides archived lists from the reorder rows but keeps their order slot", async () => {
    // An archived list rendered as an unmarked, fully-reorderable row right
    // above its own Unarchive row — the same list twice in one card, and
    // interactive while hidden from every picker/grouping. The reorder rows
    // now show only visible lists; the archived name KEEPS its slot in the
    // persisted listOrder so unarchiving restores its position instead of
    // dumping it at the alphabetical tail.
    const { wrapper, calls } = mountSettings({
      list_task_lists: () => ["Inbox", "Next", "Old", "Waiting"], // Old's folder still exists
      get_tasks_config: () => ({ tasksFolder: null, defaultList: null, listOrder: ["Next", "Old", "Waiting"], archivedLists: ["Old"] }),
    });
    await flushPromises();
    const rows = wrapper
      .findAll('[data-testid="list-order-row"]')
      .map((r) => r.text().replace(/[↑↓]/g, "").trim());
    expect(rows).toEqual(["Next", "Waiting", "Inbox"]); // "Old" not offered for reorder
    // Move "Waiting" (visible row 1) up: it swaps with "Next" AROUND the
    // hidden "Old" slot — the saved order keeps Old exactly where it was.
    await wrapper.get('[data-testid="list-order-up-1"]').trigger("click");
    await flushPromises();
    const save = calls.find((c) => c.cmd === "set_task_lists_config");
    expect((save?.args as { listOrder: string[] }).listOrder).toEqual(["Waiting", "Old", "Next", "Inbox"]);
  });

  it("clears an archived default on the next save (Codex re-review)", async () => {
    const { wrapper, calls } = mountSettings({
      get_tasks_config: () => ({ tasksFolder: null, defaultList: "Old", listOrder: ["Next"], archivedLists: ["Old", "Stale"] }),
    });
    await flushPromises();
    // Unarchiving a DIFFERENT list triggers a save; the still-archived default
    // ("Old") must normalize to null so unpicked adds stop landing in it.
    await wrapper.get('[data-testid="unarchive-Stale"]').trigger("click");
    await flushPromises();
    const save = [...calls].reverse().find((c) => c.cmd === "set_task_lists_config");
    expect(save?.args).toMatchObject({ defaultList: null, archivedLists: ["Old"] });
  });
});
