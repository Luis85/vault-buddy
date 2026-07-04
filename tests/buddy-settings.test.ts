import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import BuddySettings from "../src/components/BuddySettings.vue";
import { CHARACTERS } from "../src/characters";
import { useSettingsStore } from "../src/stores/settings";

describe("BuddySettings", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });

  it("shows every character with the current one selected", () => {
    const wrapper = mount(BuddySettings);
    const options = wrapper.findAll(".character-option");
    expect(options).toHaveLength(CHARACTERS.length);
    for (const c of CHARACTERS) expect(wrapper.text()).toContain(c.name);
    // classic is the persisted default
    expect(
      wrapper.find('[aria-label="Choose Classic"]').attributes("aria-checked"),
    ).toBe("true");
  });

  it("selecting a character updates and persists the store", async () => {
    const wrapper = mount(BuddySettings);
    await wrapper.find('[aria-label="Choose Knight"]').trigger("click");
    expect(useSettingsStore().character).toBe("knight");
    expect(localStorage.getItem("vault-buddy.character")).toBe("knight");
    expect(
      wrapper.find('[aria-label="Choose Knight"]').attributes("aria-checked"),
    ).toBe("true");
  });

  it("mirrors the dragging toggle", async () => {
    const wrapper = mount(BuddySettings);
    const toggle = wrapper.find("#dragging-toggle");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    await toggle.setValue(false);
    expect(useSettingsStore().draggingEnabled).toBe(false);
  });

  it("mirrors the animations toggle", async () => {
    const wrapper = mount(BuddySettings);
    const toggle = wrapper.find("#animations-toggle");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    await toggle.setValue(false);
    expect(useSettingsStore().animationsEnabled).toBe(false);
  });
});
