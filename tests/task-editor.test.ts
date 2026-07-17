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
  priority: null,
  tags: [],
  list: "",
  order: null,
  id: null,
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
