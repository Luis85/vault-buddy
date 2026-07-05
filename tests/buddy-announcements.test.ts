import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
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

  it("announces recording-saved, transcription start and done", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.lastSavedFile = "/v/2026/07/a.mp3";
    await wrapper.vm.$nextTick();
    capture.transcribing = true;
    await wrapper.vm.$nextTick();
    capture.lastTranscribed = { mp3: "/v/2026/07/a.mp3" };
    await wrapper.vm.$nextTick();
    expect(spoken.some((t) => t.includes("saved"))).toBe(true);
    expect(spoken.some((t) => t.includes("Writing it down"))).toBe(true);
    expect(spoken.some((t) => t.includes("Transcript ready"))).toBe(true);
  });

  it("announces a transcription failure once", async () => {
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.transcriptError = "model missing";
    await wrapper.vm.$nextTick();
    expect(spoken.filter((t) => t.includes("didn't work"))).toHaveLength(1);
  });

  it("stays silent when Buddy messages is off", async () => {
    useSettingsStore().buddyMessagesEnabled = false;
    const wrapper = mount(Host);
    const capture = useCaptureStore();
    capture.status = "recording";
    capture.transcribing = true;
    capture.transcriptError = "boom";
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
