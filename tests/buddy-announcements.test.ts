import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { defineComponent } from "vue";

import { useBuddyAnnouncements } from "../src/composables/useBuddyAnnouncements";
import { useCaptureStore } from "../src/stores/capture";
import { useSettingsStore } from "../src/stores/settings";

const Host = defineComponent({
  setup() {
    useBuddyAnnouncements();
  },
  render: () => null,
});

describe("useBuddyAnnouncements", () => {
  let spoken: string[];
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    spoken = [];
    mockIPC((cmd, args) => {
      if (cmd === "announce") spoken.push((args as { text: string }).text);
    });
  });
  afterEach(() => clearMocks());

  it("announces when a recording starts", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.status = "recording";
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("Listening"))).toBe(true);
  });

  it("announces pause and resume while recording", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.status = "recording";
    await wrapper.vm.$nextTick();
    capture.paused = true;
    await wrapper.vm.$nextTick();
    capture.paused = false;
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("Taking a breather"))).toBe(true);
    expect(spoken.some((t) => t.includes("Back to it"))).toBe(true);
  });

  it("does not announce a resume when a paused recording is stopped", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.status = "recording";
    capture.paused = true;
    await wrapper.vm.$nextTick();
    spoken.length = 0; // drop the pause line; we only care about what stop says
    // stopping clears status + paused (resetRecordingState), then saves — the
    // paused→false flip must NOT read as a resume
    capture.resetRecordingState();
    capture.lastSavedFile = "/v/2026/07/a.mp3";
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("Back to it"))).toBe(false);
    expect(spoken.some((t) => t.includes("saved"))).toBe(true);
  });

  it("announces recording-saved, transcription start and done", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.lastSavedFile = "/v/2026/07/a.mp3";
    await wrapper.vm.$nextTick();
    capture.transcriptions = {
      "/v/2026/07/a.mp3": {
        mp3: "/v/2026/07/a.mp3",
        vaultId: "v1",
        name: "a",
        phase: "preparing",
        progress: null,
        model: null,
        error: null,
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    capture.transcriptions = {
      "/v/2026/07/a.mp3": {
        ...capture.transcriptions["/v/2026/07/a.mp3"],
        phase: "done",
        progress: 1,
      },
    };
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("saved"))).toBe(true);
    expect(spoken.some((t) => t.includes("Writing it down"))).toBe(true);
    expect(spoken.some((t) => t.includes("Transcript ready"))).toBe(true);
  });

  it("stays quiet for a skipped (kept-existing) transcript, but still announces a genuine finish", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.transcriptions = {
      "/v/a.mp3": {
        mp3: "/v/a.mp3",
        vaultId: "v1",
        name: "a",
        phase: "preparing",
        progress: null,
        model: null,
        error: null,
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    capture.transcriptions = {
      "/v/a.mp3": {
        ...capture.transcriptions["/v/a.mp3"],
        phase: "done",
        progress: 1,
        skipped: true,
      },
    };
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("Transcript ready"))).toBe(false);

    // A genuine (non-skipped) finish for a different job still announces —
    // the suppression must be specific to skipped jobs, not all "done"s.
    capture.transcriptions = {
      ...capture.transcriptions,
      "/v/b.mp3": {
        mp3: "/v/b.mp3",
        vaultId: "v1",
        name: "b",
        phase: "done",
        progress: 1,
        model: null,
        error: null,
        startedAtMs: Date.now() + 1,
      },
    };
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("Transcript ready"))).toBe(true);
  });

  it("announces a transcription failure once, speaking its reason", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.transcriptions = {
      "/v/a.mp3": {
        mp3: "/v/a.mp3",
        vaultId: "v1",
        name: "a",
        phase: "failed",
        progress: null,
        model: null,
        error: "model missing",
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    // The job's specific reason replaces the generic line, spoken exactly once.
    expect(spoken.filter((t) => t.includes("model missing"))).toHaveLength(1);
    expect(spoken.some((t) => t.includes("didn't work"))).toBe(false);
  });

  it("falls back to the generic failure line when a failed job has no reason", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.transcriptions = {
      "/v/b.mp3": {
        mp3: "/v/b.mp3",
        vaultId: "v1",
        name: "b",
        phase: "failed",
        progress: null,
        model: null,
        error: null,
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("didn't work"))).toBe(true);
    expect(spoken.filter((t) => t.includes("didn't work"))).toHaveLength(1);
  });

  it("announces the capture error's reason, not the generic line", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.error = "disk is full";
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("disk is full"))).toBe(true);
    expect(spoken.some((t) => t.includes("didn't work"))).toBe(false);
    expect(spoken.filter((t) => t.includes("disk is full"))).toHaveLength(1);
  });

  it("stays silent when Buddy messages is off", async () => {
    useSettingsStore().buddyMessagesEnabled = false;
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.status = "recording";
    capture.transcriptions = {
      "/v/a.mp3": {
        mp3: "/v/a.mp3",
        vaultId: "v1",
        name: "a",
        phase: "preparing",
        progress: null,
        model: null,
        error: null,
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    capture.transcriptions = {
      "/v/a.mp3": { ...capture.transcriptions["/v/a.mp3"], phase: "failed", error: "boom" },
    };
    await wrapper.vm.$nextTick();
    expect(spoken).toEqual([]);
  });

  it("does not re-announce a state already present at mount", async () => {
    // a recording already in progress when the buddy window (re)mounts must not
    // fire "Listening…" — watchers are non-immediate for exactly this.
    const capture = useCaptureStore();
    capture.status = "recording";
    const wrapper = mount(Host);
    await wrapper.vm.$nextTick();
    expect(spoken).toEqual([]);
  });
});
