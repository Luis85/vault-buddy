import { describe, it, expect, vi, afterEach } from "vitest";
import { mount } from "@vue/test-utils";
import RecordingBar from "../src/components/RecordingBar.vue";

describe("RecordingBar", () => {
  afterEach(() => vi.useRealTimers());

  it("shows elapsed time from startedAtMs", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(100_000));
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: 100_000 - 65_000,
        saving: false,
        starting: false,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(wrapper.text()).toContain("1:05");
  });

  it("emits stop on button click", async () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: Date.now(),
        saving: false,
        starting: false,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    await wrapper.get("button[aria-label='Stop recording']").trigger("click");
    expect(wrapper.emitted("stop")).toHaveLength(1);
  });

  it("shows saving state and disables stop", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: Date.now(),
        saving: true,
        starting: false,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(wrapper.text()).toContain("Saving");
    expect(
      wrapper.get("button[aria-label='Stop recording']").attributes("disabled"),
    ).toBeDefined();
  });

  it("shows a starting label instead of the elapsed time while starting", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: null,
        saving: false,
        starting: true,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(wrapper.text()).toContain("Starting…");
    expect(wrapper.text()).not.toContain("Recording");
  });

  it("disables stop while starting", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: null,
        saving: false,
        starting: true,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(
      wrapper.get("button[aria-label='Stop recording']").attributes("disabled"),
    ).toBeDefined();
  });

  it("renders a warning when present", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: Date.now(),
        saving: false,
        starting: false,
        warning: "source lost: mic",
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(wrapper.text()).toContain("source lost: mic");
  });

  it("freezes elapsed while paused and excludes prior pauses", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(200_000));
    const wrapper = mount(RecordingBar, {
      props: {
        // started 100s ago, 20s of past pauses, paused again 10s ago:
        // elapsed = 100 - 20 - 10 = 70s
        startedAtMs: 100_000,
        saving: false,
        starting: false,
        warning: null,
        paused: true,
        pausedTotalMs: 20_000,
        pausedSinceMs: 190_000,
        level: 0,
      },
    });
    expect(wrapper.text()).toContain("Paused 1:10");
  });

  it("emits pause while recording and resume while paused", async () => {
    const base = {
      startedAtMs: Date.now(),
      saving: false,
      starting: false,
      warning: null,
      pausedTotalMs: 0,
      pausedSinceMs: null,
      level: 0,
    };
    const recording = mount(RecordingBar, { props: { ...base, paused: false } });
    await recording.get("button[aria-label='Pause recording']").trigger("click");
    expect(recording.emitted("pause")).toHaveLength(1);
    expect(recording.find("button[aria-label='Resume recording']").exists()).toBe(false);
    const paused = mount(RecordingBar, { props: { ...base, paused: true } });
    await paused.get("button[aria-label='Resume recording']").trigger("click");
    expect(paused.emitted("resume")).toHaveLength(1);
  });

  it("disables pause while starting or saving", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: null,
        saving: false,
        starting: true,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(
      wrapper.get("button[aria-label='Pause recording']").attributes("disabled"),
    ).toBeDefined();
  });

  it("renders the level meter width from the level prop", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: Date.now(),
        saving: false,
        starting: false,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0.4,
      },
    });
    const meter = wrapper.get('[data-testid="level-meter"]');
    expect(meter.attributes("style")).toContain("width: 40%");
  });
});
