import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import CaptureSettings from "../src/components/CaptureSettings.vue";

vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { logWarning } from "../src/logging";

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
  recordingDateFolders: true,
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
    onListLists?: () => unknown;
    documentsFolder?: string | null;
    documentDateFolders?: boolean;
    onGetDocuments?: () => unknown;
    onSetDocuments?: (args: unknown) => unknown;
  } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  // A dispatch table (not an if-chain): each command gets its own small,
  // independently-scored handler instead of one long branchy function —
  // keeps this mock under fallow's per-function complexity ceiling as the
  // command list grows.
  const handlers: Record<string, (args: unknown) => unknown> = {
    get_capture_config: () => ({ ...config, ...overrides.config }),
    list_audio_devices: () => overrides.devices ?? devices,
    set_capture_config: (args) => overrides.onSet?.(args),
    get_tasks_config: () =>
      overrides.onGetTasks
        ? overrides.onGetTasks()
        : { tasksFolder: overrides.tasksFolder ?? null },
    set_tasks_config: (args) => overrides.onSetTasks?.(args) ?? null,
    // The embedded TaskListSettings card reads the vault's lists at mount.
    list_task_lists: () => overrides.onListLists?.() ?? [],
    get_documents_config: () =>
      overrides.onGetDocuments
        ? overrides.onGetDocuments()
        : {
            documentsFolder: overrides.documentsFolder ?? null,
            documentDateFolders: overrides.documentDateFolders ?? true,
          },
    set_documents_config: (args) => overrides.onSetDocuments?.(args) ?? null,
  };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return handlers[cmd]?.(args);
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

  it("groups the form into buddy-settings-style sections with uppercase headers", async () => {
    // Aligns the vault-settings page with the Buddy settings page, which lays
    // every group out as an uppercase section header over a bordered card.
    const { wrapper } = await mountLoaded();
    const headers = wrapper.findAll("h2");
    const texts = headers.map((h) => h.text());
    expect(texts).toContain("Recording");
    expect(texts).toContain("Audio");
    const recording = headers.find((h) => h.text() === "Recording")!;
    expect(recording.classes()).toContain("uppercase");
    expect(recording.classes()).toContain("tracking-wide");
    // the section groups its controls inside a bordered card, like BuddySettings
    expect(wrapper.findAll(".rounded-xl.border").length).toBeGreaterThan(0);
  });

  it("renders the three domain super-group headings", async () => {
    // The form is further grouped into Recording/Tasks/Documents domain
    // super-groups, one level above the buddy-settings-style sections above
    // (e.g. "Companion note", "Tasks folder") — each wrapper carries its own
    // data-testid and a domain h2 as ITS OWN first heading, so this is
    // precise about which h2 belongs to the group vs. a nested sub-card.
    const { wrapper } = await mountLoaded();
    const groups: Array<[string, string]> = [
      ["group-recording", "Recording"],
      ["group-tasks", "Tasks"],
      ["group-documents", "Documents"],
    ];
    for (const [testid, heading] of groups) {
      const group = wrapper.get(`[data-testid="${testid}"]`);
      expect(group.get("h2").text()).toBe(heading);
    }
  });

  it("loads the config into the form", async () => {
    const { wrapper, calls } = await mountLoaded();
    expect(calls.map((c) => c.cmd)).toContain("get_capture_config");
    expect(calls.map((c) => c.cmd)).toContain("list_audio_devices");
    const meetingFolder = wrapper.get<HTMLInputElement>('[data-testid="meeting-folder-input"]');
    expect(meetingFolder.element.value).toBe("Meetings");
    const voiceNoteFolder = wrapper.get<HTMLInputElement>(
      '[data-testid="voice-note-folder-input"]',
    );
    expect(voiceNoteFolder.element.value).toBe("Voice Notes");
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
  });

  it("shows a distinct placeholder naming each mode's default folder", async () => {
    // One folder input per mode now (replacing the old single input whose
    // placeholder had to name both defaults at once), so each names only its
    // own mode's default.
    const { wrapper } = await mountLoaded();
    expect(
      wrapper.get('[data-testid="meeting-folder-input"]').attributes("placeholder"),
    ).toBe("Meetings");
    expect(
      wrapper.get('[data-testid="voice-note-folder-input"]').attributes("placeholder"),
    ).toBe("Voice Notes");
  });

  it("shows the output picker regardless of the stored mode", async () => {
    // Was gated on meeting mode; without a mode control the loopback device
    // must stay reachable (it applies whenever a meeting recording is made).
    const { wrapper } = await mountLoaded({ config: { mode: "voice-note" } });
    expect(wrapper.find('[data-testid="output-device-select"]').exists()).toBe(true);
  });

  it("saves the edited form through set_capture_config", async () => {
    const { wrapper, calls } = await mountLoaded();
    // Both folder inputs are edited so the recordingBundle adapter's
    // round-trip is proven in both directions for both fields, not just one.
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("Inbox/Audio");
    await wrapper.get('[data-testid="voice-note-folder-input"]').setValue("Personal/Notes");
    await pickOption(wrapper, "bitrate-select", 192);
    await pickOption(wrapper, "input-device-select", "");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toEqual({
      id: "v1",
      cfg: {
        mode: "meeting",
        meetingFolder: "Inbox/Audio",
        voiceNoteFolder: "Personal/Notes",
        bitrateKbps: 192,
        createNote: true,
        followUpTemplate: true,
        inputDevice: null,
        outputDevice: null,
        transcribe: false,
        transcriptionModel: "small",
        transcriptionLanguage: null,
        transcriptTimestamps: true,
        recordingDateFolders: true,
      },
    });
    expect(wrapper.text()).toContain("Saved");
  });

  it("clears the Saved confirmation when a field is edited", async () => {
    const { wrapper } = await mountLoaded();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.text()).toContain("Saved");
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("Elsewhere");
    expect(wrapper.text()).not.toContain("Saved ✓");
  });

  it("shows a folder error inline and keeps the form state", async () => {
    const { wrapper } = await mountLoaded({
      onSet: () => {
        throw "Configured recording folder must stay inside the vault: \"../x\"";
      },
    });
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("../x");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[data-testid="folder-error"]').text()).toContain(
      "must stay inside the vault",
    );
    const folder = wrapper.get<HTMLInputElement>('[data-testid="meeting-folder-input"]');
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
        meetingFolder: "Meetings",
        voiceNoteFolder: "Voice Notes",
        bitrateKbps: 160,
        createNote: true,
        followUpTemplate: true,
        inputDevice: "USB Mic",
        outputDevice: null,
        transcribe: true,
        transcriptionModel: "medium",
        transcriptionLanguage: "es",
        transcriptTimestamps: true,
        recordingDateFolders: true,
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

  it("round-trips the recording date-folders toggle through the recordingBundle adapter", async () => {
    // Proves recordingBundle carries recordingDateFolders on BOTH the get half
    // (loaded config -> checkbox) and the set half (checkbox edit -> saved
    // payload) — a dropped field on either half would silently break this.
    const { wrapper, calls } = await mountLoaded({ config: { recordingDateFolders: true } });
    const toggle = wrapper.get<HTMLInputElement>(
      '[data-testid="recording-date-folders-toggle"]',
    );
    expect(toggle.element.checked).toBe(true);
    await toggle.setValue(false);
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(
      (set?.args as { cfg: { recordingDateFolders: boolean } }).cfg.recordingDateFolders,
    ).toBe(false);
  });

  it("shows the documents date-folders toggle and saves it through set_documents_config", async () => {
    const { wrapper, calls } = await mountLoaded({ documentDateFolders: true });
    const toggle = wrapper.get<HTMLInputElement>(
      '[data-testid="document-date-folders-toggle"]',
    );
    expect(toggle.element.checked).toBe(true);
    await toggle.setValue(false);
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_documents_config");
    expect(set?.args).toEqual({
      id: "v1",
      documentsFolder: null,
      documentDateFolders: false,
    });
  });

  it("hydrates the date-folders toggle from disk even when the folder input was edited first (shared edit-guard regression)", async () => {
    // Regression: the date-folders toggle was added reusing
    // documentsFolderEdited — the SAME flag that guards the documents FOLDER
    // text input's hydration. Editing the folder before get_documents_config
    // resolves wrongly gated off the checkbox's OWN hydration from that same
    // response, leaving it stuck on its seeded default (true) and silently
    // reverting a persisted "flat" (false) choice on the next save.
    let resolveDocuments!: (v: unknown) => void;
    const { wrapper, calls } = await mountLoaded({
      onGetDocuments: () =>
        new Promise((resolve) => {
          resolveDocuments = resolve;
        }),
    });
    await wrapper.get('[data-testid="documents-folder-input"]').setValue("Mine");
    resolveDocuments({ documentsFolder: "Stored/Docs", documentDateFolders: false });
    await flushPromises();
    const toggle = wrapper.get<HTMLInputElement>(
      '[data-testid="document-date-folders-toggle"]',
    );
    expect(toggle.element.checked).toBe(false);
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_documents_config");
    expect(set?.args).toEqual({
      id: "v1",
      documentsFolder: "Mine",
      documentDateFolders: false,
    });
  });

  it("keeps the folder input hydrating from disk when the date-folders toggle is clicked first (shared edit-guard regression, symmetric case)", async () => {
    // Symmetric case: clicking the checkbox before get_documents_config
    // resolves must not block the FOLDER input's own hydration from that same
    // response — it would otherwise stay blank and Save would send
    // documentsFolder: null, clearing a persisted custom folder path.
    let resolveDocuments!: (v: unknown) => void;
    const { wrapper, calls } = await mountLoaded({
      onGetDocuments: () =>
        new Promise((resolve) => {
          resolveDocuments = resolve;
        }),
    });
    await wrapper.get('[data-testid="document-date-folders-toggle"]').setValue(false);
    resolveDocuments({ documentsFolder: "Stored/Docs", documentDateFolders: true });
    await flushPromises();
    const input = wrapper.get<HTMLInputElement>('[data-testid="documents-folder-input"]');
    expect(input.element.value).toBe("Stored/Docs");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_documents_config");
    expect(set?.args).toEqual({
      id: "v1",
      documentsFolder: "Stored/Docs",
      documentDateFolders: false,
    });
  });

  it("saves a checkbox-only edit even though the documents-config read was still in flight at save time (P2 drop regression)", async () => {
    // Regression: Save becomes clickable as soon as the capture-config
    // Promise.all resolves — well before get_documents_config returns.
    // Toggling ONLY the checkbox (the folder text stays untouched) and
    // saving while that read is still pending used to gate the WHOLE
    // documents save off documentsFolderEdited alone (loaded=false,
    // edited=false — the checkbox's own edit wasn't counted), skipping
    // set_documents_config entirely and silently dropping the checkbox
    // change while the form still showed "Saved ✓".
    let resolveDocuments!: (v: unknown) => void;
    const { wrapper, calls } = await mountLoaded({
      onGetDocuments: () =>
        new Promise((resolve) => {
          resolveDocuments = resolve;
        }),
    });
    await wrapper.get('[data-testid="document-date-folders-toggle"]').setValue(false);
    await wrapper.get("form").trigger("submit");
    // Let save() run as far as it can while the read is still pending (this
    // is the pending-load window itself) before resolving it — otherwise the
    // resolve could race ahead of save()'s own gate check and mask the bug.
    await flushPromises();
    resolveDocuments({ documentsFolder: null, documentDateFolders: true });
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_documents_config");
    expect(set?.args).toEqual({
      id: "v1",
      documentsFolder: null,
      documentDateFolders: false,
    });
  });

  it("does not clobber a persisted documentDateFolders with the seeded default when the folder is edited before the read resolves (P2 clobber regression)", async () => {
    // Regression: editing the folder text (which already satisfies the
    // loaded-or-edited gate via documentsFolderEdited) while
    // get_documents_config is still pending used to save IMMEDIATELY, with
    // documentDateFolders still at its seeded default (true) rather than the
    // disk's real value — overwriting a persisted "flat" (false) choice the
    // user never touched.
    let resolveDocuments!: (v: unknown) => void;
    const { wrapper, calls } = await mountLoaded({
      onGetDocuments: () =>
        new Promise((resolve) => {
          resolveDocuments = resolve;
        }),
    });
    await wrapper.get('[data-testid="documents-folder-input"]').setValue("Mine");
    await wrapper.get("form").trigger("submit");
    // Same reasoning as the drop-regression test above: give save() a chance
    // to act while the read is still unresolved before resolving it.
    await flushPromises();
    resolveDocuments({ documentsFolder: "Mine", documentDateFolders: false });
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_documents_config");
    expect(set?.args).toEqual({
      id: "v1",
      documentsFolder: "Mine",
      documentDateFolders: false,
    });
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

  it("reloads the lists card after the tasks folder is saved with a NEW value (Codex #53 re-review)", async () => {
    // The lists card (TaskListSettings) reads lists/config only at mount; a
    // persisted tasks-folder change swaps the root those lists live under, so
    // the card must remount (reload) — else a default/order save from the
    // stale card persists old-root list names against the new root. An
    // unchanged save must NOT remount (it would discard unsaved card edits).
    let lists = ["OldList"];
    const { wrapper, calls } = await mountLoaded({
      tasksFolder: "Tasks",
      onListLists: () => lists,
    });
    const cardLoads = () => calls.filter((c) => c.cmd === "list_task_lists").length;
    const before = cardLoads(); // the card's own mount-time read
    expect(before).toBeGreaterThan(0);
    // Change the folder on disk and in the input, then save the form. Two
    // lists so the card's order rows render them as text (one list renders
    // only inside the closed picker).
    lists = ["NewList", "NewToo"];
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Other/Tasks");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(cardLoads()).toBe(before + 1); // remounted → re-read the lists
    expect(wrapper.text()).toContain("NewList");
    // A second save with the folder unchanged leaves the card alone.
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(cardLoads()).toBe(before + 1);
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

  it("renders the self-contained Task lists settings card", async () => {
    // The lists settings object (defaultList/listOrder) saves through its own
    // command so a lists-config failure can't block the capture/folder saves.
    const { wrapper } = await mountLoaded();
    expect(wrapper.text()).toContain("Task lists");
    expect(wrapper.find('[data-testid="task-lists-save"]').exists()).toBe(true);
  });
});
