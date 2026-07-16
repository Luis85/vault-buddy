import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

import TranscriptionAppSettings from "../src/components/TranscriptionAppSettings.vue";

let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  clearMocks();
  vi.clearAllMocks();
});

function mountWith(useGpu = true, failSave = false) {
  const calls: { cmd: string; payload?: unknown }[] = [];
  mockIPC((cmd, payload) => {
    calls.push({ cmd, payload });
    if (cmd === "get_transcription_config") return { useGpu };
    if (cmd === "set_transcription_config") {
      if (failSave) throw new Error("disk full");
      return null;
    }
    return null;
  });
  active = mount(TranscriptionAppSettings, { attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TranscriptionAppSettings", () => {
  it("loads the app-global setting on mount and renders the toggle", async () => {
    const { wrapper } = mountWith(false);
    await flushPromises();
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="use-gpu-toggle"]').element.checked,
    ).toBe(false);
  });

  it("saves on toggle with the camelCase payload", async () => {
    const { wrapper, calls } = mountWith(true);
    await flushPromises();
    await wrapper.get('[data-testid="use-gpu-toggle"]').setValue(false);
    await flushPromises();
    const save = calls.find((c) => c.cmd === "set_transcription_config");
    expect(save?.payload).toEqual({ cfg: { useGpu: false } });
  });

  it("reverts the toggle and surfaces an error when the save fails", async () => {
    const { wrapper } = mountWith(true, true);
    await flushPromises();
    await wrapper.get('[data-testid="use-gpu-toggle"]').setValue(false);
    await flushPromises();
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="use-gpu-toggle"]').element.checked,
    ).toBe(true);
    expect(wrapper.get('[data-testid="use-gpu-error"]').text()).toContain("disk full");
  });
});
