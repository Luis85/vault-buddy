import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import RecordingConfigTab from "../src/components/RecordingConfigTab.vue";

const config = {
  mode: "meeting",
  meetingFolder: "Meetings",
  voiceNoteFolder: "Voice Notes",
  bitrateKbps: 160,
  createNote: true,
  inputDevice: "USB Mic",
  outputDevice: null,
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: null as string | null,
  transcriptTimestamps: true,
  followUpTemplate: true,
  noteExtraFrontmatter: null as string | null,
  noteBodyTemplate: null as string | null,
  recordingDateFolders: true,
};
const devices = {
  inputs: [{ name: "USB Mic", isDefault: false }],
  outputs: [{ name: "Speakers", isDefault: true }],
};

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

function mountTab(opts: { config?: Partial<typeof config>; onSet?: (a: unknown) => unknown; onGet?: () => unknown } = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return opts.onGet ? opts.onGet() : { ...config, ...opts.config };
    if (cmd === "list_audio_devices") return devices;
    if (cmd === "set_capture_config") return opts.onSet?.(args) ?? null;
  });
  active = mount(RecordingConfigTab, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

const pick = async (wrapper: ReturnType<typeof mount>, testid: string, value: string | number) => {
  await wrapper.get(`[data-testid="${testid}"]`).trigger("click");
  (document.body.querySelector(`[data-testid="${testid}-option-${value}"]`) as HTMLElement).click();
  await flushPromises();
};

describe("RecordingConfigTab", () => {
  it("loads the config into the form", async () => {
    const { wrapper } = mountTab();
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="meeting-folder-input"]').element.value).toBe("Meetings");
  });

  it("does not save on mount", async () => {
    const { calls } = mountTab();
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);
  });

  it("debounces a folder edit and saves the whole struct with mode preserved", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("Inbox/Audio");
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);
    vi.advanceTimersByTime(600);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toMatchObject({
      id: "v1",
      cfg: { mode: "meeting", meetingFolder: "Inbox/Audio", voiceNoteFolder: "Voice Notes", recordingDateFolders: true },
    });
  });

  it("loads a note body template and debounces an edit before saving", async () => {
    const { wrapper, calls } = mountTab({
      config: { noteBodyTemplate: "## Summary\n{{type}}" },
    });
    await flushPromises();
    expect(
      wrapper.get<HTMLTextAreaElement>('[data-testid="note-body-template"]').element.value,
    ).toBe("## Summary\n{{type}}");

    await wrapper.get('[data-testid="note-body-template"]').setValue("## Notes\n{{vault}}");
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);
    vi.advanceTimersByTime(600);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toMatchObject({
      id: "v1",
      cfg: { noteBodyTemplate: "## Notes\n{{vault}}" },
    });
  });

  it("saves a toggle immediately (no debounce)", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();
    await wrapper.get('[data-testid="recording-date-folders-toggle"]').setValue(false);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config") as { args: { cfg: { recordingDateFolders: boolean } } };
    expect(set.args.cfg.recordingDateFolders).toBe(false);
  });

  it("saves a select change immediately", async () => {
    const { calls, wrapper } = mountTab();
    await flushPromises();
    await pick(wrapper, "bitrate-select", 192);
    const set = calls.find((c) => c.cmd === "set_capture_config") as { args: { cfg: { bitrateKbps: number } } };
    expect(set.args.cfg.bitrateKbps).toBe(192);
  });

  it("routes a folder rejection to the inline folder error", async () => {
    const { wrapper } = mountTab({
      onSet: () => {
        throw 'Configured recording folder must stay inside the vault: "../x"';
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("../x");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(wrapper.get('[data-testid="folder-error"]').text()).toContain("must stay inside the vault");
  });

  it("routes a non-folder failure to a form error", async () => {
    const { wrapper } = mountTab({
      onSet: () => {
        throw "Could not save capture settings: disk full";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="recording-date-folders-toggle"]').setValue(false);
    await flushPromises();
    expect(wrapper.get('[data-testid="recording-form-error"]').text()).toContain("disk full");
  });

  it("shows a load error and no form when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw "config unreadable";
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="recording-load-error"]').text()).toContain("config unreadable");
    expect(wrapper.find('[data-testid="meeting-folder-input"]').exists()).toBe(false);
  });
});
