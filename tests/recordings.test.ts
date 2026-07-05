import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Recordings from "../src/components/Recordings.vue";
import { useVaultsStore } from "../src/stores/vaults";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const sample = [
  { mp3: "C:/v/Meetings/2026/07/a.mp3", title: "Standup", recordedAt: "2026-07-04 14:05", duration: "1:05", type: "Meeting" },
  { mp3: "C:/v/Voice Notes/2026/07/b.mp3", title: "Idea", recordedAt: "2026-07-04 09:00", duration: "0:30", type: "Voice Note" },
  { mp3: "C:/v/Meetings/2026/07/c.mp3", title: "Orphan", recordedAt: "2026-07-03 10:00", duration: null as string | null, type: null as string | null },
];

const mountView = async (
  opts: { list?: unknown; onOpen?: (args: unknown) => unknown } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_recordings") return opts.list ?? sample;
    if (cmd === "open_recording") return opts.onOpen?.(args);
  });
  const wrapper = mount(Recordings, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("Recordings", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("fetches list_recordings for the vault", async () => {
    const { calls } = await mountView();
    expect(calls[0]).toEqual({ cmd: "list_recordings", args: { id: "v1" } });
  });

  it("groups by type with Ungrouped last", async () => {
    const { wrapper } = await mountView();
    const headers = wrapper.findAll("h2").map((h) => h.text());
    expect(headers[0]).toContain("Meeting");
    expect(headers[1]).toContain("Voice Note");
    expect(headers[headers.length - 1]).toContain("Ungrouped");
    expect(wrapper.text()).toContain("Standup");
    expect(wrapper.text()).toContain("—"); // null duration renders a dash
  });

  it("toggles to a flat list, hiding the type headers", async () => {
    const { wrapper } = await mountView();
    await wrapper.get('[data-testid="group-toggle"]').trigger("click");
    expect(wrapper.findAll("h2")).toHaveLength(0);
    expect(wrapper.findAll('[data-testid="recording-row"]')).toHaveLength(3);
  });

  it("shows an empty state when there are no recordings", async () => {
    const { wrapper } = await mountView({ list: [] });
    expect(wrapper.text()).toContain("No recordings yet.");
  });

  it("opens a recording and closes the panel", async () => {
    const { wrapper, calls } = await mountView();
    const store = useVaultsStore();
    store.panelOpen = true;
    await wrapper.findAll('[data-testid="recording-row"]')[0].trigger("click");
    await flushPromises();
    const open = calls.find((c) => c.cmd === "open_recording");
    expect(open?.args).toEqual({ path: sample[0].mp3 }); // first row = Meeting/Standup
    expect(store.panelOpen).toBe(false);
  });

  it("surfaces a load error", async () => {
    // override with a throwing mock
    clearMocks();
    mockIPC((cmd) => {
      if (cmd === "list_recordings") throw new Error("scan boom");
    });
    const w = mount(Recordings, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(w.text()).toContain("scan boom");
  });

  it("keeps the list visible and shows an error when opening fails", async () => {
    const { wrapper } = await mountView({
      onOpen: () => {
        throw new Error("launch boom");
      },
    });
    const store = useVaultsStore();
    store.panelOpen = true;
    await wrapper.findAll('[data-testid="recording-row"]')[0].trigger("click");
    await flushPromises();
    // list stays visible, panel NOT closed, error surfaced
    expect(wrapper.findAll('[data-testid="recording-row"]').length).toBeGreaterThan(0);
    expect(store.panelOpen).toBe(true);
    expect(wrapper.text()).toContain("launch boom");
  });
});
