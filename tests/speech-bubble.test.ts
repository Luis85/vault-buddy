import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
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
});
