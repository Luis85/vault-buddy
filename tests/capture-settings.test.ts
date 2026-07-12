import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logBreadcrumb: vi.fn(), logWarning: vi.fn() }));

import CaptureSettings from "../src/components/CaptureSettings.vue";

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
const devices = { inputs: [{ name: "USB Mic", isDefault: false }], outputs: [{ name: "Speakers", isDefault: true }] };

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
});
afterEach(() => {
  active?.unmount();
  active = null;
  clearMocks();
  document.body.innerHTML = "";
});

function mountShell() {
  mockIPC((cmd) => {
    if (cmd === "get_capture_config") return config;
    if (cmd === "list_audio_devices") return devices;
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [] };
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_documents_config") return { documentsFolder: null, documentDateFolders: true };
  });
  active = mount(CaptureSettings, { props: { vaultId: "v1" }, attachTo: document.body });
  return active;
}

describe("CaptureSettings shell", () => {
  it("renders Recording / Tasks / Documents tabs", async () => {
    const wrapper = mountShell();
    await flushPromises();
    for (const id of ["recording", "tasks", "documents"]) {
      expect(wrapper.find(`[data-testid="tab-${id}"]`).exists()).toBe(true);
    }
  });

  it("shows the Recording tab by default with its form loaded", async () => {
    const wrapper = mountShell();
    await flushPromises();
    expect(wrapper.get('[data-testid="panel-recording"]').isVisible()).toBe(true);
    expect(wrapper.get<HTMLInputElement>('[data-testid="meeting-folder-input"]').element.value).toBe("Meetings");
  });

  it("reveals the Documents tab content on click", async () => {
    const wrapper = mountShell();
    await flushPromises();
    await wrapper.get('[data-testid="tab-documents"]').trigger("click");
    expect(wrapper.get('[data-testid="panel-documents"]').isVisible()).toBe(true);
    expect(wrapper.find('[data-testid="documents-folder-input"]').exists()).toBe(true);
  });

  it("no longer renders a Save button", async () => {
    const wrapper = mountShell();
    await flushPromises();
    expect(wrapper.find('[data-testid="save-button"]').exists()).toBe(false);
  });
});
