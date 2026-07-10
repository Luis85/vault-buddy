import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import BuddyAvatar from "../src/components/BuddyAvatar.vue";
import CompanionCharacter from "../src/components/CompanionCharacter.vue";

const ipcCalls: Array<{ cmd: string; args: unknown }> = [];
// The OS drag must go through the Rust-side start_buddy_drag command (which
// drops requests whose mouse button is already up again) — never through the
// raw window API.
const dragCalls = () => ipcCalls.filter((c) => c.cmd === "start_buddy_drag");

// The standard mouse flick: press, then a move past the threshold with the
// button still physically down. Centralized so the gesture contract lives in
// one place (adding the `buttons`/`pointerType` fields touched every call
// site before this existed).
async function flick(
  buddy: ReturnType<ReturnType<typeof mount>["find"]>,
  opts: { screenX?: number; screenY?: number; pointerType?: string } = {},
) {
  const { screenX = 90, screenY = 90, pointerType = "mouse" } = opts;
  await buddy.trigger("pointerdown", {
    button: 0,
    pointerType,
    screenX: 50,
    screenY: 50,
  });
  await buddy.trigger("pointermove", { buttons: 1, pointerType, screenX, screenY });
}

describe("CompanionCharacter", () => {
  beforeEach(() => {
    ipcCalls.length = 0;
    mockIPC((cmd, args) => {
      ipcCalls.push({ cmd, args });
      if (cmd === "start_buddy_drag") return true;
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
    await flick(buddy, { screenX: 70, screenY: 60 });
    expect(dragCalls()).toHaveLength(1);
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
    await flick(buddy);
    expect(dragCalls()).toHaveLength(1);

    // no trailing click arrives; the user later hovers and clicks
    await buddy.trigger("pointermove", { screenX: 91, screenY: 91 });
    await buddy.trigger("click", { detail: 1 });
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("never swallows keyboard activation after a drag", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await flick(buddy);

    // Enter/Space produce a click with detail 0 and no pointer events —
    // it can never be a drag's trailing click
    await buddy.trigger("click", { detail: 0 });
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("treats a press with only tiny movement as a click", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", {
      buttons: 1,
      screenX: 52,
      screenY: 51,
    });
    await buddy.trigger("pointerup");
    await buddy.trigger("click");
    expect(dragCalls()).toHaveLength(0);
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("tells the command which pointer started the drag", async () => {
    // The Rust guard only re-checks the mouse button; a touch/pen contact
    // reports buttons=1 to the webview but need not surface as a mouse
    // button, so the command must know not to drop it.
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    await flick(wrapper.find("button.buddy"), { pointerType: "touch" });
    expect(dragCalls()).toHaveLength(1);
    expect(dragCalls()[0].args).toEqual({ pointerType: "touch" });
  });

  it("never starts an OS drag from a move that arrives after the button is up", async () => {
    // A fast flick can leave a queued pointermove that is only dispatched
    // after the button was already released. Starting the native drag from
    // it hands Windows a buttonless WM_NCLBUTTONDOWN — the "sticky window"
    // move loop that glues the buddy to the cursor.
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", {
      buttons: 0,
      screenX: 90,
      screenY: 90,
    });
    expect(dragCalls()).toHaveLength(0);
    expect(wrapper.emitted("drag-start")).toBeUndefined();
    // the gesture was still a flick, not an open-the-panel intent — its
    // trailing click must stay swallowed
    await buddy.trigger("click", { detail: 1 });
    expect(wrapper.emitted("toggle")).toBeUndefined();
  });

  it("keeps a press with a stale sub-threshold move a plain click", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", {
      buttons: 0,
      screenX: 52,
      screenY: 51,
    });
    await buddy.trigger("pointerup");
    await buddy.trigger("click", { detail: 1 });
    expect(dragCalls()).toHaveLength(0);
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
    await buddy.trigger("pointermove", {
      buttons: 1,
      screenX: 120,
      screenY: 120,
    });
    expect(dragCalls()).toHaveLength(0);
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
    await flick(buddy);
    await buddy.trigger("click", { detail: 1 }); // swallowed trailing click
    await buddy.trigger("click", { detail: 1 }); // genuine follow-up click
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  // A click and a drop each bob the buddy once (one idle burst), driven by an
  // incrementing nonce passed down to BuddyAvatar. The nonce is the observable
  // contract; whether the sprite actually animates is BuddyAvatar's own tested
  // concern (and gated on the animations-off setting there).
  const playNonce = (w: ReturnType<typeof mount>) =>
    w.findComponent(BuddyAvatar).props("playNonce");

  it("bobs the buddy once on a real click", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    expect(playNonce(wrapper)).toBe(0);
    await wrapper.find("button.buddy").trigger("click");
    expect(playNonce(wrapper)).toBe(1);
  });

  it("bobs the buddy when a drag ends via the trailing click, not on pickup", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await flick(buddy);
    // picking the buddy up must not bob — only landing does
    expect(playNonce(wrapper)).toBe(0);
    await buddy.trigger("click", { detail: 1 }); // the drop's trailing click
    expect(playNonce(wrapper)).toBe(1);
    // the trailing click still must not open the panel
    expect(wrapper.emitted("toggle")).toBeUndefined();
  });

  it("bobs the buddy when the drag ends with no trailing click", async () => {
    // Windows can swallow the release (tauri-apps/tauri#10767); the drop is
    // then detected from the first hover move — the bob must still fire.
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    const buddy = wrapper.find("button.buddy");
    await flick(buddy);
    await buddy.trigger("pointermove", { screenX: 91, screenY: 91 });
    expect(playNonce(wrapper)).toBe(1);
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

  it("shows a steady amber dot while paused", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, recording: true, paused: true },
    });
    const dot = wrapper.get(".rec-dot");
    expect(dot.classes()).toContain("bg-amber-400");
    expect(dot.classes()).not.toContain("bg-red-500");
  });

  it("shows a violet transcribing dot while transcribing", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, transcribing: true },
    });
    const dot = wrapper.get(".transcribe-dot");
    expect(dot.classes()).toContain("bg-violet-400");
    expect(wrapper.get("button").classes()).toContain("transcribing");
  });

  it("hides the transcribing dot while recording takes precedence", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, transcribing: true, recording: true },
    });
    expect(wrapper.find(".transcribe-dot").exists()).toBe(false);
    expect(wrapper.find(".rec-dot").exists()).toBe(true);
  });

  // The drawn character sits bottom-center of a larger 64x64 box, so a dot
  // pinned to the box's top-right corner floats up-and-away from the buddy.
  // Both status dots hug the buddy: centered over it, just above the head.
  it("floats the transcribing dot centered above the buddy, not in the corner", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, transcribing: true },
    });
    const dot = wrapper.get(".transcribe-dot");
    expect(dot.classes()).toContain("left-1/2");
    expect(dot.classes()).toContain("-translate-x-1/2");
    expect(dot.classes()).not.toContain("-right-1");
  });

  it("floats the recording dot centered above the buddy, not in the corner", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, recording: true },
    });
    const dot = wrapper.get(".rec-dot");
    expect(dot.classes()).toContain("left-1/2");
    expect(dot.classes()).toContain("-translate-x-1/2");
    expect(dot.classes()).not.toContain("-right-1");
  });
});
