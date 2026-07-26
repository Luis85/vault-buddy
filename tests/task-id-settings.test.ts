import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import TaskIdSettings from "../src/components/TaskIdSettings.vue";

describe("TaskIdSettings", () => {
  it("shows the error even while the toggle reads disabled (a refusal must stay legible, not hidden behind the very state it's rejecting)", () => {
    // Regression: the error paragraph used to live inside `v-if="enabled"`.
    // A refused DISABLE optimistically flips `enabled` to false right before
    // the save that rejects it, so the guard's count-and-remedy message
    // (design spec §2a) was hidden at exactly the moment it mattered. The
    // error must render whenever it's set, independent of the toggle's
    // current value.
    const wrapper = mount(TaskIdSettings, {
      props: {
        enabled: false,
        property: "",
        error: "This vault has 2 tasks with a parent, referencing Task IDs under the current property.",
        placeholder: "task-id",
      },
    });
    expect(wrapper.get('[data-testid="task-id-error"]').text()).toContain("referencing Task IDs");
  });

  it("still hides the property-name input while disabled (only the error is unconditional)", () => {
    // Scope guard: the fix above must not also un-gate the property input —
    // that field is meaningless (and was never editable) while IDs are off.
    const wrapper = mount(TaskIdSettings, {
      props: { enabled: false, property: "", error: "boom", placeholder: "task-id" },
    });
    expect(wrapper.find('[data-testid="task-id-property"]').exists()).toBe(false);
  });

  it("shows the error alongside the property input while enabled, as before", () => {
    const wrapper = mount(TaskIdSettings, {
      props: { enabled: true, property: "uid", error: "Invalid ID property name", placeholder: "task-id" },
    });
    expect(wrapper.find('[data-testid="task-id-property"]').exists()).toBe(true);
    expect(wrapper.get('[data-testid="task-id-error"]').text()).toBe("Invalid ID property name");
  });
});
