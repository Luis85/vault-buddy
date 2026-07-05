import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import RecordModeDialog from "../src/components/RecordModeDialog.vue";

describe("RecordModeDialog", () => {
  it("renders both options with the meeting default highlighted", () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "meeting" },
    });
    expect(wrapper.text()).toContain("Record in Personal");
    const meeting = wrapper.get('[data-testid="mode-meeting"]');
    const voiceNote = wrapper.get('[data-testid="mode-voice-note"]');
    expect(meeting.classes()).toContain("border-violet-400");
    expect(meeting.classes()).toContain("bg-violet-500/20");
    expect(voiceNote.classes()).not.toContain("border-violet-400");
    expect(voiceNote.classes()).toContain("border-white/10");
  });

  it("renders both options with the voice-note default highlighted", () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "voice-note" },
    });
    const meeting = wrapper.get('[data-testid="mode-meeting"]');
    const voiceNote = wrapper.get('[data-testid="mode-voice-note"]');
    expect(voiceNote.classes()).toContain("border-violet-400");
    expect(voiceNote.classes()).toContain("bg-violet-500/20");
    expect(meeting.classes()).not.toContain("border-violet-400");
    expect(meeting.classes()).toContain("border-white/10");
  });

  it("emits start with the clicked mode", async () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "meeting" },
    });
    await wrapper.get('[data-testid="mode-voice-note"]').trigger("click");
    expect(wrapper.emitted("start")).toEqual([["voice-note"]]);
  });

  it("emits start with meeting when the meeting option is clicked", async () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "voice-note" },
    });
    await wrapper.get('[data-testid="mode-meeting"]').trigger("click");
    expect(wrapper.emitted("start")).toEqual([["meeting"]]);
  });

  it("cancels on backdrop click but not on a click inside the card", async () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "meeting" },
    });
    await wrapper.get('[role="dialog"]').trigger("click");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    await wrapper.get('[data-testid="mode-meeting"]').trigger("click");
    // the option click above emits start, not another cancel
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });

  it("cancels when the close button is clicked", async () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "meeting" },
    });
    await wrapper.get('[aria-label="Cancel recording"]').trigger("click");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });

  it("cancels on Escape and stops the event from bubbling", async () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "meeting" },
    });
    const stopPropagation = vitestSpyOnStopPropagation();
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "Escape" });
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    stopPropagation.restore();
  });
});

// A minimal helper to verify stopPropagation was actually invoked by the
// .stop modifier — a real KeyboardEvent lets us observe defaultPrevented
// and a custom flag instead of mocking Vue's event handling internals.
function vitestSpyOnStopPropagation() {
  const original = Event.prototype.stopPropagation;
  let called = false;
  Event.prototype.stopPropagation = function (this: Event) {
    called = true;
    return original.call(this);
  };
  return {
    restore: () => {
      Event.prototype.stopPropagation = original;
      expect(called).toBe(true);
    },
  };
}
