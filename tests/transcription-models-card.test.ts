import { mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

import TranscriptionModelsCard from "../src/components/TranscriptionModelsCard.vue";

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  vi.clearAllMocks();
});

const MODELS = [
  { id: "base", fileName: "ggml-base.bin", present: false, sizeBytes: null, approxDownloadBytes: 148_000_000 },
  { id: "small", fileName: "ggml-small.bin", present: true, sizeBytes: 487_654_321, approxDownloadBytes: 488_000_000 },
  { id: "vad", fileName: "ggml-silero-v5.1.2.bin", present: true, sizeBytes: 885_098, approxDownloadBytes: 885_098 },
];

function mountWith(failDelete = false) {
  const calls: { cmd: string; payload?: unknown }[] = [];
  mockIPC((cmd, payload) => {
    calls.push({ cmd, payload });
    if (cmd === "list_transcription_models") return MODELS;
    if (cmd === "delete_transcription_model") {
      if (failDelete) throw new Error("still in use");
      return null;
    }
    return null;
  });
  active = mount(TranscriptionModelsCard, { attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TranscriptionModelsCard", () => {
  it("lists every artifact with real sizes for present and approx for absent", async () => {
    const { wrapper } = mountWith();
    await flushPromises();
    expect(wrapper.get('[data-testid="model-row-small"]').text()).toContain("465 MB");
    expect(wrapper.get('[data-testid="model-row-base"]').text()).toContain("not downloaded");
    expect(wrapper.get('[data-testid="model-row-base"]').text()).toContain("141 MB");
    // Absent rows have no delete affordance.
    expect(wrapper.find('[data-testid="model-delete-base"]').exists()).toBe(false);
  });

  it("delete requires the in-panel confirm; cancel makes no IPC call", async () => {
    const { wrapper, calls } = mountWith();
    await flushPromises();
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    // Confirm state visible, nothing deleted yet.
    expect(calls.some((c) => c.cmd === "delete_transcription_model")).toBe(false);
    await wrapper.get('[data-testid="model-cancel-small"]').trigger("click");
    expect(calls.some((c) => c.cmd === "delete_transcription_model")).toBe(false);
    // Confirm path actually deletes and re-lists.
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    await wrapper.get('[data-testid="model-confirm-small"]').trigger("click");
    await flushPromises();
    const del = calls.find((c) => c.cmd === "delete_transcription_model");
    expect(del?.payload).toEqual({ id: "small" });
    expect(calls.filter((c) => c.cmd === "list_transcription_models").length).toBe(2);
  });

  it("surfaces a failed delete inline and keeps the row", async () => {
    const { wrapper } = mountWith(true);
    await flushPromises();
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    await wrapper.get('[data-testid="model-confirm-small"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="models-error"]').text()).toContain("still in use");
    expect(wrapper.find('[data-testid="model-row-small"]').exists()).toBe(true);
  });

  it("disables every other row's delete while one delete is in flight", async () => {
    // The busy guard serializes deletes card-wide: two concurrent deletes
    // would race the worker's single purge wake (final review Minor 2 —
    // the spec's busy-serialization bullet, previously untested).
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    mockIPC(async (cmd) => {
      if (cmd === "list_transcription_models") return MODELS;
      if (cmd === "delete_transcription_model") {
        await gate;
        return null;
      }
      return null;
    });
    active = mount(TranscriptionModelsCard, { attachTo: document.body });
    const wrapper = active;
    await flushPromises();
    await wrapper.get('[data-testid="model-delete-small"]').trigger("click");
    await wrapper.get('[data-testid="model-confirm-small"]').trigger("click");
    // small's delete is now awaiting the gate — vad's delete must be inert.
    expect(
      wrapper.get('[data-testid="model-delete-vad"]').attributes("disabled"),
    ).toBeDefined();
    release?.();
    await flushPromises();
    expect(
      wrapper.get('[data-testid="model-delete-vad"]').attributes("disabled"),
    ).toBeUndefined();
  });
});
