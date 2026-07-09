import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Tasks from "../src/components/Tasks.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import type { TaskItem } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const many = (n: number): TaskItem[] =>
  Array.from({ length: n }, (_, i) => ({
    path: `C:/v/Tasks/${i}.md`,
    title: `Task ${i}`,
    status: "new",
    created: "2026-07-08",
    done: false,
    due: null,
    priority: null,
    tags: [],
  }));

const sample: TaskItem[] = [
  { path: "C:/v/Tasks/2026-07-08-b.md", title: "B open", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [] },
  { path: "C:/v/Tasks/2026-07-06-a.md", title: "A done", status: "done", created: "2026-07-06", done: true, due: null, priority: null, tags: [] },
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
    if (cmd === "list_tasks") return list;
    if (cmd === "add_task") {
      const a = args as { title: string; due?: string; priority?: string; tags?: string[] };
      const created = { path: "C:/v/Tasks/2026-07-08-new.md", title: a.title, status: "new", created: "2026-07-08", done: false, due: a.due ?? null, priority: a.priority ?? null, tags: a.tags ?? [] };
      list = [created, ...list];
      return created;
    }
    if (cmd === "set_task_status") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: "v1" } });
  return { wrapper, calls };
}

describe("Tasks", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("loads tasks for the vault on mount", async () => {
    const { calls } = mountView();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "list_tasks")).toEqual({ cmd: "list_tasks", args: { id: "v1" } });
    // The folder setting moved to Vault settings — the Tasks view no longer
    // reads the tasks config.
    expect(calls.find((c) => c.cmd === "get_tasks_config")).toBeUndefined();
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

  it("toggles a task via set_task_status with a status string", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    const call = calls.find((c) => c.cmd === "set_task_status");
    expect(call?.args).toMatchObject({ id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", status: "done" });
  });

  it("archives a task: sends status archived and removes the row", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-archive"]').trigger("click"); // first row = "B open"
    await flushPromises();
    const call = calls.find((c) => c.cmd === "set_task_status");
    expect(call?.args).toMatchObject({ id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", status: "archived" });
    expect(wrapper.text()).not.toContain("B open");
  });

  it("re-inserts the row and notifies when archive fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper } = mountView({
      set_task_status: () => {
        throw new Error("disk full");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-archive"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("B open"); // restored
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });

  it("shows a progress bar of done/total and hides it at zero", async () => {
    const { wrapper } = mountView(); // sample = 1 open + 1 done → 1/2
    await flushPromises();
    const bar = wrapper.get('[data-testid="task-progress"]');
    expect(bar.text()).toContain("1 / 2");
    // Empty vault → no bar.
    const empty = mountView({ list_tasks: () => [] });
    await flushPromises();
    expect(empty.wrapper.find('[data-testid="task-progress"]').exists()).toBe(false);
  });

  it("no longer renders the tasks-folder input", async () => {
    const { wrapper } = mountView();
    await flushPromises();
    expect(wrapper.find('[data-testid="tasks-folder-input"]').exists()).toBe(false);
  });

  it("ignores a re-toggle while the row's write is still in flight", async () => {
    // A slow set_task_status: the second change on the same row must not fire a
    // second concurrent write (which could land out of order vs the first).
    // Re-query the checkbox by row content (not a held reference): once
    // toggled optimistically the row relocates from the open/no-date bucket
    // into the Done bucket's own <ul>, so Vue mounts a fresh node there
    // rather than reusing the one from the old bucket's list.
    let resolve: (() => void) | undefined;
    const { wrapper, calls } = mountView({
      set_task_status: () => new Promise<null>((r) => {
        resolve = () => r(null);
      }),
    });
    await flushPromises();
    const rowCheckbox = () =>
      wrapper
        .findAll('[data-testid="task-row"]')
        .find((r) => r.text().includes("B open"))!
        .get('[data-testid="task-checkbox"]');
    await rowCheckbox().trigger("change"); // first toggle — write pending
    await rowCheckbox().trigger("change"); // re-toggle while pending — must be ignored
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "set_task_status")).toHaveLength(1);
    expect((rowCheckbox().element as HTMLInputElement).disabled).toBe(true);
    resolve?.();
    await flushPromises();
    expect((rowCheckbox().element as HTMLInputElement).disabled).toBe(false);
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

  it("opens a task in Obsidian when its title is clicked and closes the panel", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-open"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "open_task")).toEqual({
      cmd: "open_task",
      args: { id: "v1", path: "C:/v/Tasks/2026-07-08-b.md" },
    });
    // Obsidian takes over — the panel gets out of the way, mirroring the
    // vault-open and recording-open flows.
    expect(calls.find((c) => c.cmd === "close_panel")).toBeTruthy();
  });

  it("toasts and keeps the panel open when open_task fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper, calls } = mountView({
      open_task: () => {
        throw new Error("no vault");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-open"]').trigger("click");
    await flushPromises();
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
    // A failed launch must NOT hide the panel — the error toast is there.
    expect(calls.find((c) => c.cmd === "close_panel")).toBeUndefined();
  });

  it("renders a due chip and priority dot from the task fields", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/p.md", title: "P", status: "new", created: "2026-07-08", done: false, due: "2026-07-15", priority: "high", tags: [] },
      ],
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="task-due"]').text()).toBe("Jul 15");
    expect(wrapper.find('[data-testid="task-priority"]').exists()).toBe(true);
  });

  it("shows no due chip or dot for a plain task, and no dot for normal", async () => {
    const { wrapper } = mountView();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-due"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-priority"]').exists()).toBe(false);
  });

  it("falls back to the raw due string for an out-of-range month", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/x.md", title: "Bad month", status: "new", created: "2026-07-08", done: false, due: "2026-13-05", priority: null, tags: [] },
      ],
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="task-due"]').text()).toBe("2026-13-05");
  });

  it("renders a due chip with no leading zero on the day", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/x.md", title: "Single digit day", status: "new", created: "2026-07-08", done: false, due: "2026-07-05", priority: null, tags: [] },
      ],
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="task-due"]').text()).toBe("Jul 5");
  });

  it("groups tasks into date buckets with headers", async () => {
    vi.useFakeTimers({ now: new Date(2026, 6, 9, 12, 0, 0), toFake: ["Date"] }); // 2026-07-09 local
    try {
      const { wrapper } = mountView({
        list_tasks: () => [
          { path: "C:/v/Tasks/o.md", title: "Old", status: "new", created: "2026-07-01", done: false, due: "2026-07-08", priority: null, tags: [] },
          { path: "C:/v/Tasks/t.md", title: "Now", status: "new", created: "2026-07-01", done: false, due: "2026-07-09", priority: null, tags: [] },
          { path: "C:/v/Tasks/u.md", title: "Soon", status: "new", created: "2026-07-01", done: false, due: "2026-07-10", priority: null, tags: [] },
          { path: "C:/v/Tasks/n.md", title: "Someday", status: "new", created: "2026-07-01", done: false, due: null, priority: null, tags: [] },
          { path: "C:/v/Tasks/d.md", title: "Finished", status: "done", created: "2026-07-01", done: true, due: null, priority: null, tags: [] },
        ],
      });
      await flushPromises();
      const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
      expect(headers).toEqual(["Overdue", "Today", "Upcoming", "No date", "Done"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows no bucket headers when no open task has a parseable due date", async () => {
    // The pre-due-date flat list must stay visually unchanged — headers appear
    // only once dated open tasks exist.
    const { wrapper } = mountView(); // sample: one undated open + one done
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
  });

  it("buckets an unparseable hand-authored due under No date", async () => {
    vi.useFakeTimers({ now: new Date(2026, 6, 9, 12, 0, 0), toFake: ["Date"] });
    try {
      const { wrapper } = mountView({
        list_tasks: () => [
          { path: "C:/v/Tasks/x.md", title: "Bad", status: "new", created: "2026-07-01", done: false, due: "tomorrow", priority: null, tags: [] },
          { path: "C:/v/Tasks/y.md", title: "Dated", status: "new", created: "2026-07-01", done: false, due: "2026-07-10", priority: null, tags: [] },
        ],
      });
      await flushPromises();
      const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
      expect(headers).toEqual(["Upcoming", "No date"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("adds a task with due and priority from the options row", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-add-options"]').trigger("click");
    await wrapper.get('[data-testid="task-add-due"]').setValue("2026-07-20");
    await wrapper.get('[data-testid="task-add-priority-high"]').trigger("click");
    await wrapper.get('[data-testid="task-input"]').setValue("Big one");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({
      cmd: "add_task",
      args: { id: "v1", title: "Big one", due: "2026-07-20", priority: "high" },
    });
  });

  it("omits due/priority when the options are untouched", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-input"]').setValue("Plain");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({
      cmd: "add_task",
      args: { id: "v1", title: "Plain" },
    });
  });

  it("edits a task inline: sends only the changed fields", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "Old name", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-title"]').setValue("New name");
    await wrapper.get('[data-testid="task-edit-priority-high"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toEqual({
      cmd: "update_task",
      args: { id: "v1", path: "C:/v/Tasks/e.md", patch: { title: "New name", priority: "high" } },
    });
    expect(wrapper.text()).toContain("New name"); // optimistic
  });

  it("clearing the due date sends clearDue", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-due"]').setValue("");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")?.args).toMatchObject({
      patch: { clearDue: true },
    });
  });

  it("reverts the row and notifies when the edit save fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "B open", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [] },
      ],
      update_task: () => {
        throw new Error("disk full");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-title"]').setValue("Broken");
    await wrapper.get('[data-testid="task-edit-due"]').setValue("2026-08-01");
    await wrapper.get('[data-testid="task-edit-priority-high"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    // All three fields (title, due, priority) revert together — pins that
    // saveEdit's failure path restores the whole `before` snapshot, not just
    // the field a given test happens to check.
    expect(wrapper.text()).toContain("B open"); // reverted title
    expect(wrapper.text()).not.toContain("Broken");
    expect(wrapper.get('[data-testid="task-due"]').text()).toBe("Jul 10"); // reverted due
    expect(wrapper.find('[data-testid="task-priority"]').exists()).toBe(false); // reverted priority
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });

  it("cancel closes the editor without a write", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-title"]').setValue("Nope");
    await wrapper.get('[data-testid="task-edit-cancel"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
    expect(wrapper.text()).toContain("B open");
  });

  it("saving with nothing changed is a no-op close", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
    expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(false);
  });

  it("shows the filter only above 5 tasks and narrows by title", async () => {
    const { wrapper } = mountView({ list_tasks: () => many(6) });
    await flushPromises();
    const input = wrapper.get('[data-testid="task-filter"]');
    await input.setValue("Task 3");
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(1);
    expect(wrapper.text()).toContain("Task 3");
  });

  it("hides the filter for short lists", async () => {
    const { wrapper } = mountView(); // 2 tasks
    await flushPromises();
    expect(wrapper.find('[data-testid="task-filter"]').exists()).toBe(false);
  });

  it("ignores stale filter text once the task count drops to five or fewer", async () => {
    // Archiving tasks below the threshold hides the filter INPUT; the stale
    // query must stop applying too, or the user is stuck on a narrowed/empty
    // list with no visible way to clear it.
    const { wrapper } = mountView({ list_tasks: () => many(6) });
    await flushPromises();
    await wrapper.get('[data-testid="task-filter"]').setValue("Task 0");
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(1);
    // Archive the one visible row ("Task 0") — total drops to 5.
    await wrapper.get('[data-testid="task-archive"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="task-filter"]').exists()).toBe(false);
    // All 5 remaining tasks render; the stale "Task 0" query is ignored.
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(5);
    expect(wrapper.text()).not.toContain("No tasks match");
  });

  it("keeps the progress bar counting the unfiltered list", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        ...many(6),
        { path: "C:/v/Tasks/d.md", title: "Done one", status: "done", created: "2026-07-01", done: true, due: null, priority: null, tags: [] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-filter"]').setValue("Task 3");
    expect(wrapper.get('[data-testid="task-progress"]').text()).toContain("1 / 7");
  });

  it("shows the no-match empty state when the filter excludes everything", async () => {
    const { wrapper } = mountView({ list_tasks: () => many(6) });
    await flushPromises();
    await wrapper.get('[data-testid="task-filter"]').setValue("zzz");
    await flushPromises();
    expect(wrapper.text()).toContain('No tasks match "zzz"');
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(0);
  });

  it("hides the filter for exactly 5 tasks (off-by-one boundary)", async () => {
    const { wrapper } = mountView({ list_tasks: () => many(5) });
    await flushPromises();
    expect(wrapper.find('[data-testid="task-filter"]').exists()).toBe(false);
  });

  it("filters case-insensitively by title substring", async () => {
    const { wrapper } = mountView({ list_tasks: () => many(6) });
    await flushPromises();
    await wrapper.get('[data-testid="task-filter"]').setValue("task 3");
    await flushPromises();
    const rows = wrapper.findAll('[data-testid="task-row"]');
    expect(rows).toHaveLength(1);
    expect(rows[0].text()).toContain("Task 3");
  });

  it("renders tag chips and filters by tag on chip click", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work", "home/errands"] },
        { path: "C:/v/Tasks/b.md", title: "Plain", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [] },
      ],
    });
    await flushPromises();
    const chips = wrapper.findAll('[data-testid="task-tag"]');
    expect(chips.map((c) => c.text())).toEqual(["#work", "#home/errands"]);
    await chips[0].trigger("click");
    await flushPromises();
    // Chip click filters (no open_task fired) and shows the dismiss chip.
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(1);
    expect(wrapper.text()).toContain("Tagged");
    expect(wrapper.text()).not.toContain("Plain");
    expect(wrapper.get('[data-testid="task-tag-filter"]').text()).toContain("#work");
  });

  it("a chip click does not open the task in Obsidian", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-tag"]').trigger("click");
    expect(calls.find((c) => c.cmd === "open_task")).toBeUndefined();
  });

  it("clearing the tag filter restores the full list", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"] },
        { path: "C:/v/Tasks/b.md", title: "Plain", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-tag"]').trigger("click");
    await wrapper.get('[data-testid="task-tag-filter-clear"]').trigger("click");
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(2);
  });

  it("tag and title filters combine (AND)", async () => {
    const tagged = (n: number, tags: string[]): TaskItem => ({
      path: `C:/v/Tasks/${n}.md`, title: `Task ${n}`, status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags,
    });
    const { wrapper } = mountView({
      list_tasks: () => [tagged(0, ["work"]), tagged(1, ["work"]), tagged(2, []), tagged(3, []), tagged(4, []), tagged(5, [])],
    });
    await flushPromises();
    await wrapper.findAll('[data-testid="task-tag"]')[0].trigger("click"); // tag=work → 0,1
    await wrapper.get('[data-testid="task-filter"]').setValue("Task 1");
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(1);
    expect(wrapper.text()).toContain("Task 1");
  });

  it("adds a task with tags parsed from the options row", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-add-options"]').trigger("click");
    await wrapper.get('[data-testid="task-add-tags"]').setValue("#work, home/errands");
    await wrapper.get('[data-testid="task-input"]').setValue("Tagged one");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({
      cmd: "add_task",
      args: { id: "v1", title: "Tagged one", tags: ["work", "home/errands"] },
    });
  });

  it("edits tags inline: sends the parsed list, empty input clears", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    const input = wrapper.get('[data-testid="task-edit-tags"]');
    expect((input.element as HTMLInputElement).value).toBe("work");
    await input.setValue("work urgent");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")?.args).toMatchObject({
      patch: { tags: ["work", "urgent"] },
    });
    // Now clear them.
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-tags"]').setValue("");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    const clears = calls.filter((c) => c.cmd === "update_task");
    expect(clears[clears.length - 1]?.args).toMatchObject({ patch: { tags: [] } });
  });

  it("editor omits tags from the patch when unchanged", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-title"]').setValue("T2");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")?.args).toEqual({
      id: "v1",
      path: "C:/v/Tasks/e.md",
      patch: { title: "T2" },
    });
  });

  it("groups by tag with repeats, No tags and Done sections", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Both", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["Work", "home"] },
        { path: "C:/v/Tasks/b.md", title: "Untagged", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [] },
        { path: "C:/v/Tasks/c.md", title: "Finished", status: "done", created: "2026-07-06", done: true, due: null, priority: null, tags: ["work"] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-tags"]').trigger("click");
    const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
    // Alphabetical case-insensitive tag sections, then No tags, then Done.
    expect(headers).toEqual(["#home", "#Work", "No tags", "Done"]);
    // "Both" repeats under each of its tags.
    const rows = wrapper.findAll('[data-testid="task-row"]');
    expect(rows.filter((r) => r.text().includes("Both"))).toHaveLength(2);
    // Done tasks land in Done, not under their tags.
    expect(rows.filter((r) => r.text().includes("Finished"))).toHaveLength(1);
  });

  it("the editor opens on only the clicked duplicate row in tag view", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Both", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work", "home"] },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-tags"]').trigger("click");
    const pencils = wrapper.findAll('[data-testid="task-edit"]');
    expect(pencils).toHaveLength(2);
    await pencils[0].trigger("click");
    // One editor, not two — the clicked row only.
    expect(wrapper.findAll('[data-testid="task-edit-title"]')).toHaveLength(1);
  });

  it("grouping defaults to dates and the toggle switches back", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"] },
      ],
    });
    await flushPromises();
    // Dates mode by default: an undated list shows no headers.
    expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
    await wrapper.get('[data-testid="task-grouping-tags"]').trigger("click");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]').length).toBeGreaterThan(0);
    await wrapper.get('[data-testid="task-grouping-dates"]').trigger("click");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
  });

});
