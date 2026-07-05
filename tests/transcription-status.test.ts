import { describe, expect, it, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import TranscriptionStatus from "../src/components/TranscriptionStatus.vue";
import { useCaptureStore } from "../src/stores/capture";

describe("TranscriptionStatus", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is empty when idle", () => {
    const w = mount(TranscriptionStatus);
    expect(w.text()).toBe("");
  });

  it("shows a transcribing message", () => {
    const store = useCaptureStore();
    store.transcribing = true;
    const w = mount(TranscriptionStatus);
    expect(w.text()).toContain("Transcribing");
  });

  it("shows model download progress", () => {
    const store = useCaptureStore();
    store.transcribing = true;
    store.modelDownload = { model: "small", received: 5, total: 10 };
    const w = mount(TranscriptionStatus);
    expect(w.text()).toContain("small");
    expect(w.text()).toContain("50%");
  });

  it("shows an error with a retry button", () => {
    const store = useCaptureStore();
    store.transcriptError = "model unavailable";
    store.transcriptFailedMp3 = "/v/m.mp3";
    const w = mount(TranscriptionStatus);
    expect(w.text()).toContain("model unavailable");
    expect(w.find("button").exists()).toBe(true);
  });

  it("offers an Open in Obsidian button after a transcription finishes", () => {
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    const w = mount(TranscriptionStatus);
    const btn = w.get('[data-testid="open-transcript"]');
    expect(btn.text()).toContain("Open in Obsidian");
  });
});
