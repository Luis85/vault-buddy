import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import CompanionCharacter from "../src/components/CompanionCharacter.vue";

const startDragging = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging }),
}));

const ipcCalls: Array<{ cmd: string; args: unknown }> = [];

describe("CompanionCharacter", () => {
  beforeEach(() => {
    startDragging.mockClear();
    ipcCalls.length = 0;
    mockIPC((cmd, args) => {
      ipcCalls.push({ cmd, args });
    });
  });

  afterEach(() => {
    clearMocks();
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
    // the OS drag consumed the gesture — a trailing mouse click must not
    // toggle (detail >= 1 marks a pointer-generated click)
    await buddy.trigger("click", { detail: 1 });
    expect(wrapper.emitted("toggle")).toBeUndefined();
  });

  it("recovers when the native drag consumes the release entirely", async () => {
    // Windows can swallow the release without a trailing click
    // (tauri-apps/tauri#10767) — the suppression must not eat the next
    // deliberate interaction
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 90, screenY: 90 });
    expect(startDragging).toHaveBeenCalledTimes(1);

    // no trailing click arrives; the user later hovers and clicks
    await buddy.trigger("pointermove", { screenX: 91, screenY: 91 });
    await buddy.trigger("click", { detail: 1 });
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("never swallows keyboard activation after a drag", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 90, screenY: 90 });

    // Enter/Space produce a click with detail 0 and no pointer events —
    // it can never be a drag's trailing click
    await buddy.trigger("click", { detail: 0 });
    expect(wrapper.emitted("toggle")).toHaveLength(1);
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

  it("opens the native context menu on right-click with the current settings", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    await wrapper.find("button.buddy").trigger("contextmenu");
    expect(ipcCalls).toEqual([
      { cmd: "show_buddy_menu", args: { animated: true, dragging: true } },
    ]);
    // the browser context menu must not appear alongside the native one —
    // the handler prevents the default
    expect(wrapper.emitted("toggle")).toBeUndefined();
  });

  it("stops all animation when animated is off", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: true, animated: false },
    });
    // .still overrides idle, hover and working animations via CSS
    expect(wrapper.find("button.buddy").classes()).toContain("still");
  });

  it("never starts a window drag when dragging is disabled", async () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, draggable: false },
    });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 120, screenY: 120 });
    expect(startDragging).not.toHaveBeenCalled();
    expect(wrapper.emitted("drag-start")).toBeUndefined();
    // the press stays a plain click and still opens the panel
    await buddy.trigger("pointerup");
    await buddy.trigger("click", { detail: 1 });
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("drops the grab cursor and drag hint when dragging is disabled", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, draggable: false },
    });
    const buddy = wrapper.find("button.buddy");
    expect(buddy.classes()).toContain("cursor-pointer");
    expect(buddy.classes()).not.toContain("cursor-grab");
    expect(buddy.attributes("aria-label")).toBe(
      "Vault Buddy — click to open the panel",
    );
  });

  it("toggles again on the click after a completed drag", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { screenX: 90, screenY: 90 });
    await buddy.trigger("click", { detail: 1 }); // swallowed trailing click
    await buddy.trigger("click", { detail: 1 }); // genuine follow-up click
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("shows the recording dot when recording", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, recording: true },
    });
    expect(wrapper.find(".rec-dot").exists()).toBe(true);
    expect(wrapper.get("button").classes()).toContain("recording");
  });

  it("hides the recording dot when idle", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    expect(wrapper.find(".rec-dot").exists()).toBe(false);
  });
});
