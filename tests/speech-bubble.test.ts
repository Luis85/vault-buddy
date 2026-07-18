import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import SpeechBubble from "../src/components/SpeechBubble.vue";

describe("SpeechBubble", () => {
  it("renders the greeting text", () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Good morning!", side: "right", valign: "middle" },
    });
    expect(wrapper.get('[data-testid="speech-bubble"]').text()).toContain(
      "Good morning!",
    );
  });

  it("reflects the buddy side and vertical alignment so the tail points home", () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Hi", side: "left", valign: "bottom" },
    });
    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).toContain("side-left");
    expect(bubble.classes()).toContain("valign-bottom");
  });

  it("centres the tail when the bubble sits level with the buddy", () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Hi", side: "right", valign: "middle" },
    });
    expect(wrapper.get('[data-testid="speech-bubble"]').classes()).toContain(
      "valign-middle",
    );
  });

  it("is inert by default — no interactive class, no activate emit", async () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Hi", side: "right", valign: "middle" },
    });
    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).not.toContain("clickable");
    // A plain greeting/ack must not fire the action, even if clicked.
    await bubble.trigger("click");
    expect(wrapper.emitted("activate")).toBeUndefined();
  });

  it("shows an interactive affordance and emits activate when clickable", async () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Update ready", side: "right", valign: "middle", clickable: true },
    });
    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).toContain("clickable");
    await bubble.trigger("click");
    expect(wrapper.emitted("activate")).toHaveLength(1);
  });

  it("activates from the keyboard when clickable", async () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Update ready", side: "right", valign: "middle", clickable: true },
    });
    await wrapper.get('[data-testid="speech-bubble"]').trigger("keydown.enter");
    expect(wrapper.emitted("activate")).toHaveLength(1);
  });
});
