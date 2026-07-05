import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Recordings from "../src/components/Recordings.vue";

const state = vi.hoisted(() => ({
  eventHandlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));

// Mirrors tests/capture-store.test.ts: listen() under mockIPC only records
// the plugin:event|listen invoke — nothing delivers a Rust-side emit back
// into the handler, so firing capture:* events requires replacing the
// module and invoking the registered handler directly.
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    state.eventHandlers[name] = handler;
    return Promise.resolve(() => {
      delete state.eventHandlers[name];
    });
  },
}));

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const sample = [
  { mp3: "C:/v/Meetings/2026/07/a.mp3", title: "Standup", recordedAt: "2026-07-04 14:05", duration: "1:05", type: "Meeting", transcriptStatus: "complete" },
  { mp3: "C:/v/Voice Notes/2026/07/b.mp3", title: "Idea", recordedAt: "2026-07-04 09:00", duration: "0:30", type: "Voice Note", transcriptStatus: "none" },
  { mp3: "C:/v/Meetings/2026/07/c.mp3", title: "Orphan", recordedAt: "2026-07-03 10:00", duration: null as string | null, type: null as string | null, transcriptStatus: "failed" },
];

const mountView = async (
  opts: { list?: unknown; onOpen?: (args: unknown) => unknown } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_recordings") return opts.list ?? sample;
    if (cmd === "open_recording") return opts.onOpen?.(args);
    if (cmd === "retranscribe") return null;
  });
  const wrapper = mount(Recordings, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("Recordings", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    state.eventHandlers = {};
  });
  afterEach(() => clearMocks());

  it("fetches list_recordings for the vault", async () => {
    const { calls } = await mountView();
    // onMounted registers the capture event listeners before fetching, so
    // list_recordings is no longer necessarily calls[0] — find it instead.
    expect(calls.find((c) => c.cmd === "list_recordings")).toEqual({
      cmd: "list_recordings",
      args: { id: "v1" },
    });
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
    await wrapper.findAll('[data-testid="recording-row"]')[0].trigger("click");
    await flushPromises();
    const open = calls.find((c) => c.cmd === "open_recording");
    expect(open?.args).toEqual({ path: sample[0].mp3 }); // first row = Meeting/Standup
    // Panel visibility is Rust-owned in the split-window architecture: a
    // successful open fires close_panel (not the old store.panelOpen flag).
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(true);
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
    const { wrapper, calls } = await mountView({
      onOpen: () => {
        throw new Error("launch boom");
      },
    });
    await wrapper.findAll('[data-testid="recording-row"]')[0].trigger("click");
    await flushPromises();
    // list stays visible, panel NOT closed, error surfaced
    expect(wrapper.findAll('[data-testid="recording-row"]').length).toBeGreaterThan(0);
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    expect(wrapper.text()).toContain("launch boom");
  });

  it("re-transcribes immediately for a non-complete transcript", async () => {
    const { wrapper, calls } = await mountView();
    // sample[1] "Idea" has transcriptStatus "none" — its row's retranscribe button
    // find the row for sample[1] by its retranscribe button and click it
    await wrapper.findAll('[data-testid="retranscribe"]')[1].trigger("click");
    await flushPromises();
    const rt = calls.find((c) => c.cmd === "retranscribe");
    expect(rt).toBeTruthy();
    // no confirm shown for a non-complete transcript
    expect(wrapper.find('[data-testid="retranscribe-confirm"]').exists()).toBe(false);
  });

  it("confirms before re-transcribing a complete transcript", async () => {
    const { wrapper, calls } = await mountView();
    // sample[0] "Standup" has transcriptStatus "complete"
    await wrapper.findAll('[data-testid="retranscribe"]')[0].trigger("click");
    // no invoke yet — a confirm is shown
    expect(calls.some((c) => c.cmd === "retranscribe")).toBe(false);
    await wrapper.get('[data-testid="retranscribe-confirm"]').trigger("click");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "retranscribe")).toBe(true);
  });

  it("keeps the re-transcribe button enabled for a stuck pending recording", async () => {
    // A sidecar stuck at `pending` (a crash left a placeholder, no job
    // running) must stay re-transcribable: the button is gated only on THIS
    // session's transient in-flight set, never on the persisted pending
    // status, or such a recording is stranded with no way to recover it.
    const { wrapper } = await mountView({
      list: [
        {
          mp3: "C:/v/Meetings/2026/07/p.mp3",
          title: "Stuck",
          recordedAt: "2026-07-04 12:00",
          duration: "0:10",
          type: "Meeting",
          transcriptStatus: "pending",
        },
      ],
    });
    const btn = wrapper.get('[data-testid="retranscribe"]');
    expect(btn.attributes("disabled")).toBeUndefined();
  });

  it("settles a row's status to complete when capture:transcribed fires", async () => {
    // mockIPC returns the mock's value as-is (no clone — see mocks.js), so
    // recordings.value would alias the shared `sample` array; a fresh copy
    // keeps this test's row mutation from leaking into later tests.
    const rows = sample.map((r) => ({ ...r }));
    const { wrapper } = await mountView({ list: rows });
    // grouped order is Meeting, Voice Note, Ungrouped — index 1 is "Idea"
    // (transcriptStatus "none"), same row targeted by the retranscribe test above.
    const row = () =>
      wrapper.findAll('[data-testid="recording-row"]')[1].element.parentElement;
    expect(row()?.textContent).not.toContain("✓");

    state.eventHandlers["capture:transcribed"]!({ payload: { mp3: rows[1].mp3 } });
    await flushPromises();

    expect(row()?.textContent).toContain("✓");
    expect(wrapper.find('[title="Transcribed ✓"]').exists()).toBe(true);
  });
});
