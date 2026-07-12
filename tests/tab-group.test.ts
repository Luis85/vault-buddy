import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { h } from "vue";

import TabGroup from "../src/components/TabGroup.vue";

const TABS = [
  { id: "one", label: "One" },
  { id: "two", label: "Two" },
  { id: "three", label: "Three" },
];

function mountGroup(initial?: string) {
  return mount(TabGroup, {
    props: { tabs: TABS, ...(initial ? { initial } : {}) },
    slots: {
      one: () => h("p", { "data-testid": "c-one" }, "one body"),
      two: () => h("p", { "data-testid": "c-two" }, "two body"),
      three: () => h("p", { "data-testid": "c-three" }, "three body"),
    },
    attachTo: document.body,
  });
}

describe("TabGroup", () => {
  it("starts on the first tab and marks it selected", () => {
    const wrapper = mountGroup();
    expect(wrapper.get('[data-testid="tab-one"]').attributes("aria-selected")).toBe("true");
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("false");
    wrapper.unmount();
  });

  it("honors the initial prop", () => {
    const wrapper = mountGroup("two");
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("true");
    wrapper.unmount();
  });

  it("mounts every panel but shows only the active one", () => {
    const wrapper = mountGroup();
    // all slot bodies are in the DOM (eager mount)
    expect(wrapper.find('[data-testid="c-one"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="c-two"]').exists()).toBe(true);
    // inactive panels are hidden via v-show (display:none)
    expect(wrapper.get('[data-testid="panel-two"]').isVisible()).toBe(false);
    expect(wrapper.get('[data-testid="panel-one"]').isVisible()).toBe(true);
    wrapper.unmount();
  });

  it("switches the shown panel on tab click", async () => {
    const wrapper = mountGroup();
    await wrapper.get('[data-testid="tab-two"]').trigger("click");
    expect(wrapper.get('[data-testid="panel-two"]').isVisible()).toBe(true);
    expect(wrapper.get('[data-testid="panel-one"]').isVisible()).toBe(false);
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("true");
    wrapper.unmount();
  });

  it("moves between tabs with arrow keys and wraps", async () => {
    const wrapper = mountGroup();
    await wrapper.get('[data-testid="tab-one"]').trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("true");
    await wrapper.get('[data-testid="tab-two"]').trigger("keydown", { key: "ArrowLeft" });
    expect(wrapper.get('[data-testid="tab-one"]').attributes("aria-selected")).toBe("true");
    await wrapper.get('[data-testid="tab-one"]').trigger("keydown", { key: "ArrowLeft" });
    expect(wrapper.get('[data-testid="tab-three"]').attributes("aria-selected")).toBe("true"); // wrapped
    wrapper.unmount();
  });
});
