import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import TaskRow from "../src/components/TaskRow.vue";
import type { AggTask } from "../src/types";

const task = (): AggTask => ({
  path: "p", title: "T", status: "new", created: "2026-07-01", done: false,
  due: null, scheduled: null, priority: null, tags: [], list: "", order: null,
  id: null, description: null, vaultId: "v1", vaultName: "V",
});

describe("TaskRow open emit", () => {
  it("emits open with the MouseEvent so the container can route ctrl/meta", async () => {
    const wrapper = mount(TaskRow, {
      props: { task: task(), busy: false, isAggregate: false, editing: false },
    });
    await wrapper.find('[data-testid="task-open"]').trigger("click");
    const ev = wrapper.emitted("open")?.[0]?.[0];
    expect(ev).toBeInstanceOf(MouseEvent);
  });
});
