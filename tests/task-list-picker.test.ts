import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it } from "vitest";

import TaskListPicker from "../src/components/TaskListPicker.vue";

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  document.body.innerHTML = "";
});

function mountPicker(props: Partial<InstanceType<typeof TaskListPicker>["$props"]> = {}) {
  active = mount(TaskListPicker, {
    props: { modelValue: "", lists: ["Inbox", "Next"], dataTestid: "list-picker", ...props },
    attachTo: document.body,
  });
  return active;
}

const optionTexts = () =>
  [...document.body.querySelectorAll('[role="option"]')].map((o) => o.textContent?.trim());

describe("TaskListPicker", () => {
  it("offers No list, the lists in order, and New list…", async () => {
    const w = mountPicker();
    await w.get('[data-testid="list-picker"]').trigger("click");
    expect(optionTexts()).toEqual(["No list", "Inbox", "Next", "New list…"]);
  });

  it("emits the picked list and shows No list for the root", async () => {
    const w = mountPicker();
    await w.get('[data-testid="list-picker"]').trigger("click");
    (document.body.querySelector('[data-testid="list-picker-option-Next"]') as HTMLElement).click();
    await flushPromises();
    expect(w.emitted("update:modelValue")).toEqual([["Next"]]);
  });

  it("hides New list… when creation is not allowed", async () => {
    const w = mountPicker({ allowCreate: false });
    await w.get('[data-testid="list-picker"]').trigger("click");
    expect(optionTexts()).toEqual(["No list", "Inbox", "Next"]);
  });

  it("New list… swaps to an inline input and emits create on confirm", async () => {
    const w = mountPicker();
    await w.get('[data-testid="list-picker"]').trigger("click");
    (document.body.querySelector('[data-testid="list-picker-option-__new__"]') as HTMLElement).click();
    await flushPromises();
    const input = w.get('[data-testid="list-picker-new-name"]');
    await input.setValue("Someday");
    await w.get('[data-testid="list-picker-new-confirm"]').trigger("click");
    expect(w.emitted("create")).toEqual([["Someday"]]);
    // No modelValue emit — the parent selects the list once creation lands.
    expect(w.emitted("update:modelValue")).toBeUndefined();
  });

  it("cancel and Escape leave new-list mode without emitting, keeping the prior pick", async () => {
    const w = mountPicker({ modelValue: "Inbox" });
    await w.get('[data-testid="list-picker"]').trigger("click");
    (document.body.querySelector('[data-testid="list-picker-option-__new__"]') as HTMLElement).click();
    await flushPromises();
    await w.get('[data-testid="list-picker-new-cancel"]').trigger("click");
    expect(w.find('[data-testid="list-picker-new-name"]').exists()).toBe(false);
    expect(w.emitted("create")).toBeUndefined();
    expect(w.get('[data-testid="list-picker"]').text()).toContain("Inbox");
  });

  it("Escape in the name input stays inside the picker (GAP-27 class)", async () => {
    const w = mountPicker();
    await w.get('[data-testid="list-picker"]').trigger("click");
    (document.body.querySelector('[data-testid="list-picker-option-__new__"]') as HTMLElement).click();
    await flushPromises();
    let reachedWindow = false;
    const spy = () => {
      reachedWindow = true;
    };
    window.addEventListener("keydown", spy);
    try {
      await w
        .get('[data-testid="list-picker-new-name"]')
        .trigger("keydown", { key: "Escape", isComposing: false });
      expect(w.find('[data-testid="list-picker-new-name"]').exists()).toBe(false);
      expect(reachedWindow).toBe(false);
    } finally {
      window.removeEventListener("keydown", spy);
    }
  });

  it("leaves new-list mode when the parent selects the created list", async () => {
    const w = mountPicker();
    await w.get('[data-testid="list-picker"]').trigger("click");
    (document.body.querySelector('[data-testid="list-picker-option-__new__"]') as HTMLElement).click();
    await flushPromises();
    await w.setProps({ modelValue: "Someday", lists: ["Inbox", "Next", "Someday"] });
    await flushPromises();
    expect(w.find('[data-testid="list-picker-new-name"]').exists()).toBe(false);
    expect(w.get('[data-testid="list-picker"]').text()).toContain("Someday");
  });
});
