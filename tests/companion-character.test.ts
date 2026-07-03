import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import CompanionCharacter from "../src/components/CompanionCharacter.vue";

describe("CompanionCharacter", () => {
  it("emits toggle when the character is clicked", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    await wrapper.find("button.buddy").trigger("click");
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("applies the working class while an action runs", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: true } });
    expect(wrapper.find("button.buddy").classes()).toContain("working");
  });

  it("has a drag region handle for moving the window", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    expect(wrapper.find("[data-tauri-drag-region]").exists()).toBe(true);
  });
});
