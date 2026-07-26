import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

import TaskEditor from "../src/components/TaskEditor.vue";
import type { AggTask } from "../src/types";

const t = (extra: Partial<AggTask> = {}): AggTask => ({
  path: "C:/v/Tasks/Sample.md",
  title: "Sample",
  status: "new",
  created: "2026-07-08",
  done: false,
  due: null,
  scheduled: null,
  priority: null,
  tags: [],
  list: "",
  order: null,
  id: null,
  description: null, parentId: null, parentLink: null,
  vaultId: "v",
  vaultName: "Vault",
  ...extra,
});

function mountEditor(task: AggTask) {
  return mount(TaskEditor, { props: { task, busy: false, lists: [] } });
}

describe("TaskEditor copy-id row", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows the task id and copies it to the clipboard on click", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const wrapper = mountEditor(t({ id: "abc12345" }));
    const idRow = wrapper.find('[data-testid="task-edit-id"]');
    expect(idRow.exists()).toBe(true);
    expect(idRow.text()).toBe("abc12345");
    await wrapper.find('[data-testid="task-edit-id-copy"]').trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith("abc12345");
  });

  it("hides the id row entirely when the task has no id", () => {
    const wrapper = mountEditor(t({ id: null }));
    expect(wrapper.find('[data-testid="task-edit-id"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-edit-id-copy"]').exists()).toBe(false);
  });
});

describe("TaskEditor scheduled (do date)", () => {
  it("sends scheduled when set and clearScheduled when emptied", async () => {
    // Set a do-date on a task that had none.
    const setW = mountEditor(t({ scheduled: null }));
    await setW.get('[data-testid="task-edit-scheduled"]').setValue("2026-07-20");
    await setW.get('[data-testid="task-edit-save"]').trigger("click");
    expect(setW.emitted("save")![0][0]).toMatchObject({ scheduled: "2026-07-20" });

    // Clear an existing do-date.
    const clrW = mountEditor(t({ scheduled: "2026-07-20" }));
    await clrW.get('[data-testid="task-edit-scheduled"]').setValue("");
    await clrW.get('[data-testid="task-edit-save"]').trigger("click");
    expect(clrW.emitted("save")![0][0]).toMatchObject({ clearScheduled: true });
  });

  it("shows distinct visible Due/Do labels beside the date inputs, not aria-label-only", () => {
    const wrapper = mountEditor(t());
    const due = wrapper.get('[data-testid="task-edit-due"]').element;
    const scheduled = wrapper.get('[data-testid="task-edit-scheduled"]').element;
    expect(due.previousElementSibling?.textContent).toBe("Due");
    expect(scheduled.previousElementSibling?.textContent).toBe("Do");
  });
});
