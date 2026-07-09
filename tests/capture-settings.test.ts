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
  followUpTemplate: true,
};

const devices = {
  inputs: [
    { name: "USB Mic", isDefault: false },
    { name: "Built-in Mic", isDefault: true },
  ],
  outputs: [{ name: "Speakers", isDefault: true }],
};

let lastWrapper: ReturnType<typeof mount> | null = null;

const mountLoaded = async (
  overrides: {
    config?: Partial<typeof config>;
    devices?: typeof devices;
    onSet?: (args: unknown) => unknown;
    tasksFolder?: string | null;
    onGetTasks?: () => unknown;
    onSetTasks?: (args: unknown) => unknown;
  } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return { ...config, ...overrides.config };
    if (cmd === "list_audio_devices") return overrides.devices ?? devices;
    if (cmd === "set_capture_config") return overrides.onSet?.(args);
    if (cmd === "get_tasks_config")
      return overrides.onGetTasks
        ? overrides.onGetTasks()
        : { tasksFolder: overrides.tasksFolder ?? null };
    if (cmd === "set_tasks_config") return overrides.onSetTasks?.(args) ?? null;
  });
  // attachTo document.body so the SelectMenu's Teleported popups land in a
  // queryable place; afterEach unmounts and clears the body.
  const wrapper = mount(CaptureSettings, {
    props: { vaultId: "v1" },
    attachTo: document.body,
  });
  lastWrapper = wrapper;
  await flushPromises();
  return { wrapper, calls };
};

// Open a SelectMenu dropdown and click one of its (Teleported) options.
const pickOption = async (
  wrapper: ReturnType<typeof mount>,
  testid: string,
  value: string | number,
) => {
  await wrapper.get(`[data-testid="${testid}"]`).trigger("click");
  (
    document.body.querySelector(`[data-testid="${testid}-option-${value}"]`) as HTMLElement
  ).click();
  await flushPromises();
};

describe("CaptureSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => {
    lastWrapper?.unmount();
    lastWrapper = null;
    document.body.innerHTML = "";
    clearMocks();
  });

  it("loads the config into the form", async () => {
    const { wrapper, calls } = await mountLoaded();
    expect(calls.map((c) => c.cmd)).toContain("get_capture_config");
    expect(calls.map((c) => c.cmd)).toContain("list_audio_devices");
    const folder = wrapper.get<HTMLInputElement>('[data-testid="folder-input"]');
    expect(folder.element.value).toBe("Meetings");
    expect(wrapper.get('[data-testid="bitrate-select"]').text()).toContain("160 kbps");
    expect(wrapper.get('[data-testid="input-device-select"]').text()).toContain("USB Mic");
  });

  it("System default is the first option in both device pickers", async () => {
    const { wrapper } = await mountLoaded();
    for (const testid of ["input-device-select", "output-device-select"]) {
      await wrapper.get(`[data-testid="${testid}"]`).trigger("click");
      const first = document.body.querySelectorAll('[role="option"]')[0];
      expect(first?.textContent?.trim()).toBe("System default");
      await wrapper.get(`[data-testid="${testid}"]`).trigger("click"); // close before the next
    }
  });

  it("marks a configured-but-absent device as not connected instead of dropping it", async () => {
    const { wrapper } = await mountLoaded({
      config: { inputDevice: "Unplugged Headset" },
    });
    expect(wrapper.get('[data-testid="input-device-select"]').text()).toContain(
      "Unplugged Headset (not connected)",
    );
  });

  it("renders no default recording mode control", async () => {
    // The mode is a per-recording choice made in the Record view now; the
    // stored config value is a pass-through the UI can no longer edit.
    const { wrapper } = await mountLoaded();
    expect(wrapper.find('[data-testid="mode-meeting"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="mode-voice-note"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Default recording mode");
    // The folder placeholder was mode-dependent; with no mode control it names
    // both per-type defaults.
    expect(
      wrapper.get('[data-testid="folder-input"]').attributes("placeholder"),
    ).toBe("Meetings or Voice Notes");
  });

  it("shows the output picker regardless of the stored mode", async () => {
    // Was gated on meeting mode; without a mode control the loopback device
    // must stay reachable (it applies whenever a meeting recording is made).
    const { wrapper } = await mountLoaded({ config: { mode: "voice-note" } });
    expect(wrapper.find('[data-testid="output-device-select"]').exists()).toBe(true);
  });

  it("saves the edited form through set_capture_config", async () => {
    const { wrapper, calls } = await mountLoaded();
    await wrapper.get('[data-testid="folder-input"]').setValue("Inbox/Audio");
    await pickOption(wrapper, "bitrate-select", 192);
    await pickOption(wrapper, "input-device-select", "");
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
        followUpTemplate: true,
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
    expect(wrapper.get('[data-testid="transcription-model-select"]').text()).toContain(
      "Medium",
    );
    expect(wrapper.get('[data-testid="transcription-language-select"]').text()).toContain(
      "Spanish",
    );
    const timestamps = wrapper.get<HTMLInputElement>(
      '[data-testid="transcript-timestamps-toggle"]',
    );
    expect(timestamps.element.checked).toBe(false);
  });

  it("saves transcription settings after enabling transcribe and picking a model/language", async () => {
    const { wrapper, calls } = await mountLoaded();
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await pickOption(wrapper, "transcription-model-select", "medium");
    await pickOption(wrapper, "transcription-language-select", "es");
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
        followUpTemplate: true,
        inputDevice: "USB Mic",
        outputDevice: null,
        transcribe: true,
        transcriptionModel: "medium",
        transcriptionLanguage: "es",
        transcriptTimestamps: true,
      },
    });
  });

  it("saves the follow-up template toggle", async () => {
    let saved: { cfg: { followUpTemplate: boolean } } | undefined;
    const { wrapper } = await mountLoaded({
      onSet: (args) => {
        saved = args as typeof saved;
      },
    });
    await wrapper.get('[data-testid="follow-up-toggle"]').setValue(false);
    await wrapper.get('[data-testid="save-button"]').trigger("click");
    await flushPromises();
    expect(saved?.cfg.followUpTemplate).toBe(false);
  });

  it("loads the tasks folder and saves it with the form Save (no dedicated button)", async () => {
    const { wrapper, calls } = await mountLoaded({ tasksFolder: "Inbox/Tasks" });
    const input = wrapper.get('[data-testid="tasks-folder-input"]');
    expect((input.element as HTMLInputElement).value).toBe("Inbox/Tasks");
    expect(wrapper.find('[data-testid="tasks-folder-save"]').exists()).toBe(false);
    await input.setValue("  Work/Tasks  ");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
      cmd: "set_tasks_config",
      args: { id: "v1", tasksFolder: "Work/Tasks" },
    });
  });

  it("clears the tasks folder to the default on save when emptied", async () => {
    const { wrapper, calls } = await mountLoaded({ tasksFolder: "Inbox/Tasks" });
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
      cmd: "set_tasks_config",
      args: { id: "v1", tasksFolder: null },
    });
  });

  it("shows a tasks-folder failure inline, withholds Saved ✓, and still saves the capture config", async () => {
    const { wrapper, calls } = await mountLoaded({
      onSetTasks: () => {
        throw "Configured tasks folder must stay inside the vault";
      },
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[data-testid="tasks-folder-error"]').text()).toContain(
      "must stay inside the vault",
    );
    expect(wrapper.text()).not.toContain("Saved ✓");
    // The two configs save independently — a tasks failure never blocks the
    // capture-config write that already happened.
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(true);
  });

  it("still saves the tasks folder when the capture-config save fails", async () => {
    const { wrapper, calls } = await mountLoaded({
      onSet: () => {
        throw "Could not save capture settings: disk full";
      },
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_tasks_config")).toBe(true);
    expect(wrapper.get('[data-testid="save-error"]').text()).toContain("disk full");
    expect(wrapper.text()).not.toContain("Saved ✓");
  });

  it("clears the Saved confirmation when the tasks folder is edited", async () => {
    const { wrapper } = await mountLoaded();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.text()).toContain("Saved ✓");
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Elsewhere");
    expect(wrapper.text()).not.toContain("Saved ✓");
  });

  it("does not write the tasks config while its read is still in flight", async () => {
    // Regression (Codex review on #42): the form is submittable before
    // get_tasks_config resolves (its read deliberately runs after the
    // capture-config `loading` gate flips). An unconditional set_tasks_config
    // in save() would send the default-seeded "" (→ null) and CLEAR a
    // configured tasks folder the form never got to see.
    const { wrapper, calls } = await mountLoaded({
      onGetTasks: () => new Promise(() => {}), // never resolves
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(true);
    expect(calls.some((c) => c.cmd === "set_tasks_config")).toBe(false);
    // The capture config alone saved — the confirmation still shows.
    expect(wrapper.text()).toContain("Saved ✓");
  });

  it("does not write the tasks config after its read failed and the field is untouched", async () => {
    const { wrapper, calls } = await mountLoaded({
      onGetTasks: () => {
        throw "config unreadable";
      },
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(true);
    expect(calls.some((c) => c.cmd === "set_tasks_config")).toBe(false);
  });

  it("saves a tasks folder the user typed even though its read failed", async () => {
    // An explicit edit is explicit intent — a failed read must not silently
    // discard what the user typed into the visible field.
    const { wrapper, calls } = await mountLoaded({
      onGetTasks: () => {
        throw "config unreadable";
      },
    });
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Mine");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
      cmd: "set_tasks_config",
      args: { id: "v1", tasksFolder: "Mine" },
    });
  });

  it("keeps a user edit made while the tasks-config read was still in flight", async () => {
    // Mirrors RecordMode's pre-load-toggle guard: the resolving read must not
    // clobber a field the user already owns.
    let resolveTasks!: (v: unknown) => void;
    const { wrapper, calls } = await mountLoaded({
      onGetTasks: () =>
        new Promise((resolve) => {
          resolveTasks = resolve;
        }),
    });
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Mine");
    resolveTasks({ tasksFolder: "Stored/Elsewhere" });
    await flushPromises();
    const input = wrapper.get<HTMLInputElement>('[data-testid="tasks-folder-input"]');
    expect(input.element.value).toBe("Mine");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
      cmd: "set_tasks_config",
      args: { id: "v1", tasksFolder: "Mine" },
    });
  });
});
