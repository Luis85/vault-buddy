import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import RenamePrompt from "../src/components/RenamePrompt.vue";

describe("RenamePrompt", () => {
  it("prefills the input with the saved base name", () => {
    const wrapper = mount(RenamePrompt, {
      props: {
        savedMp3: "C:\\v\\Meetings\\2026\\07\\2026-07-04 1405 Meeting.mp3",
        error: null,
      },
    });
    const input = wrapper.get<HTMLInputElement>("input");
    expect(input.element.value).toBe("2026-07-04 1405 Meeting");
  });

  it("emits accept with the edited title on submit", async () => {
    const wrapper = mount(RenamePrompt, {
      props: { savedMp3: "/v/2026-07-04 1405 Meeting.mp3", error: null },
    });
    await wrapper.get("input").setValue("2026-07-04 1405 Standup");
    await wrapper.get("form").trigger("submit");
    expect(wrapper.emitted("accept")).toEqual([["2026-07-04 1405 Standup"]]);
  });

  it("has exactly one button with Accept text", () => {
    const wrapper = mount(RenamePrompt, {
      props: { savedMp3: "/v/2026-07-04 1405 Meeting.mp3", error: null },
    });
    const buttons = wrapper.findAll("button");
    expect(buttons).toHaveLength(1);
    expect(buttons[0].text()).toContain("Accept");
  });

  it("shows a rename error", () => {
    const wrapper = mount(RenamePrompt, {
      props: {
        savedMp3: "/v/2026-07-04 1405 Meeting.mp3",
        error: "Title is too long (max 120 characters)",
      },
    });
    expect(wrapper.text()).toContain("Title is too long");
  });
});
