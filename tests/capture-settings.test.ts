import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import CaptureSettings from "../src/components/CaptureSettings.vue";

vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { logWarning } from "../src/logging";

const config = {
  mode: "meeting",
  recordingFolder: "Meetings",
  bitrateKbps: 160,
  createNote: true,
  inputDevice: "USB Mic",
  outputDevice: null,
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: null as string | null,
  transcriptTimestamps: true,
};

const devices = {
  inputs: [
    { name: "USB Mic", isDefault: false },
    { name: "Built-in Mic", isDefault: true },
  ],
  outputs: [{ name: "Speakers", isDefault: true }],
};

const mountLoaded = async (
  overrides: {
    config?: Partial<typeof config>;
    devices?: typeof devices;
    onSet?: (args: unknown) => unknown;
  } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return { ...config, ...overrides.config };
    if (cmd === "list_audio_devices") return overrides.devices ?? devices;
    if (cmd === "set_capture_config") return overrides.onSet?.(args);
  });
  const wrapper = mount(CaptureSettings, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("CaptureSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => clearMocks());

  it("loads the config into the form", async () => {
    const { wrapper, calls } = await mountLoaded();
    expect(calls.map((c) => c.cmd)).toContain("get_capture_config");
    expect(calls.map((c) => c.cmd)).toContain("list_audio_devices");
    const folder = wrapper.get<HTMLInputElement>('[data-testid="folder-input"]');
    expect(folder.element.value).toBe("Meetings");
    const bitrate = wrapper.get<HTMLSelectElement>('[data-testid="bitrate-select"]');
    expect(bitrate.element.value).toBe("160");
    const input = wrapper.get<HTMLSelectElement>('[data-testid="input-device-select"]');
    expect(input.element.value).toBe("USB Mic");
  });

  it("System default is the first option in both device pickers", async () => {
    const { wrapper } = await mountLoaded();
    for (const testid of ["input-device-select", "output-device-select"]) {
      const options = wrapper.get(`[data-testid="${testid}"]`).findAll("option");
      expect(options[0]!.text()).toBe("System default");
      expect(options[0]!.attributes("value")).toBe("");
    }
  });

  it("marks a configured-but-absent device as not connected instead of dropping it", async () => {
    const { wrapper } = await mountLoaded({
      config: { inputDevice: "Unplugged Headset" },
    });
    const select = wrapper.get<HTMLSelectElement>('[data-testid="input-device-select"]');
    expect(select.element.value).toBe("Unplugged Headset");
    expect(select.text()).toContain("Unplugged Headset (not connected)");
  });

  it("hides the output picker in voice-note mode", async () => {
    const { wrapper } = await mountLoaded({ config: { mode: "voice-note" } });
    expect(wrapper.find('[data-testid="output-device-select"]').exists()).toBe(false);
  });

  it("saves the edited form through set_capture_config", async () => {
    const { wrapper, calls } = await mountLoaded();
    await wrapper.get('[data-testid="folder-input"]').setValue("Inbox/Audio");
    await wrapper.get('[data-testid="bitrate-select"]').setValue("192");
    await wrapper.get('[data-testid="input-device-select"]').setValue("");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toEqual({
      id: "v1",
      cfg: {
        mode: "meeting",
        recordingFolder: "Inbox/Audio",
        bitrateKbps: 192,
        createNote: true,
        inputDevice: null,
        outputDevice: null,
        transcribe: false,
        transcriptionModel: "small",
        transcriptionLanguage: null,
        transcriptTimestamps: true,
      },
    });
    expect(wrapper.text()).toContain("Saved");
  });

  it("clears the Saved confirmation when a field is edited", async () => {
    const { wrapper } = await mountLoaded();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.text()).toContain("Saved");
    await wrapper.get('[data-testid="folder-input"]').setValue("Elsewhere");
    expect(wrapper.text()).not.toContain("Saved ✓");
  });

  it("shows a folder error inline and keeps the form state", async () => {
    const { wrapper } = await mountLoaded({
      onSet: () => {
        throw "Configured recording folder must stay inside the vault: \"../x\"";
      },
    });
    await wrapper.get('[data-testid="folder-input"]').setValue("../x");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[data-testid="folder-error"]').text()).toContain(
      "must stay inside the vault",
    );
    const folder = wrapper.get<HTMLInputElement>('[data-testid="folder-input"]');
    expect(folder.element.value).toBe("../x");
  });

  it("shows non-folder save failures as a form error", async () => {
    const { wrapper } = await mountLoaded({
      onSet: () => {
        throw "Could not save capture settings: disk full";
      },
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[data-testid="save-error"]').text()).toContain("disk full");
  });

  it("logs a warning through the log bridge when the save fails", async () => {
    const { wrapper } = await mountLoaded({
      onSet: () => {
        throw "Could not save capture settings: disk full";
      },
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("settings save failed"),
    );
  });

  it("shows the transcribe toggle reflecting the loaded value", async () => {
    const off = await mountLoaded({ config: { transcribe: false } });
    expect(
      off.wrapper.get<HTMLInputElement>('[data-testid="transcribe-toggle"]').element.checked,
    ).toBe(false);

    const on = await mountLoaded({ config: { transcribe: true } });
    expect(
      on.wrapper.get<HTMLInputElement>('[data-testid="transcribe-toggle"]').element.checked,
    ).toBe(true);
  });

  it("hides the model/language/timestamps controls while transcribe is off", async () => {
    const { wrapper } = await mountLoaded({ config: { transcribe: false } });
    expect(wrapper.find('[data-testid="transcription-model-select"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="transcription-language-select"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="transcript-timestamps-toggle"]').exists()).toBe(false);
  });

  it("shows the model/language/timestamps controls, loaded correctly, once transcribe is on", async () => {
    const { wrapper } = await mountLoaded({
      config: {
        transcribe: true,
        transcriptionModel: "medium",
        transcriptionLanguage: "es",
        transcriptTimestamps: false,
      },
    });
    const model = wrapper.get<HTMLSelectElement>('[data-testid="transcription-model-select"]');
    expect(model.element.value).toBe("medium");
    const language = wrapper.get<HTMLSelectElement>(
      '[data-testid="transcription-language-select"]',
    );
    expect(language.element.value).toBe("es");
    const timestamps = wrapper.get<HTMLInputElement>(
      '[data-testid="transcript-timestamps-toggle"]',
    );
    expect(timestamps.element.checked).toBe(false);
  });

  it("saves transcription settings after enabling transcribe and picking a model/language", async () => {
    const { wrapper, calls } = await mountLoaded();
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await wrapper.get('[data-testid="transcription-model-select"]').setValue("medium");
    await wrapper.get('[data-testid="transcription-language-select"]').setValue("es");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toEqual({
      id: "v1",
      cfg: {
        mode: "meeting",
        recordingFolder: "Meetings",
        bitrateKbps: 160,
        createNote: true,
        inputDevice: "USB Mic",
        outputDevice: null,
        transcribe: true,
        transcriptionModel: "medium",
        transcriptionLanguage: "es",
        transcriptTimestamps: true,
      },
    });
  });
});
