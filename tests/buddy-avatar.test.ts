import { flushPromises,mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

import { getCharacter } from "../src/characters";
import BuddyAvatar from "../src/components/BuddyAvatar.vue";

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

  describe("greeting hover", () => {
    it("wiggles while the pointer is over the buddy", async () => {
      const wrapper = mount(BuddyAvatar, { props: { characterId: "knight" } });
      const avatar = wrapper.find(".avatar");
      expect(avatar.classes()).not.toContain("hovering");

      await avatar.trigger("pointerenter");
      expect(avatar.classes()).toContain("hovering");

      await avatar.trigger("pointerleave");
      expect(avatar.classes()).not.toContain("hovering");
    });

    it("clears a stuck hover when the page is hidden (hide to tray)", async () => {
      // hiding the window never delivers pointerleave — without the
      // visibilitychange reset the buddy comes back wiggling
      const wrapper = mount(BuddyAvatar, { props: { characterId: "knight" } });
      const avatar = wrapper.find(".avatar");
      await avatar.trigger("pointerenter");
      expect(avatar.classes()).toContain("hovering");

      document.dispatchEvent(new Event("visibilitychange"));
      await nextTick();
      expect(avatar.classes()).not.toContain("hovering");
    });

    it("clears a stuck hover when the window blurs", async () => {
      const wrapper = mount(BuddyAvatar); // classic avatar wiggles too
      const avatar = wrapper.find(".avatar");
      await avatar.trigger("pointerenter");
      expect(avatar.classes()).toContain("hovering");

      window.dispatchEvent(new Event("blur"));
      await nextTick();
      expect(avatar.classes()).not.toContain("hovering");
    });
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

    it("glances the other way briefly, then snaps back", async () => {
      // < 0.5 deterministically picks the mirror "glance" action, and
      // makes the burst delay exactly 3000 + 0.25 × 4000 = 4000ms and the
      // glance duration exactly 700 + 0.25 × 800 = 900ms
      vi.spyOn(Math, "random").mockReturnValue(0.25);
      const wrapper = mount(BuddyAvatar, { props: { characterId: "knight" } });
      const sheet = wrapper.find(".sprite .sheet");
      expect(sheet.classes()).not.toContain("flipped");

      vi.advanceTimersByTime(4000);
      await nextTick();
      expect(sheet.classes()).toContain("flipped");
      // a glance is not a strip animation
      expect(sheet.classes()).not.toContain("playing");

      // a glance is a quick look — it returns on its own well before the
      // next burst would be due
      vi.advanceTimersByTime(900);
      await nextTick();
      expect(sheet.classes()).not.toContain("flipped");

      // and the regular scheduler is re-armed afterwards
      vi.advanceTimersByTime(4000);
      await nextTick();
      expect(sheet.classes()).toContain("flipped");
    });

    it("honors a left home direction, glancing to the right", async () => {
      vi.spyOn(Math, "random").mockReturnValue(0.25);
      const wrapper = mount(BuddyAvatar, {
        props: { characterId: "wizard", facing: "left" },
      });
      const sheet = wrapper.find(".sprite .sheet");
      // home direction is left — mirrored from the start, no timer needed
      expect(sheet.classes()).toContain("flipped");

      // a glance looks AWAY from home, i.e. back to the unmirrored side
      vi.advanceTimersByTime(4000);
      await nextTick();
      expect(sheet.classes()).not.toContain("flipped");

      vi.advanceTimersByTime(900);
      await nextTick();
      expect(sheet.classes()).toContain("flipped");
    });

    it("plays one idle burst on demand when the play nonce changes", async () => {
      // CompanionCharacter bumps `playNonce` on a click/drop to acknowledge the
      // interaction with one idle bob (the same burst the scheduler fires).
      const wrapper = mount(BuddyAvatar, {
        props: { characterId: "knight", playNonce: 0 },
      });
      const sheet = wrapper.find(".sprite .sheet");
      expect(sheet.classes()).not.toContain("playing");

      await wrapper.setProps({ playNonce: 1 });
      await flushPromises();
      expect(sheet.classes()).toContain("playing");
    });

    it("does not play a demand burst while animations are off", async () => {
      const wrapper = mount(BuddyAvatar, {
        props: { characterId: "knight", playNonce: 0, animated: false },
      });
      await wrapper.setProps({ playNonce: 1 });
      await flushPromises();
      expect(wrapper.find(".sprite .sheet").classes()).not.toContain("playing");
    });

    it("faces forward again when animations are turned off mid-glance", async () => {
      vi.spyOn(Math, "random").mockReturnValue(0.25);
      const wrapper = mount(BuddyAvatar, { props: { characterId: "elf" } });
      // 4000ms puts us inside the glance window (returns on its own at 4900)
      vi.advanceTimersByTime(4000);
      await nextTick();
      expect(wrapper.find(".sprite .sheet").classes()).toContain("flipped");

      await wrapper.setProps({ animated: false });
      expect(wrapper.find(".sprite .sheet").classes()).not.toContain(
        "flipped",
      );
    });
  });
});
