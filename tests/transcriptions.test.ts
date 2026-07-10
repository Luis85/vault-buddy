import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Transcriptions from "../src/components/Transcriptions.vue";
import { useCaptureStore } from "../src/stores/capture";
import { useVaultsStore } from "../src/stores/vaults";
import type { TranscriptionJob } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

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

describe("Transcriptions", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
    vi.useRealTimers();
  });

  it("shows an empty state when nothing is active, queued, or finished", () => {
    const wrapper = mount(Transcriptions);
    expect(wrapper.text()).toContain("No transcriptions yet.");
  });

  it("renders the active transcribing job with a progress bar and cancels it", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ progress: 0.42, startedAtMs: Date.now() - 5000 }),
    };
    const wrapper = mount(Transcriptions);

    const active = wrapper.get('[data-testid="transcription-active"]');
    expect(active.text()).toContain("Standup");
    expect(active.text()).toContain("v1");
    expect(active.text()).toContain("42%");
    const bar = wrapper.get('[data-testid="transcription-progress"]');
    expect(bar.attributes("aria-valuenow")).toBe("42");

    await wrapper.get('[data-testid="transcription-cancel"]').trigger("click");
    await flushPromises();
    expect(calls).toContainEqual({
      cmd: "cancel_transcription",
      args: { path: "a.mp3" },
    });
  });

  it("renders the download percentage for an active downloading job", () => {
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "downloading", progress: 0.3 }),
    };
    const wrapper = mount(Transcriptions);
    expect(wrapper.get('[data-testid="transcription-active"]').text()).toContain("30%");
  });

  it("shows an honest indeterminate download state — no percent — when the model's progress is unknown", () => {
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "downloading", progress: null, model: null }),
    };
    const wrapper = mount(Transcriptions);
    const active = wrapper.get('[data-testid="transcription-active"]');
    expect(active.text()).toContain("Downloading model…");
    expect(active.text()).not.toMatch(/\d+%/);
    const spinner = wrapper.get('[data-testid="transcription-progress"]');
    expect(spinner.classes()).toContain("animate-spin");
    // Regression: the spinner's aria-label was hardcoded to "Preparing…"
    // regardless of phase, so a screen reader announced "Preparing…" while
    // the visible label correctly said "Downloading model…". The aria-label
    // must track the same phase text the visible label shows.
    expect(spinner.attributes("aria-label")).toBe("Downloading model…");
  });

  it("renders the vault name for active and queued jobs, falling back to the id when the vault isn't known", () => {
    const vaults = useVaultsStore();
    vaults.vaults = [
      { id: "v1", name: "Work Vault", path: "/vaults/work", open: true },
    ];
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ vaultId: "v1" }),
      "b.mp3": job({
        mp3: "b.mp3",
        name: "Idea",
        vaultId: "v9",
        phase: "queued",
        progress: null,
      }),
    };
    const wrapper = mount(Transcriptions);

    const active = wrapper.get('[data-testid="transcription-active"]');
    expect(active.text()).toContain("Work Vault");
    expect(active.text()).not.toContain("v1");

    // v9 isn't in the seeded vault list — this is a cross-vault view, so a
    // queued job may belong to a vault this window hasn't discovered. Falls
    // back to the raw id rather than showing nothing.
    const queued = wrapper.get('[data-testid="transcription-queued"]');
    expect(queued.text()).toContain("v9");
  });

  it("renders an indeterminate spinner (no percent) while preparing", () => {
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "preparing", progress: null }),
    };
    const wrapper = mount(Transcriptions);
    const active = wrapper.get('[data-testid="transcription-active"]');
    expect(active.text()).not.toMatch(/\d+%/);
    expect(wrapper.get('[data-testid="transcription-progress"]').classes()).toContain(
      "animate-spin",
    );
  });

  it("shows an honest indeterminate transcribing label — no percent — when a transcribing job's progress is unknown", () => {
    // Distinct from the "preparing" spinner test above: this pins
    // phaseLabel's OWN "transcribing" branch (job.progress != null ? ...%
    // : "Transcribing…"), which a phase-only test can't exercise.
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "transcribing", progress: null }),
    };
    const wrapper = mount(Transcriptions);
    const active = wrapper.get('[data-testid="transcription-active"]');
    expect(active.text()).toContain("Transcribing…");
    expect(active.text()).not.toMatch(/\d+%/);
  });

  it("shows the waiting-for-recording label when nothing is active yet", () => {
    const store = useCaptureStore();
    store.waitingForRecording = true;
    const wrapper = mount(Transcriptions);
    expect(wrapper.text()).toContain("Waiting for the recording to finish…");
    expect(wrapper.find('[data-testid="transcription-progress"]').exists()).toBe(false);
  });

  it("renders a queued job with a cancel button", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.transcriptions = {
      "b.mp3": job({ mp3: "b.mp3", name: "Idea", phase: "queued", progress: null }),
    };
    const wrapper = mount(Transcriptions);

    const queuedSection = wrapper.get('[data-testid="transcription-queued"]');
    expect(queuedSection.text()).toContain("Idea");
    expect(queuedSection.text()).toContain("Waiting");

    await queuedSection.get('[data-testid="transcription-cancel"]').trigger("click");
    await flushPromises();
    expect(calls).toContainEqual({
      cmd: "cancel_transcription",
      args: { path: "b.mp3" },
    });
  });

  it("renders finished jobs: done -> Open, failed -> error + Re-transcribe, cancelled -> Re-transcribe", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.transcriptions = {
      "done.mp3": job({
        mp3: "done.mp3",
        name: "Standup",
        phase: "done",
        progress: 1,
        startedAtMs: 3000,
      }),
      "failed.mp3": job({
        mp3: "failed.mp3",
        name: "Oops",
        phase: "failed",
        progress: null,
        error: "boom",
        startedAtMs: 2000,
      }),
      "cancelled.mp3": job({
        mp3: "cancelled.mp3",
        name: "Skipped",
        phase: "cancelled",
        progress: null,
        startedAtMs: 1000,
      }),
    };
    const wrapper = mount(Transcriptions);
    const finished = wrapper.get('[data-testid="transcription-finished"]');
    expect(finished.text()).toContain("Standup");
    expect(finished.text()).toContain("Oops");
    expect(finished.text()).toContain("boom");
    expect(finished.text()).toContain("Skipped");

    const openButtons = finished.findAll('[data-testid="transcription-open"]');
    const retryButtons = finished.findAll('[data-testid="transcription-retranscribe"]');
    expect(openButtons).toHaveLength(1);
    expect(retryButtons).toHaveLength(2);

    await openButtons[0]!.trigger("click");
    await flushPromises();
    expect(calls).toContainEqual({
      cmd: "open_transcript",
      args: { path: "done.mp3" },
    });

    await retryButtons[0]!.trigger("click");
    await flushPromises();
    expect(calls).toContainEqual({
      cmd: "retranscribe",
      args: { path: "failed.mp3" },
    });

    await retryButtons[1]!.trigger("click");
    await flushPromises();
    expect(calls).toContainEqual({
      cmd: "retranscribe",
      args: { path: "cancelled.mp3" },
    });
  });

  it("dismisses a finished row, removing it (and its error) from the list", async () => {
    // The failed row (with its error text) had no dismiss control — it just
    // sat there. Every finished row now carries a dismiss button that clears
    // it via the store's terminal-only dismissTranscription.
    const store = useCaptureStore();
    store.transcriptions = {
      "failed.mp3": job({
        mp3: "failed.mp3",
        name: "Oops",
        phase: "failed",
        progress: null,
        error: "boom",
        startedAtMs: 2000,
      }),
    };
    const wrapper = mount(Transcriptions);
    const finished = wrapper.get('[data-testid="transcription-finished"]');
    expect(finished.text()).toContain("Oops");
    expect(finished.text()).toContain("boom");

    await wrapper.get('[data-testid="transcription-dismiss"]').trigger("click");
    await flushPromises();

    // The row — and the whole finished section, since it was the only one —
    // is gone, and the underlying job is cleared from the store.
    expect(wrapper.find('[data-testid="transcription-finished"]').exists()).toBe(false);
    expect(store.transcriptions["failed.mp3"]).toBeUndefined();
  });

  it("shows a stuck hint only once a transcribing job's progress has stalled past the threshold", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(0));
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "transcribing", progress: 0.5, startedAtMs: 0 }),
    };
    const wrapper = mount(Transcriptions);
    expect(wrapper.find('[data-testid="transcription-stuck-hint"]').exists()).toBe(false);

    vi.advanceTimersByTime(90_000);
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="transcription-stuck-hint"]').exists()).toBe(false);

    // A repeated, unchanged progress event (whisper re-reporting the same %)
    // must NOT reset the stuck clock.
    store.upsert("a.mp3", { phase: "transcribing", progress: 0.5 });
    await wrapper.vm.$nextTick();

    vi.advanceTimersByTime(40_000); // 130s since the first observation
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="transcription-stuck-hint"]').exists()).toBe(true);
  });

  it("clears the stuck hint once real progress arrives, restarting the clock", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(0));
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "transcribing", progress: 0.5, startedAtMs: 0 }),
    };
    const wrapper = mount(Transcriptions);

    vi.advanceTimersByTime(110_000);
    store.upsert("a.mp3", { phase: "transcribing", progress: 0.6 }); // genuine advance
    await wrapper.vm.$nextTick();

    vi.advanceTimersByTime(110_000); // only 110s since the real advance
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="transcription-stuck-hint"]').exists()).toBe(false);
  });

  it("keeps the stuck hint across a component remount — the clock lives in the store, not a local ref", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(0));
    const store = useCaptureStore();
    store.transcriptions = {
      "a.mp3": job({ phase: "transcribing", progress: 0.5, startedAtMs: 0 }),
    };
    let wrapper = mount(Transcriptions);

    vi.advanceTimersByTime(130_000); // past the 2-minute threshold
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="transcription-stuck-hint"]').exists()).toBe(true);

    wrapper.unmount();
    wrapper = mount(Transcriptions); // a component-local clock would restart here
    await wrapper.vm.$nextTick();

    // No time has advanced since the remount — this only stays true if the
    // "since" timestamp survived in the store instead of resetting to now.
    expect(wrapper.find('[data-testid="transcription-stuck-hint"]').exists()).toBe(true);
  });
});
