import { describe, it, expect, vi, afterEach } from "vitest";
import { mount } from "@vue/test-utils";
import RecordingBar from "../src/components/RecordingBar.vue";

describe("RecordingBar", () => {
  afterEach(() => vi.useRealTimers());

  it("shows elapsed time from startedAtMs", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(100_000));
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: 100_000 - 65_000, saving: false, warning: null },
    });
    expect(wrapper.text()).toContain("1:05");
  });

  it("emits stop on button click", async () => {
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: Date.now(), saving: false, warning: null },
    });
    await wrapper.get("button[aria-label='Stop recording']").trigger("click");
    expect(wrapper.emitted("stop")).toHaveLength(1);
  });

  it("shows saving state and disables stop", () => {
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: Date.now(), saving: true, warning: null },
    });
    expect(wrapper.text()).toContain("Saving");
    expect(
      wrapper.get("button[aria-label='Stop recording']").attributes("disabled"),
    ).toBeDefined();
  });

  it("renders a warning when present", () => {
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: Date.now(), saving: false, warning: "source lost: mic" },
    });
    expect(wrapper.text()).toContain("source lost: mic");
  });
});
