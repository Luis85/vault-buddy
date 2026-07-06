import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks } from "@tauri-apps/api/mocks";
import TranscriptionSummary from "../src/components/TranscriptionSummary.vue";
import { useCaptureStore } from "../src/stores/capture";
import { useVaultsStore } from "../src/stores/vaults";
import type { TranscriptionJob } from "../src/types";

function job(overrides: Partial<TranscriptionJob> = {}): TranscriptionJob {
  return {
    mp3: "a.mp3",
    vaultId: "v1",
    name: "Standup",
    phase: "transcribing",
    progress: 0.42,
    model: null,
    error: null,
    startedAtMs: Date.now(),
    ...overrides,
  };
}

describe("TranscriptionSummary", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("renders the active job with its percent and opens Transcriptions on click", async () => {
    const capture = useCaptureStore();
    capture.transcriptions = { "a.mp3": job({ progress: 0.42 }) };
    const vaults = useVaultsStore();
    const spy = vi.spyOn(vaults, "openTranscriptions");

    const wrapper = mount(TranscriptionSummary);
    const el = wrapper.get('[data-testid="transcription-summary"]');
    expect(el.attributes("role")).toBe("button");
    expect(el.text()).toContain('Transcribing "Standup"');
    expect(el.text()).toContain("42%");
    expect(el.text()).not.toContain("queued");

    await el.trigger("click");
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("appends the queued count when jobs are waiting behind the active one", () => {
    const capture = useCaptureStore();
    capture.transcriptions = {
      "a.mp3": job({ progress: 0.5 }),
      "b.mp3": job({ mp3: "b.mp3", name: "Idea", phase: "queued", progress: null }),
      "c.mp3": job({ mp3: "c.mp3", name: "Notes", phase: "queued", progress: null }),
    };
    const wrapper = mount(TranscriptionSummary);
    expect(wrapper.get('[data-testid="transcription-summary"]').text()).toContain(
      "+2 queued",
    );
  });

  it("omits the percent while preparing, since progress isn't known yet", () => {
    const capture = useCaptureStore();
    capture.transcriptions = { "a.mp3": job({ phase: "preparing", progress: null }) };
    const wrapper = mount(TranscriptionSummary);
    const text = wrapper.get('[data-testid="transcription-summary"]').text();
    expect(text).toContain('Transcribing "Standup"');
    expect(text).not.toMatch(/\d+%/);
  });

  it("shows a failed count when nothing is active but a job failed", () => {
    const capture = useCaptureStore();
    capture.transcriptions = {
      "failed.mp3": job({ mp3: "failed.mp3", name: "Oops", phase: "failed", progress: null }),
    };
    const wrapper = mount(TranscriptionSummary);
    expect(wrapper.get('[data-testid="transcription-summary"]').text()).toContain(
      "⚠ 1 transcription failed",
    );
  });

  it("pluralizes when more than one job failed", () => {
    const capture = useCaptureStore();
    capture.transcriptions = {
      "f1.mp3": job({ mp3: "f1.mp3", phase: "failed", progress: null }),
      "f2.mp3": job({ mp3: "f2.mp3", phase: "failed", progress: null }),
    };
    const wrapper = mount(TranscriptionSummary);
    expect(wrapper.get('[data-testid="transcription-summary"]').text()).toContain(
      "⚠ 2 transcriptions failed",
    );
  });

  it("prefers the active job over a failed one when both exist", () => {
    const capture = useCaptureStore();
    capture.transcriptions = {
      "a.mp3": job({ progress: 0.1 }),
      "failed.mp3": job({ mp3: "failed.mp3", phase: "failed", progress: null }),
    };
    const wrapper = mount(TranscriptionSummary);
    expect(wrapper.get('[data-testid="transcription-summary"]').text()).toContain(
      "Transcribing",
    );
  });

  it("renders nothing when idle: nothing active, nothing failed", () => {
    const wrapper = mount(TranscriptionSummary);
    expect(wrapper.find('[data-testid="transcription-summary"]').exists()).toBe(false);
  });

  it("renders nothing when the only finished jobs are done/cancelled, not failed", () => {
    const capture = useCaptureStore();
    capture.transcriptions = {
      "done.mp3": job({ mp3: "done.mp3", phase: "done", progress: 1 }),
      "cancelled.mp3": job({ mp3: "cancelled.mp3", phase: "cancelled", progress: null }),
    };
    const wrapper = mount(TranscriptionSummary);
    expect(wrapper.find('[data-testid="transcription-summary"]').exists()).toBe(false);
  });

  it("exposes the newest failed job's error via the chip's title", () => {
    const capture = useCaptureStore();
    capture.transcriptions = {
      "f1.mp3": job({
        mp3: "f1.mp3",
        name: "Older",
        phase: "failed",
        progress: null,
        error: "older error",
        startedAtMs: 100,
      }),
      "f2.mp3": job({
        mp3: "f2.mp3",
        name: "Newer",
        phase: "failed",
        progress: null,
        error: "whisper inference: out of memory",
        startedAtMs: 200,
      }),
    };
    const wrapper = mount(TranscriptionSummary);
    // finishedTranscriptions is newest-first (by startedAtMs) — the chip's
    // title should reflect the newest failure's reason, not the oldest.
    expect(wrapper.get('[data-testid="transcription-summary"]').attributes("title")).toBe(
      "whisper inference: out of memory",
    );
  });
});
