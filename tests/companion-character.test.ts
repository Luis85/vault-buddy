import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import CompanionCharacter from "../src/components/CompanionCharacter.vue";

const startDragging = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging }),
}));

describe("CompanionCharacter", () => {
  beforeEach(() => {
    startDragging.mockClear();
  });

  it("emits toggle when the character is clicked", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    await wrapper.find("button.buddy").trigger("click");
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("applies the working class while an action runs", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: true } });
    expect(wrapper.find("button.buddy").classes()).toContain("working");
  });

  it("starts a window drag when the pointer moves past the threshold", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 70, screenY: 60 });
    expect(startDragging).toHaveBeenCalledTimes(1);
    // App needs to know so it can ignore the drag-induced focus loss
    expect(wrapper.emitted("drag-start")).toHaveLength(1);
    // the OS drag consumed the gesture — a trailing click must not toggle
    await buddy.trigger("click");
    expect(wrapper.emitted("toggle")).toBeUndefined();
  });

  it("treats a press with only tiny movement as a click", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 52, screenY: 51 });
    await buddy.trigger("pointerup");
    await buddy.trigger("click");
    expect(startDragging).not.toHaveBeenCalled();
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("captures the pointer on press so fast flicks can't escape the 64px buddy", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    const el = buddy.element as HTMLElement & {
      setPointerCapture: (id: number) => void;
      releasePointerCapture: (id: number) => void;
    };
    el.setPointerCapture = vi.fn();
    el.releasePointerCapture = vi.fn();

    await buddy.trigger("pointerdown", {
      button: 0,
      pointerId: 7,
      screenX: 50,
      screenY: 50,
    });
    expect(el.setPointerCapture).toHaveBeenCalledWith(7);

    await buddy.trigger("pointerup", { pointerId: 7 });
    expect(el.releasePointerCapture).toHaveBeenCalledWith(7);
  });

  it("toggles again on the click after a completed drag", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 90, screenY: 90 });
    await buddy.trigger("click"); // swallowed
    await buddy.trigger("click"); // genuine follow-up click
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });
});
