import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import TasksConfigTab from "../src/components/TasksConfigTab.vue";
import { useSettingsStatusStore } from "../src/stores/settingsStatus";

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

function mountTab(
  opts: {
    tasksFolder?: string | null;
    onGet?: () => unknown;
    onSet?: (a: unknown) => unknown;
    onListLists?: () => unknown;
    onSetLists?: (a: unknown) => unknown;
    onSetId?: (a: unknown) => unknown;
    onSetTemplate?: (a: unknown) => unknown;
  } = {},
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_tasks_config")
      return opts.onGet
        ? opts.onGet()
        : { tasksFolder: opts.tasksFolder ?? null, defaultList: null, listOrder: [], taskIdEnabled: false, taskIdProperty: "task-id" };
    if (cmd === "list_task_lists") return opts.onListLists?.() ?? [];
    if (cmd === "set_tasks_config") return opts.onSet?.(args) ?? null;
    if (cmd === "set_task_lists_config") return opts.onSetLists?.(args) ?? null;
    if (cmd === "set_task_id_config") return opts.onSetId?.(args) ?? null;
    if (cmd === "set_task_template_config") return opts.onSetTemplate?.(args) ?? null;
  });
  active = mount(TasksConfigTab, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TasksConfigTab", () => {
  it("loads the tasks folder from disk", async () => {
    const { wrapper } = mountTab({ tasksFolder: "Inbox/Tasks" });
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="tasks-folder-input"]').element.value).toBe("Inbox/Tasks");
  });

  it("does not save on mount", async () => {
    const { calls } = mountTab({ tasksFolder: "Inbox/Tasks" });
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_tasks_config")).toBe(false);
  });

  it("debounces a folder edit and trims on save", async () => {
    const { wrapper, calls } = mountTab({ tasksFolder: "Tasks" });
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("  Work/Tasks  ");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")?.args).toEqual({ id: "v1", tasksFolder: "Work/Tasks" });
  });

  it("empties to null on save", async () => {
    const { wrapper, calls } = mountTab({ tasksFolder: "Tasks" });
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")?.args).toEqual({ id: "v1", tasksFolder: null });
  });

  it("remounts the lists card only when the persisted folder changes", async () => {
    let lists = ["OldList", "OldToo"];
    const { wrapper, calls } = mountTab({ tasksFolder: "Tasks", onListLists: () => lists });
    await flushPromises();
    const cardLoads = () => calls.filter((c) => c.cmd === "list_task_lists").length;
    const before = cardLoads();
    expect(before).toBeGreaterThan(0);
    lists = ["NewList", "NewToo"];
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Other/Tasks");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(cardLoads()).toBe(before + 1); // remounted → re-read the lists
    // A second save with the folder unchanged does not remount.
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Other/Tasks");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(cardLoads()).toBe(before + 1);
  });

  it("hides the lists card while a tasks-folder change is pending, then restores it after the save (Codex #55)", async () => {
    // Regression: with autosave the lists card used to stay mounted while a
    // folder edit was still debounced/in-flight, so a default/order pick in
    // that window persisted old-root list names onto the about-to-change root.
    const { wrapper } = mountTab({ tasksFolder: "Tasks", onListLists: () => ["Inbox", "Next"] });
    await flushPromises();
    expect(wrapper.text()).toContain("Task lists"); // card visible initially
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Work/Tasks");
    await flushPromises();
    // The folder differs from what's persisted → the card is gone, replaced by
    // a hint, so no stale list-preference save can land against the old root.
    expect(wrapper.text()).not.toContain("Task lists");
    expect(wrapper.find('[data-testid="tasks-lists-pending"]').exists()).toBe(true);
    // Once the folder save lands, the card remounts against the new root.
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(wrapper.text()).toContain("Task lists");
    expect(wrapper.find('[data-testid="tasks-lists-pending"]').exists()).toBe(false);
  });

  it("disables the tasks-folder input while a list save is in flight (Codex #55 follow-up)", async () => {
    // The v-if gate stops NEW list edits once the folder diverges, but an
    // already-started set_task_lists_config survives the card unmount. Fence
    // the other direction too: while a list save is in flight the folder input
    // is disabled, so a folder change can't overlap it and land old-root list
    // preferences onto the new root.
    let resolveListSave!: () => void;
    const { wrapper } = mountTab({
      tasksFolder: "Tasks",
      onListLists: () => ["Inbox", "Next"],
      onSetLists: () => new Promise<void>((r) => (resolveListSave = r)),
    });
    await flushPromises();
    const folderDisabled = () =>
      wrapper.get<HTMLInputElement>('[data-testid="tasks-folder-input"]').element.disabled;
    expect(folderDisabled()).toBe(false);
    // Reorder → a list save starts and hangs.
    await wrapper.get('[data-testid="list-order-up-1"]').trigger("click");
    await flushPromises();
    expect(folderDisabled()).toBe(true);
    // Once it resolves, the folder is editable again.
    resolveListSave();
    await flushPromises();
    expect(folderDisabled()).toBe(false);
  });

  it("clears a failed list save's error from the header when the lists card unmounts on a folder change (Codex #55)", async () => {
    // Regression: a failed list save left its owner in the shared header's
    // errorsByOwner; editing the folder unmounted the card, so the stale error
    // stuck (the remount got a new owner) until a view change. Unmount must
    // retire the owner.
    const status = useSettingsStatusStore();
    const { wrapper } = mountTab({
      tasksFolder: "Tasks",
      onListLists: () => ["Inbox", "Next"],
      onSetLists: () => {
        throw "bad list path";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="list-order-up-1"]').trigger("click"); // list save fails
    await flushPromises();
    expect(status.state).toBe("error");
    // Editing the folder unmounts the lists card → its failed owner is retired.
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Work/Tasks");
    await flushPromises();
    expect(status.state).not.toBe("error");
  });

  it("shows a save error inline", async () => {
    const { wrapper } = mountTab({
      tasksFolder: "Tasks",
      onSet: () => {
        throw "Configured tasks folder must stay inside the vault";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("../x");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(wrapper.get('[data-testid="tasks-folder-error"]').text()).toContain("inside the vault");
  });

  it("shows a load error (no folder input) but still renders the lists card when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw "config unreadable";
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="tasks-load-error"]').text()).toContain("config unreadable");
    expect(wrapper.find('[data-testid="tasks-folder-input"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("Task lists"); // TaskListSettings still mounted
  });

  it("enabling task ids and setting a property saves via set_task_id_config", async () => {
    const saved: unknown[] = [];
    const { wrapper } = mountTab({ onSetId: (a) => (saved.push(a), null) });
    await flushPromises();
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(true);
    await flushPromises();
    await wrapper.get('[data-testid="task-id-property"]').setValue("uid");
    await wrapper.get('[data-testid="task-id-property"]').trigger("blur");
    await flushPromises();
    expect(saved).toContainEqual({ id: "v1", enabled: true, property: "uid" });
  });

  it("trims a padded property value on save", async () => {
    const saved: unknown[] = [];
    const { wrapper } = mountTab({ onSetId: (a) => (saved.push(a), null) });
    await flushPromises();
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(true);
    await flushPromises();
    await wrapper.get('[data-testid="task-id-property"]').setValue("  uid  ");
    await wrapper.get('[data-testid="task-id-property"]').trigger("blur");
    await flushPromises();
    expect(saved).toContainEqual({ id: "v1", enabled: true, property: "uid" });
  });

  it("hides the property field until task ids are enabled", async () => {
    const { wrapper } = mountTab();
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="task-id-enabled"]').element.checked).toBe(false);
    expect(wrapper.find('[data-testid="task-id-property"]').exists()).toBe(false);
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(true);
    await flushPromises();
    expect(wrapper.find('[data-testid="task-id-property"]').exists()).toBe(true);
  });

  it("shows the default property name as a placeholder rather than pre-filling it", async () => {
    const { wrapper } = mountTab({
      onGet: () => ({ tasksFolder: null, defaultList: null, listOrder: [], taskIdEnabled: true, taskIdProperty: "task-id" }),
    });
    await flushPromises();
    const input = wrapper.get<HTMLInputElement>('[data-testid="task-id-property"]');
    expect(input.element.value).toBe("");
    expect(input.element.placeholder).toBe("task-id");
  });

  it("pre-fills a non-default persisted property name", async () => {
    const { wrapper } = mountTab({
      onGet: () => ({ tasksFolder: null, defaultList: null, listOrder: [], taskIdEnabled: true, taskIdProperty: "uid" }),
    });
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="task-id-property"]').element.value).toBe("uid");
  });

  it("shows a task-id save error inline without clobbering the folder autosave's error", async () => {
    const { wrapper } = mountTab({
      onSetId: () => {
        throw "Invalid ID property name";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(true);
    await flushPromises();
    expect(wrapper.get('[data-testid="task-id-error"]').text()).toContain("Invalid ID property name");
    expect(wrapper.find('[data-testid="tasks-folder-error"]').exists()).toBe(false);
  });

  it("disabling task ids succeeds even after an earlier invalid property attempt (Codex review, refined by Fix 1)", async () => {
    // Originally regression-tested that disabling succeeds DESPITE a stuck
    // invalid draft property lingering in taskIdProperty (the settings UI
    // hides the property field once disabled, so nothing let the user fix or
    // clear it) — the fix lived in set_task_id_config, which validates/applies
    // the property only while enabling and ignores it while disabling.
    // Fix 1's reload-on-failure (idAutosave, this file) goes further: ANY
    // rejected save now reloads disk truth immediately, which also resets the
    // property field — so the draft no longer lingers AT ALL, it isn't merely
    // tolerated by the backend on the next save. This test now pins the
    // surviving half of the original guarantee (disabling still succeeds
    // after an earlier failed property edit) and the new one (the draft is
    // gone, not stuck).
    const status = useSettingsStatusStore();
    const calls: Array<{ id: string; enabled: boolean; property: string | null }> = [];
    // Stateful — a successful set_task_id_config changes what the next
    // get_tasks_config reports, a REJECTED one leaves it unchanged. This
    // fidelity is load-bearing now that a rejected save reloads from
    // get_tasks_config (Fix 1); a stateless mock would report stale
    // last-mount truth forever, which no real backend does.
    let persisted = { taskIdEnabled: false, taskIdProperty: "task-id" };
    const { wrapper } = mountTab({
      onGet: () => ({
        tasksFolder: null,
        defaultList: null,
        listOrder: [],
        taskIdEnabled: persisted.taskIdEnabled,
        taskIdProperty: persisted.taskIdProperty,
      }),
      onSetId: (a) => {
        const args = a as { id: string; enabled: boolean; property: string | null };
        calls.push(args);
        if (args.enabled && args.property && !/^[A-Za-z0-9_-]+$/.test(args.property)) {
          throw "Invalid ID property name (letters, digits, - and _ only; not a reserved task field)";
        }
        persisted = { taskIdEnabled: args.enabled, taskIdProperty: args.property ?? "task-id" };
        return null;
      },
    });
    await flushPromises();

    // Enable, type an invalid property, blur — the save fails.
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(true);
    await flushPromises();
    await wrapper.get('[data-testid="task-id-property"]').setValue("bad prop");
    await wrapper.get('[data-testid="task-id-property"]').trigger("blur");
    await flushPromises();
    expect(wrapper.get('[data-testid="task-id-error"]').text()).toContain("Invalid ID property name");
    expect(status.state).toBe("error");
    // The rejected draft does NOT linger: Fix 1's reload already ran, so the
    // property field reads back the real (default/empty) persisted value
    // instead of keeping "bad prop" around indefinitely.
    expect(wrapper.get<HTMLInputElement>('[data-testid="task-id-property"]').element.value).toBe("");

    // Uncheck the toggle. Nothing invalid is queued anymore (the reload
    // already cleared it), so this is an ordinary, unremarkable disable.
    await wrapper.get('[data-testid="task-id-enabled"]').setValue(false);
    await flushPromises();

    const disableCall = calls[calls.length - 1];
    expect(disableCall).toEqual({ id: "v1", enabled: false, property: null });
    expect(status.state).not.toBe("error"); // disabling persisted, not rejected
  });

  it("reverts the toggle to disk truth and keeps the guard's error visible when a disable is refused (design spec §2a)", async () => {
    // Regression: onIdEnabledChange set taskIdEnabled optimistically and never
    // reverted it when set_task_id_config rejected. A REFUSED disable (the
    // parent-link guard) left config.json's taskIdEnabled untouched (still
    // true) while the checkbox read unchecked — the settings screen asserting
    // a persisted state that was false — and TaskIdSettings rendered the
    // count-and-remedy error only inside `v-if="enabled"`, hiding the very
    // message the guard was built to report at exactly the moment it fired.
    const REFUSAL =
      "This vault has 2 tasks with a parent, referencing Task IDs under the current property. " +
      "Clear the parent links before changing the Task ID settings.";
    const { wrapper, calls } = mountTab({
      onGet: () => ({
        tasksFolder: null,
        defaultList: null,
        listOrder: [],
        taskIdEnabled: true,
        taskIdProperty: "task-id",
      }),
      onSetId: (a) => {
        if (!(a as { enabled: boolean }).enabled) throw REFUSAL;
        return null;
      },
    });
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="task-id-enabled"]').element.checked).toBe(true);
    // Baseline BEFORE the failed save — TaskListSettings' own onMounted also
    // reads get_tasks_config, so the absolute count isn't 1; only the DELTA
    // this failed save adds is what we're pinning.
    const getCallsBefore = calls.filter((c) => c.cmd === "get_tasks_config").length;

    await wrapper.get('[data-testid="task-id-enabled"]').setValue(false);
    await flushPromises();

    // The toggle reflects what's ACTUALLY on disk (still enabled), not the
    // rejected optimistic value.
    expect(wrapper.get<HTMLInputElement>('[data-testid="task-id-enabled"]').element.checked).toBe(true);
    // Reverted via a fresh disk read, not a locally-cached "previous" value —
    // the only source of truth that's still correct whether the refusal came
    // from this guard, some other rejection, or a concurrent external write.
    expect(calls.filter((c) => c.cmd === "get_tasks_config")).toHaveLength(getCallsBefore + 1);
    // The detailed count-and-remedy error is visible, not swallowed by a
    // reverted-but-still-hidden enabled state.
    expect(wrapper.get('[data-testid="task-id-error"]').text()).toBe(REFUSAL);
  });

  it("loads the task template from disk", async () => {
    const { wrapper } = mountTab({
      onGet: () => ({
        tasksFolder: null,
        defaultList: null,
        listOrder: [],
        taskIdEnabled: false,
        taskIdProperty: "task-id",
        taskExtraFrontmatter: "project: Alpha",
        taskBodyTemplate: "- [ ] {{title}}",
      }),
    });
    await flushPromises();
    expect(wrapper.get<HTMLTextAreaElement>('[data-testid="task-extra-frontmatter"]').element.value).toBe(
      "project: Alpha",
    );
    expect(wrapper.get<HTMLTextAreaElement>('[data-testid="task-body-template"]').element.value).toBe(
      "- [ ] {{title}}",
    );
  });

  it("does not save the task template on mount", async () => {
    const { calls } = mountTab({
      onGet: () => ({
        tasksFolder: null,
        defaultList: null,
        listOrder: [],
        taskIdEnabled: false,
        taskIdProperty: "task-id",
        taskExtraFrontmatter: "project: Alpha",
        taskBodyTemplate: "- [ ] {{title}}",
      }),
    });
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_task_template_config")).toBe(false);
  });

  it("debounces a task-template edit and saves via set_task_template_config", async () => {
    const saved: unknown[] = [];
    const { wrapper } = mountTab({ onSetTemplate: (a) => (saved.push(a), null) });
    await flushPromises();
    await wrapper.get('[data-testid="task-extra-frontmatter"]').setValue("project: Alpha");
    await wrapper.get('[data-testid="task-body-template"]').setValue("- [ ] Follow up");
    expect(saved).toHaveLength(0); // still debouncing
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(saved).toContainEqual({
      id: "v1",
      extraFrontmatter: "project: Alpha",
      bodyTemplate: "- [ ] Follow up",
    });
  });

  it("flushes a pending task-template save on blur", async () => {
    const saved: unknown[] = [];
    const { wrapper } = mountTab({ onSetTemplate: (a) => (saved.push(a), null) });
    await flushPromises();
    await wrapper.get('[data-testid="task-body-template"]').setValue("- [ ] Follow up");
    await wrapper.get('[data-testid="task-body-template"]').trigger("blur");
    await flushPromises();
    expect(saved).toContainEqual({ id: "v1", extraFrontmatter: null, bodyTemplate: "- [ ] Follow up" });
  });

  it("empties the task template to null on save", async () => {
    const saved: unknown[] = [];
    const { wrapper } = mountTab({
      onGet: () => ({
        tasksFolder: null,
        defaultList: null,
        listOrder: [],
        taskIdEnabled: false,
        taskIdProperty: "task-id",
        taskExtraFrontmatter: "project: Alpha",
        taskBodyTemplate: "- [ ] {{title}}",
      }),
      onSetTemplate: (a) => (saved.push(a), null),
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-extra-frontmatter"]').setValue("");
    await wrapper.get('[data-testid="task-body-template"]').setValue("");
    await wrapper.get('[data-testid="task-body-template"]').trigger("blur");
    await flushPromises();
    expect(saved).toContainEqual({ id: "v1", extraFrontmatter: null, bodyTemplate: null });
  });
});
