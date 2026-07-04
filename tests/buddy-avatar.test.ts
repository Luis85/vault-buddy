import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import BuddyAvatar from "../src/components/BuddyAvatar.vue";
import { getCharacter } from "../src/characters";

describe("BuddyAvatar", () => {
  it("renders the classic SVG blob by default", () => {
    const wrapper = mount(BuddyAvatar);
    expect(wrapper.find("svg.classic").exists()).toBe(true);
    expect(wrapper.find(".sprite").exists()).toBe(false);
  });

  it("renders a sprite character from its idle strip", () => {
    const wrapper = mount(BuddyAvatar, { props: { characterId: "knight" } });
    expect(wrapper.find("svg.classic").exists()).toBe(false);
    const sheet = wrapper.find(".sprite .sheet");
    expect(sheet.exists()).toBe(true);
    expect(sheet.attributes("style")).toContain(
      getCharacter("knight").sprite!.idle,
    );
  });

  it("switches to the run strip while working", () => {
    const wrapper = mount(BuddyAvatar, {
      props: { characterId: "wizard", working: true },
    });
    const sheet = wrapper.find(".sprite .sheet");
    expect(sheet.attributes("style")).toContain(
      getCharacter("wizard").sprite!.run,
    );
    expect(sheet.classes()).toContain("running");
  });

  it("marks the avatar still when animations are off", () => {
    const classic = mount(BuddyAvatar, { props: { animated: false } });
    expect(classic.find(".avatar").classes()).toContain("still");
    const sprite = mount(BuddyAvatar, {
      props: { characterId: "elf", animated: false },
    });
    expect(sprite.find(".avatar").classes()).toContain("still");
  });

  it("falls back to the classic buddy for unknown ids", () => {
    const wrapper = mount(BuddyAvatar, {
      props: { characterId: "totally-bogus" },
    });
    expect(wrapper.find("svg.classic").exists()).toBe(true);
  });
});
