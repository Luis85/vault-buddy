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

  it("emits rename with the edited title on submit", async () => {
    const wrapper = mount(RenamePrompt, {
      props: { savedMp3: "/v/2026-07-04 1405 Meeting.mp3", error: null },
    });
    await wrapper.get("input").setValue("2026-07-04 1405 Standup");
    await wrapper.get("form").trigger("submit");
    expect(wrapper.emitted("rename")).toEqual([["2026-07-04 1405 Standup"]]);
  });

  it("emits dismiss from the keep-name button", async () => {
    const wrapper = mount(RenamePrompt, {
      props: { savedMp3: "/v/2026-07-04 1405 Meeting.mp3", error: null },
    });
    await wrapper
      .get("button[aria-label='Keep the timestamp name']")
      .trigger("click");
    expect(wrapper.emitted("dismiss")).toHaveLength(1);
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
