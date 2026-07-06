import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import RecordMode from "../src/components/RecordMode.vue";
import { useVaultsStore } from "../src/stores/vaults";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const mountView = async (mode: "meeting" | "voice-note" = "meeting") => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return { mode /* other fields unused here */ };
    if (cmd === "start_capture") return { recording: true, vaultId: "v1", startedAtMs: 1, paused: false, pausedTotalMs: 0, pausedSinceMs: null };
  });
  const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("RecordMode", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("highlights the vault's default mode", async () => {
    const { wrapper } = await mountView("voice-note");
    expect(wrapper.get('[data-testid="mode-voice-note"]').classes()).toContain("border-violet-400");
    expect(wrapper.get('[data-testid="mode-meeting"]').classes()).not.toContain("border-violet-400");
  });

  it("starts a recording and returns to the list", async () => {
    const { wrapper, calls } = await mountView("meeting");
    const store = useVaultsStore();
    store.openRecordMode("v1");
    await wrapper.get('[data-testid="mode-voice-note"]').trigger("click");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "start_capture")).toBe(true);
    expect(store.view).toBe("list");
  });

  it("navigates to recordings on Browse", async () => {
    const { wrapper } = await mountView("meeting");
    const store = useVaultsStore();
    await wrapper.get('[data-testid="mode-browse"]').trigger("click");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("v1");
  });

  it("falls back to meeting when the config read fails", async () => {
    clearMocks();
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") throw new Error("nope");
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(wrapper.get('[data-testid="mode-meeting"]').classes()).toContain("border-violet-400");
  });

  it("saves a changed transcription setting to the vault config, preserving the rest", async () => {
    const cfg = {
      mode: "meeting",
      recordingFolder: "Meetings",
      bitrateKbps: 160,
      createNote: true,
      followUpTemplate: false,
      inputDevice: "Headset Mic",
      outputDevice: "Speakers",
      transcribe: false,
      transcriptionModel: "small",
      transcriptionLanguage: null,
      transcriptTimestamps: true,
    };
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_capture_config") return cfg;
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await flushPromises();

    const saveCall = calls.find((c) => c.cmd === "set_capture_config");
    expect(saveCall?.args).toEqual({
      id: "v1",
      cfg: { ...cfg, transcribe: true },
    });
  });
});
