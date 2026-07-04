import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import CaptureSettings from "../src/components/CaptureSettings.vue";

const config = {
  mode: "meeting",
  recordingFolder: "Meetings",
  bitrateKbps: 160,
  createNote: true,
  inputDevice: "USB Mic",
  outputDevice: null,
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
      },
    });
    expect(wrapper.text()).toContain("Saved");
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
});
