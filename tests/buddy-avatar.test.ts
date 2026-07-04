import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import BuddyAvatar from "../src/components/BuddyAvatar.vue";
import { getCharacter } from "../src/characters";

// past the minimum delay plus the full random jitter — a burst is
// guaranteed to have been scheduled by then, whatever Math.random returned
const MAX_IDLE_DELAY_MS = 7001;

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

  describe("random idle bursts", () => {
    beforeEach(() => {
      vi.useFakeTimers();
      // the scheduler draws for the delay AND for the burst action
      // (play vs. glance); >= 0.5 deterministically picks "play"
      vi.spyOn(Math, "random").mockReturnValue(0.75);
    });

    afterEach(() => {
      vi.useRealTimers();
      vi.restoreAllMocks();
    });

    it("stands still at first, then plays one idle cycle and re-arms", async () => {
      const wrapper = mount(BuddyAvatar, { props: { characterId: "knight" } });
      const sheet = wrapper.find(".sprite .sheet");
      expect(sheet.classes()).not.toContain("playing");

      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      expect(sheet.classes()).toContain("playing");

      // the one-shot CSS animation finished — back to standing still
      await sheet.trigger("animationend");
      expect(sheet.classes()).not.toContain("playing");

      // and the next burst is already scheduled
      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      expect(sheet.classes()).toContain("playing");
    });

    it("waits at least the minimum delay before a burst", async () => {
      const wrapper = mount(BuddyAvatar, { props: { characterId: "elf" } });
      vi.advanceTimersByTime(2999);
      await nextTick();
      expect(wrapper.find(".sprite .sheet").classes()).not.toContain(
        "playing",
      );
    });

    it("never bursts while working — the run loop owns the strip", async () => {
      const wrapper = mount(BuddyAvatar, {
        props: { characterId: "knight", working: true },
      });
      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      const sheet = wrapper.find(".sprite .sheet");
      expect(sheet.classes()).toContain("running");
      expect(sheet.classes()).not.toContain("playing");
    });

    it("never bursts when animations are off", async () => {
      const wrapper = mount(BuddyAvatar, {
        props: { characterId: "knight", animated: false },
      });
      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      expect(wrapper.find(".sprite .sheet").classes()).not.toContain(
        "playing",
      );
    });

    it("stops a scheduled burst when work starts, resumes after", async () => {
      const wrapper = mount(BuddyAvatar, { props: { characterId: "dwarf" } });
      await wrapper.setProps({ working: true });
      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      expect(wrapper.find(".sprite .sheet").classes()).not.toContain(
        "playing",
      );

      await wrapper.setProps({ working: false });
      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      expect(wrapper.find(".sprite .sheet").classes()).toContain("playing");
    });

    it("glances the other way instead of playing, then turns back", async () => {
      // < 0.5 deterministically picks the mirror "glance" action, and
      // makes each delay exactly 3000 + 0.25 × 4000 = 4000ms
      vi.spyOn(Math, "random").mockReturnValue(0.25);
      const FLIP_DELAY_MS = 4000;
      const wrapper = mount(BuddyAvatar, { props: { characterId: "knight" } });
      const sheet = wrapper.find(".sprite .sheet");
      expect(sheet.classes()).not.toContain("flipped");

      vi.advanceTimersByTime(FLIP_DELAY_MS);
      await nextTick();
      expect(sheet.classes()).toContain("flipped");
      // a glance is not a strip animation
      expect(sheet.classes()).not.toContain("playing");

      // the glance re-arms the scheduler directly; next draw turns back
      vi.advanceTimersByTime(FLIP_DELAY_MS);
      await nextTick();
      expect(sheet.classes()).not.toContain("flipped");
    });

    it("faces forward again when animations are turned off", async () => {
      vi.spyOn(Math, "random").mockReturnValue(0.25);
      const wrapper = mount(BuddyAvatar, { props: { characterId: "elf" } });
      vi.advanceTimersByTime(MAX_IDLE_DELAY_MS);
      await nextTick();
      expect(wrapper.find(".sprite .sheet").classes()).toContain("flipped");

      await wrapper.setProps({ animated: false });
      expect(wrapper.find(".sprite .sheet").classes()).not.toContain(
        "flipped",
      );
    });
  });
});
