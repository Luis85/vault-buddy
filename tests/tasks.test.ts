import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Tasks from "../src/components/Tasks.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import type { TaskItem } from "../src/types";
import { aggTask, mountAggregate, mountAggregateAttached, mountView, sample } from "./helpers/taskMount";

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
    list: "",
    order: null,
    id: null,
  }));

describe("Tasks", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => {
    clearMocks();
    // The grouping toggle now persists per view (taskGrouping.ts); several
    // cases below switch it (dates/tags) without switching back, and later
    // cases assume the fresh-view Lists default (e.g. "grouping defaults to
    // lists…", the New-list controls tests) — without this, an earlier
    // case's persisted choice for view "v1" would leak in and break them.
    localStorage.clear();
  });

  it("loads tasks, lists, and the lists config for the vault on mount", async () => {
    const { calls } = mountView();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "list_tasks")).toEqual({ cmd: "list_tasks", args: { id: "v1" } });
    // The Lists increment reintroduced ONE config read (defaultList/
    // listOrder feed the composer and the Lists grouping) plus the list
    // enumeration; the folder setting itself still lives in Vault settings.
    expect(calls.filter((c) => c.cmd === "get_tasks_config")).toHaveLength(1);
    expect(calls.find((c) => c.cmd === "list_task_lists")).toEqual({ cmd: "list_task_lists", args: { id: "v1" } });
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

  it("failed toggle restores the ORIGINAL status, not a forged one (GAP-32)", async () => {
    // Revert used to hardcode status "new": a failed toggle on an
    // in-progress task silently relabeled it.
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/ip.md", title: "In progress", status: "in-progress", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list: "", order: null },
      ],
      set_task_status: () => {
        throw new Error("disk full");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    const checkbox = wrapper.findAll('[data-testid="task-checkbox"]')[0];
    expect((checkbox.element as HTMLInputElement).checked).toBe(false);
    const task = (wrapper.vm as any).tasks[0];
    expect(task.status).toBe("in-progress");
    expect(task.done).toBe(false);
  });

  it("refreshes the vault's task count after a successful mutation (GAP-32)", async () => {
    // Badges only reloaded on panel-shown — stale after add/toggle/archive
    // until reopen (Codex PR #46 finding).
    const { wrapper, calls } = mountView({
      count_open_tasks: () => 1,
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    expect(
      calls.some((c) => c.cmd === "count_open_tasks" && (c.args as { id: string }).id === "v1"),
    ).toBe(true);
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

  it("ignores Enter while composing an IME candidate (GAP-31)", async () => {
    // Committing a CJK candidate with Enter used to immediately create a task
    // document from the half-composed title — a sanctioned vault write.
    const { wrapper, calls } = mountView();
    await flushPromises();
    const titleInput = wrapper.get('[data-testid="task-input"]');
    await titleInput.setValue("候選");
    await titleInput.trigger("keydown", { key: "Enter", isComposing: true });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toBeUndefined();
    // After composition ends, normal Enter works.
    await titleInput.trigger("keydown", { key: "Enter", isComposing: false });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({ cmd: "add_task", args: { id: "v1", title: "候選" } });
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
        { path: "C:/v/Tasks/p.md", title: "P", status: "new", created: "2026-07-08", done: false, due: "2026-07-15", priority: "high", tags: [], list: "", order: null },
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
        { path: "C:/v/Tasks/x.md", title: "Bad month", status: "new", created: "2026-07-08", done: false, due: "2026-13-05", priority: null, tags: [], list: "", order: null },
      ],
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="task-due"]').text()).toBe("2026-13-05");
  });

  it("renders a due chip with no leading zero on the day", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/x.md", title: "Single digit day", status: "new", created: "2026-07-08", done: false, due: "2026-07-05", priority: null, tags: [], list: "", order: null },
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
          { path: "C:/v/Tasks/o.md", title: "Old", status: "new", created: "2026-07-01", done: false, due: "2026-07-08", priority: null, tags: [], list: "", order: null },
          { path: "C:/v/Tasks/t.md", title: "Now", status: "new", created: "2026-07-01", done: false, due: "2026-07-09", priority: null, tags: [], list: "", order: null },
          { path: "C:/v/Tasks/u.md", title: "Soon", status: "new", created: "2026-07-01", done: false, due: "2026-07-10", priority: null, tags: [], list: "", order: null },
          { path: "C:/v/Tasks/n.md", title: "Someday", status: "new", created: "2026-07-01", done: false, due: null, priority: null, tags: [], list: "", order: null },
          { path: "C:/v/Tasks/d.md", title: "Finished", status: "done", created: "2026-07-01", done: true, due: null, priority: null, tags: [], list: "", order: null },
        ],
      });
      await flushPromises();
      // Lists is the default grouping now — switch to Dates to exercise its
      // bucket-header behavior.
      await wrapper.get('[data-testid="task-grouping-dates"]').trigger("click");
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
    // Lists is the default grouping now — switch to Dates to exercise its
    // bucket-header behavior.
    await wrapper.get('[data-testid="task-grouping-dates"]').trigger("click");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
  });

  it("buckets an unparseable hand-authored due under No date", async () => {
    vi.useFakeTimers({ now: new Date(2026, 6, 9, 12, 0, 0), toFake: ["Date"] });
    try {
      const { wrapper } = mountView({
        list_tasks: () => [
          { path: "C:/v/Tasks/x.md", title: "Bad", status: "new", created: "2026-07-01", done: false, due: "tomorrow", priority: null, tags: [], list: "", order: null },
          { path: "C:/v/Tasks/y.md", title: "Dated", status: "new", created: "2026-07-01", done: false, due: "2026-07-10", priority: null, tags: [], list: "", order: null },
        ],
      });
      await flushPromises();
      // Lists is the default grouping now — switch to Dates to exercise its
      // bucket-header behavior.
      await wrapper.get('[data-testid="task-grouping-dates"]').trigger("click");
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
        { path: "C:/v/Tasks/e.md", title: "Old name", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [], list: "", order: null },
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

  it("does not save an edit with a blank title, keeping the editor open (Codex PR #46)", async () => {
    // Clearing the inline title dropped it from the changed-fields patch, so a
    // simultaneous due/priority/tags change wrote while the empty title was
    // silently retained — no error, no rejection. A blank title must block the
    // whole save, mirroring the add-task composer's disabled Add button.
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "Old name", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [], list: "", order: null },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-title"]').setValue("   ");
    await wrapper.get('[data-testid="task-edit-priority-high"]').trigger("click");
    await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
    await flushPromises();
    // No write at all — not even the changed priority.
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
    // The editor stays open so the user can fix the title or cancel.
    expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(true);
  });

  it("clearing the due date sends clearDue", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [], list: "", order: null },
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
        { path: "C:/v/Tasks/e.md", title: "B open", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null, tags: [], list: "", order: null },
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

  it("ignores Enter on the inline editor while composing an IME candidate", async () => {
    // Codex review, PR #46 round 2: the add-path guard (GAP-31) was applied
    // to the new-task input but missed the editor's title field — an IME
    // candidate commit fired Enter with isComposing=true and saved/closed
    // the editor with a half-composed title.
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    const titleInput = wrapper.get('[data-testid="task-edit-title"]');
    await titleInput.setValue("候選");
    await titleInput.trigger("keydown", { key: "Enter", isComposing: true });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
    expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(true); // editor still open
    // After composition ends, normal Enter saves.
    await titleInput.trigger("keydown", { key: "Enter", isComposing: false });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toEqual({
      cmd: "update_task",
      args: { id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", patch: { title: "候選" } },
    });
  });

  it("does not cancel a composing Enter's default in the editor (IME candidate commit survives)", async () => {
    // Codex review, PR #46: `@keydown.enter.prevent` runs preventDefault
    // BEFORE onEditTitleEnter can see isComposing, so a candidate-commit
    // Enter had its default cancelled and IME selection broke. Inspect
    // defaultPrevented directly on a manually dispatched event — trigger()
    // hides the event object.
    const { wrapper } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    const input = wrapper.get('[data-testid="task-edit-title"]')
      .element as HTMLInputElement;

    const composing = new KeyboardEvent("keydown", {
      key: "Enter",
      cancelable: true,
      bubbles: true,
    });
    Object.defineProperty(composing, "isComposing", { value: true });
    input.dispatchEvent(composing);
    expect(composing.defaultPrevented).toBe(false); // candidate commit not cancelled

    const real = new KeyboardEvent("keydown", {
      key: "Enter",
      cancelable: true,
      bubbles: true,
    });
    Object.defineProperty(real, "isComposing", { value: false });
    input.dispatchEvent(real);
    expect(real.defaultPrevented).toBe(true); // a real Enter is still consumed
  });

  it("editor Escape cancels only the edit — it must not reach the panel-close handler", async () => {
    // Codex review, PR #46: onEditTitleEsc cancelled the row edit but let
    // the keydown bubble to PanelRoot's window-level Escape handler, which
    // closed the WHOLE panel — discarding the editing context instead of
    // just the row edit (same class as GAP-27's SelectMenu Escape).
    // Attached mount: a detached tree never bubbles to window, so the
    // assertion would pass vacuously (the GAP-27 test learned this too).
    setActivePinia(createPinia());
    mockIPC((cmd) => (cmd === "list_tasks" ? sample.map((t) => ({ ...t })) : null));
    const wrapper = mount(Tasks, { props: { vaultId: "v1" }, attachTo: document.body });
    const reachedWindow = vi.fn();
    window.addEventListener("keydown", reachedWindow);
    try {
      await flushPromises();
      await wrapper.get('[data-testid="task-edit"]').trigger("click");
      const titleInput = wrapper.get('[data-testid="task-edit-title"]');
      await titleInput.trigger("keydown", { key: "Escape", isComposing: false });
      await flushPromises();
      expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(false); // edit cancelled
      expect(reachedWindow).not.toHaveBeenCalled(); // panel-close never sees it
    } finally {
      window.removeEventListener("keydown", reachedWindow);
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("editor Escape is caught for EVERY field, not just the title (due input)", async () => {
    // Codex review, PR #46: the Escape-doesn't-close-panel guard was wired
    // only on the title input; Escape focused in the due/tags/priority
    // controls bubbled past to PanelRoot's window handler and closed the
    // whole panel. A root-level handler must catch Escape from any field.
    setActivePinia(createPinia());
    mockIPC((cmd) => (cmd === "list_tasks" ? sample.map((t) => ({ ...t })) : null));
    const wrapper = mount(Tasks, { props: { vaultId: "v1" }, attachTo: document.body });
    const reachedWindow = vi.fn();
    window.addEventListener("keydown", reachedWindow);
    try {
      await flushPromises();
      await wrapper.get('[data-testid="task-edit"]').trigger("click");
      const dueInput = wrapper.get('[data-testid="task-edit-due"]');
      await dueInput.trigger("keydown", { key: "Escape", isComposing: false });
      await flushPromises();
      expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(false); // edit cancelled
      expect(reachedWindow).not.toHaveBeenCalled(); // panel-close never sees it
    } finally {
      window.removeEventListener("keydown", reachedWindow);
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("ignores Escape on the inline editor while composing an IME candidate", async () => {
    // Escape during composition cancels the IME CANDIDATE, not the edit —
    // without the guard, cancelEdit would drop the in-progress edit too.
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-edit"]').trigger("click");
    const titleInput = wrapper.get('[data-testid="task-edit-title"]');
    await titleInput.setValue("Nope");
    await titleInput.trigger("keydown", { key: "Escape", isComposing: true });
    await flushPromises();
    expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(true); // editor still open
    await titleInput.trigger("keydown", { key: "Escape", isComposing: false });
    await flushPromises();
    expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(false);
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
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
        { path: "C:/v/Tasks/d.md", title: "Done one", status: "done", created: "2026-07-01", done: true, due: null, priority: null, tags: [], list: "", order: null },
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
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work", "home/errands"], list: "", order: null },
        { path: "C:/v/Tasks/b.md", title: "Plain", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [], list: "", order: null },
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
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
      ],
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-tag"]').trigger("click");
    expect(calls.find((c) => c.cmd === "open_task")).toBeUndefined();
  });

  it("tag chips are real buttons and siblings of the open button (not nested)", async () => {
    // Codex review, PR #46: role="button" chips nested inside the task-open
    // <button> is invalid interactive content — browsers expose it
    // inconsistently, so a chip activation can be swallowed by the parent
    // open button. The chips must be their own buttons outside it.
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
      ],
    });
    await flushPromises();
    const chip = wrapper.get('[data-testid="task-tag"]').element;
    expect(chip.tagName).toBe("BUTTON"); // native button, not a role="button" span
    expect(chip.closest('[data-testid="task-open"]')).toBeNull(); // not a descendant of the open button
  });

  it("clearing the tag filter restores the full list", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
        { path: "C:/v/Tasks/b.md", title: "Plain", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [], list: "", order: null },
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
      path: `C:/v/Tasks/${n}.md`, title: `Task ${n}`, status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags, list: "", order: null, id: null,
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

  it("strips every leading # from a tags token, not just one", async () => {
    // Regression: parseTagsInput used to strip only one leading `#`, so
    // `##work` optimistically applied as "#work" but the shell strips its
    // own single `#` and lands "work" on disk — divergence between the
    // optimistic UI and the persisted value.
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-add-options"]').trigger("click");
    await wrapper.get('[data-testid="task-add-tags"]').setValue("##work, #home");
    await wrapper.get('[data-testid="task-input"]').setValue("Double hash");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({
      cmd: "add_task",
      args: { id: "v1", title: "Double hash", tags: ["work", "home"] },
    });
  });

  it("edits tags inline: sends the parsed list, empty input clears", async () => {
    const { wrapper, calls } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
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
        { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
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
        { path: "C:/v/Tasks/a.md", title: "Both", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["Work", "home"], list: "", order: null },
        { path: "C:/v/Tasks/b.md", title: "Untagged", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [], list: "", order: null },
        { path: "C:/v/Tasks/c.md", title: "Finished", status: "done", created: "2026-07-06", done: true, due: null, priority: null, tags: ["work"], list: "", order: null },
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
        { path: "C:/v/Tasks/a.md", title: "Both", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work", "home"], list: "", order: null },
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

  it("shows the tag-only no-match empty state and keeps the dismiss chip", async () => {
    // Filter to a tag, then archive the one task that carries it: the filtered
    // list empties out (a second, untagged task keeps the OVERALL list
    // non-empty so the "No tasks yet." branch doesn't mask this case) but the
    // tag filter chip must stay visible, or the user is stranded with no way
    // to clear the filter.
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
        { path: "C:/v/Tasks/b.md", title: "Plain", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: [], list: "", order: null },
      ],
      set_task_status: () => null,
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-tag"]').trigger("click");
    await wrapper.get('[data-testid="task-archive"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("No tasks match #work");
    expect(wrapper.find('[data-testid="task-tag-filter-clear"]').exists()).toBe(true);
  });

  it("tagFilter selects tasks (not sections) in tag grouping", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "WorkHome", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work", "home"], list: "", order: null },
        { path: "C:/v/Tasks/b.md", title: "HomeOnly", status: "new", created: "2026-07-07", done: false, due: null, priority: null, tags: ["home"], list: "", order: null },
      ],
    });
    await flushPromises();
    const chips = wrapper.findAll('[data-testid="task-tag"]');
    const workChip = chips.find((c) => c.text() === "#work")!;
    await workChip.trigger("click");
    await wrapper.get('[data-testid="task-grouping-tags"]').trigger("click");
    await flushPromises();
    // Only the work-tagged task is selected — "HomeOnly" never renders.
    const rows = wrapper.findAll('[data-testid="task-row"]');
    expect(rows).toHaveLength(2);
    expect(rows.every((r) => r.text().includes("WorkHome"))).toBe(true);
    expect(wrapper.text()).not.toContain("HomeOnly");
    // The selected task still appears under BOTH its section headers.
    const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
    expect(headers).toEqual(["#home", "#work"]);
  });

  it("grouping defaults to lists and the toggle switches to dates", async () => {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/a.md", title: "Tagged", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: ["work"], list: "", order: null },
      ],
    });
    await flushPromises();
    // Lists mode by default: a root task shows the "No list" section header.
    expect(wrapper.get('[data-testid="task-grouping-lists"]').attributes("aria-checked")).toBe("true");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]').length).toBeGreaterThan(0);
    // Switching to dates: an undated list shows no headers.
    await wrapper.get('[data-testid="task-grouping-dates"]').trigger("click");
    expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
  });

  it("creates a new list from the Lists view controls and shows the empty section", async () => {
    const created: string[] = [];
    const { wrapper } = mountView({
      list_task_lists: () => [],
      create_task_list: (args) => {
        const name = (args as { name: string }).name;
        created.push(name);
        return name; // the landed list name
      },
    });
    await flushPromises();
    // Lists grouping is the default → the New list button is visible.
    await wrapper.get('[data-testid="task-newlist"]').trigger("click");
    await wrapper.get('[data-testid="task-newlist-input"]').setValue("Inbox");
    await wrapper.get('[data-testid="task-newlist-confirm"]').trigger("click");
    await flushPromises();
    expect(created).toEqual(["Inbox"]);
    const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
    expect(headers).toContain("Inbox");
  });

  it("creates a new list on Enter in the input", async () => {
    const created: string[] = [];
    const { wrapper } = mountView({
      list_task_lists: () => [],
      create_task_list: (args) => {
        const name = (args as { name: string }).name;
        created.push(name);
        return name;
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-newlist"]').trigger("click");
    await wrapper.get('[data-testid="task-newlist-input"]').setValue("Reading");
    await wrapper.get('[data-testid="task-newlist-input"]').trigger("keydown.enter");
    await flushPromises();
    expect(created).toEqual(["Reading"]);
    expect(wrapper.find('[data-testid="task-newlist-input"]').exists()).toBe(false); // closed
  });

  it("keeps the new-list draft open when the create fails (Codex PR #59)", async () => {
    // A rejected create (invalid name, root validation) must NOT clear the
    // inline form: the parent only bumps resetNonce on success, so the draft
    // survives for a correct-and-retry instead of being lost on every failure.
    const { wrapper } = mountView({
      list_task_lists: () => [],
      create_task_list: () => {
        throw new Error("List names cannot contain /");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-newlist"]').trigger("click");
    await wrapper.get('[data-testid="task-newlist-input"]').setValue("Foo/Bar");
    await wrapper.get('[data-testid="task-newlist-confirm"]').trigger("click");
    await flushPromises();
    const input = wrapper.find('[data-testid="task-newlist-input"]');
    expect(input.exists()).toBe(true); // still open
    expect((input.element as HTMLInputElement).value).toBe("Foo/Bar"); // draft kept
  });

  it("ignores Enter on the new-list input while composing an IME candidate", async () => {
    const { wrapper, calls } = mountView({ list_task_lists: () => [] });
    await flushPromises();
    await wrapper.get('[data-testid="task-newlist"]').trigger("click");
    const input = wrapper.get('[data-testid="task-newlist-input"]');
    await input.setValue("候選");
    await input.trigger("keydown", { key: "Enter", isComposing: true });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "create_task_list")).toBeUndefined();
    expect(wrapper.find('[data-testid="task-newlist-input"]').exists()).toBe(true); // still open
  });

  it("the cancel button closes the new-list input without creating", async () => {
    const { wrapper, calls } = mountView({ list_task_lists: () => [] });
    await flushPromises();
    await wrapper.get('[data-testid="task-newlist"]').trigger("click");
    await wrapper.get('[data-testid="task-newlist-input"]').setValue("Discard me");
    await wrapper.get('[data-testid="task-newlist-cancel"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "create_task_list")).toBeUndefined();
    expect(wrapper.find('[data-testid="task-newlist-input"]').exists()).toBe(false);
  });

  it("Escape cancels the new-list input without creating — it must not reach the panel-close handler", async () => {
    // Mirrors the inline editor's Escape-doesn't-bubble tests above: the
    // handler calls stopPropagation so PanelRoot's window-level Escape
    // listener never sees it and closes the whole panel. Attached mount: a
    // detached tree never bubbles to window, so the assertion would pass
    // vacuously otherwise.
    setActivePinia(createPinia());
    mockIPC((cmd) => (cmd === "list_tasks" ? sample.map((t) => ({ ...t })) : cmd === "list_task_lists" ? [] : null));
    const wrapper = mount(Tasks, { props: { vaultId: "v1" }, attachTo: document.body });
    const reachedWindow = vi.fn();
    window.addEventListener("keydown", reachedWindow);
    try {
      await flushPromises();
      await wrapper.get('[data-testid="task-newlist"]').trigger("click");
      const input = wrapper.get('[data-testid="task-newlist-input"]');
      await input.trigger("keydown", { key: "Escape", isComposing: false });
      await flushPromises();
      expect(wrapper.find('[data-testid="task-newlist-input"]').exists()).toBe(false); // closed
      expect(reachedWindow).not.toHaveBeenCalled(); // panel-close never sees it
    } finally {
      window.removeEventListener("keydown", reachedWindow);
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("aggregate mode merges every vault's tasks in global sort order", async () => {
    const { wrapper, calls } = mountAggregate();
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "list_tasks").map((c) => (c.args as { id: string }).id).sort()).toEqual(["va", "vb"]);
    const rows = wrapper.findAll('[data-testid="task-row"]');
    // Newest created first: Beta task (07-09) before Alpha task (07-08).
    expect(rows[0].text()).toContain("Beta task");
    expect(rows[1].text()).toContain("Alpha task");
  });

  it("orders otherwise-equal tasks by vault name for cross-vault stability", async () => {
    const { wrapper, calls } = mountAggregate({
      list_tasks: (args) =>
        [(args as { id: string }).id === "va"
          ? aggTask("va", "Same", "2026-07-08")
          : aggTask("vb", "Same", "2026-07-08")],
    });
    await flushPromises();
    // Titles/created equal → vaultName tiebreak puts Alpha's copy first;
    // observable through which vault the first row's toggle hits.
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_status")?.args).toMatchObject({ id: "va" });
  });

  it("a failing vault degrades to a toast naming it, the rest render", async () => {
    const notifications = useNotificationsStore();
    const { wrapper } = mountAggregate({
      list_tasks: (args) => {
        if ((args as { id: string }).id === "vb") throw new Error("boom");
        return [aggTask("va", "Alpha task", "2026-07-08")];
      },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("Alpha task");
    expect(notifications.items.some((n) => n.kind === "error" && n.message.includes("Beta"))).toBe(true);
    // No blocking banner — partial results render.
    expect(wrapper.text()).not.toContain("boom");
  });

  it("shows the blocking banner only when every vault fails", async () => {
    const { wrapper } = mountAggregate({
      list_tasks: () => {
        throw new Error("all gone");
      },
    });
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(0);
    expect(wrapper.text()).toContain("Couldn't load tasks from any vault");
  });

  it("row actions carry the ROW's vault id in aggregate mode", async () => {
    const { wrapper, calls } = mountAggregate();
    await flushPromises();
    // First row is Beta task (vb): open + archive must hit vb, not va.
    await wrapper.get('[data-testid="task-open"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "open_task")?.args).toMatchObject({ id: "vb", path: "C:/vb/Tasks/Beta-task.md" });
    await wrapper.get('[data-testid="task-archive"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_status")?.args).toMatchObject({ id: "vb", status: "archived" });
  });

  it("aggregate mode shows the add row with the vault picker", async () => {
    const { wrapper } = mountAggregate();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-input"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="task-add-vault"]').exists()).toBe(true);
  });

  it("per-vault mode has no vault picker", async () => {
    const { wrapper } = mountView();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-add-vault"]').exists()).toBe(false);
  });

  it("aggregate add routes to the picked vault and merges the created task", async () => {
    const { wrapper, calls } = mountAggregateAttached();
    try {
      await flushPromises();
      // Picker defaults to the first vault (Alpha).
      expect(wrapper.get('[data-testid="task-add-vault"]').text()).toContain("Alpha");
      // Pick Beta from the teleported menu.
      await wrapper.get('[data-testid="task-add-vault"]').trigger("click");
      (document.body.querySelector('[data-testid="task-add-vault-option-vb"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-input"]').setValue("Cross task");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).toMatchObject({ id: "vb", title: "Cross task" });
      // Created task renders enriched with Beta's chip.
      const row = wrapper.findAll('[data-testid="task-row"]').find((r) => r.text().includes("Cross task"))!;
      expect(row.get('[data-testid="task-vault"]').attributes("title")).toBe("Beta");
    } finally {
      wrapper.unmount();
      document.body.innerHTML = "";
    }
  });

  it("shows a vault chip with the vault initial on aggregate rows", async () => {
    const { wrapper } = mountAggregate();
    await flushPromises();
    const chips = wrapper.findAll('[data-testid="task-vault"]');
    expect(chips).toHaveLength(2);
    expect(chips[0].text()).toBe("B"); // first row = Beta task
    expect(chips[0].attributes("title")).toBe("Beta");
  });

  it("shows no vault chip in per-vault mode", async () => {
    const { wrapper } = mountView();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-vault"]').exists()).toBe(false);
  });

  it("shows no drag grips in the aggregate view", async () => {
    // Manual is the default sort (taskSort.ts DEFAULT_PREF), so a fresh
    // aggregate mount would otherwise render grips — but `order` ranks are
    // per-vault numbers with no cross-vault rank space, so aggregate must
    // never permit a drag-to-reorder write (GAP-63).
    const { wrapper } = mountAggregate();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-drag"]').exists()).toBe(false);
  });

});
