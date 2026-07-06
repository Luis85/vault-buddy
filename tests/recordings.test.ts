import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Recordings from "../src/components/Recordings.vue";
import { useCaptureStore } from "../src/stores/capture";
import type { TranscriptionJob } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

// The live transcription-job map now lives in the capture store (backend-
// seeded, kept live by capture:* events the store itself listens for) —
// Recordings.vue no longer registers its own listeners, so seeding a job
// here is a direct store mutation, not a simulated Tauri event.
function job(overrides: Partial<TranscriptionJob> = {}): TranscriptionJob {
  return {
    mp3: "C:/v/Voice Notes/2026/07/b.mp3",
    vaultId: "v1",
    name: "Idea",
    phase: "transcribing",
    progress: 0.5,
    model: null,
    error: null,
    startedAtMs: Date.now(),
    ...overrides,
  };
}

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
    if (cmd === "cancel_transcription") return null;
  });
  const wrapper = mount(Recordings, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("Recordings", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
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
    // running) must stay re-transcribable: the button is gated only on the
    // store's LIVE job map, never on the persisted pending status, or such a
    // recording is stranded with no way to recover it. This mp3 is absent
    // from `capture.transcriptions` entirely (no live job), which is exactly
    // the crash-stuck scenario.
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
    // Nothing to cancel — no live job is running for this mp3.
    expect(wrapper.find('[data-testid="recording-cancel"]').exists()).toBe(false);
  });

  it("shows a spinner, disables re-transcribe, and offers cancel for a row with an active job", async () => {
    // Backend-seeded live state (mirrors what transcription_queue_status /
    // capture:transcribing would populate in the real app) — seeded on the
    // store BEFORE mount, since Recordings.vue derives from it directly
    // rather than learning about it via its own event listener.
    const store = useCaptureStore();
    store.transcriptions = { [sample[1].mp3]: job({ phase: "transcribing" }) };
    const { wrapper, calls } = await mountView();

    // sample[1] "Idea" is the second row in DOM order (Meeting, Voice Note,
    // Ungrouped sections) — see the "re-transcribes immediately" test above.
    const retranscribeButtons = wrapper.findAll('[data-testid="retranscribe"]');
    expect(retranscribeButtons[1].attributes("disabled")).toBeDefined();
    expect(retranscribeButtons[0].attributes("disabled")).toBeUndefined();

    const row = retranscribeButtons[1].element.parentElement;
    expect(row?.querySelector('[data-testid="recording-spinner"]')).toBeTruthy();

    const cancelButtons = wrapper.findAll('[data-testid="recording-cancel"]');
    expect(cancelButtons).toHaveLength(1);
    await cancelButtons[0].trigger("click");
    await flushPromises();
    expect(calls).toContainEqual({
      cmd: "cancel_transcription",
      args: { path: sample[1].mp3 },
    });
  });

  it("preserves the busy state across a full remount (new component instance)", async () => {
    // The regression this task fixes: the OLD component-local
    // `transcribingMp3` Set started empty on every fresh instance (this view
    // is destroyed/recreated on each view navigation — ActionPanel's
    // v-else-if keyed by recordingsVaultId), forgetting an in-flight job. The
    // store map must survive that destroy/recreate cycle instead.
    const store = useCaptureStore();
    store.transcriptions = { [sample[1].mp3]: job({ phase: "downloading" }) };
    await mountView();

    const remounted = mount(Recordings, { props: { vaultId: "v1" } });
    await flushPromises();

    const buttons = remounted.findAll('[data-testid="retranscribe"]');
    expect(buttons[1].attributes("disabled")).toBeDefined();
    expect(remounted.findAll('[data-testid="recording-cancel"]')).toHaveLength(1);
  });

  it("clears the busy state and reflects completion once the store's job phase moves to done", async () => {
    // Replaces the old "settles a row's status to complete when
    // capture:transcribed fires" test: that fired a simulated Tauri event at
    // a listener Recordings.vue owned itself. The component no longer
    // listens for capture:* events at all — the store does (via
    // capture.init() in the real app) — so this drives the same real-world
    // transition (transcribing -> done) through the store's own `upsert`
    // action and asserts the row derives its display from the result.
    const store = useCaptureStore();
    const rows = sample.map((r) => ({ ...r }));
    store.transcriptions = { [rows[1].mp3]: job({ phase: "transcribing" }) };
    const { wrapper } = await mountView({ list: rows });

    const row = () =>
      wrapper.findAll('[data-testid="recording-row"]')[1].element.parentElement;
    expect(row()?.querySelector('[data-testid="recording-spinner"]')).toBeTruthy();
    expect(wrapper.findAll('[data-testid="retranscribe"]')[1].attributes("disabled")).toBeDefined();

    store.upsert(rows[1].mp3, { phase: "done", progress: 1 });
    await flushPromises();

    expect(row()?.querySelector('[data-testid="recording-spinner"]')).toBeFalsy();
    expect(wrapper.findAll('[data-testid="retranscribe"]')[1].attributes("disabled")).toBeUndefined();
    expect(row()?.textContent).toContain("✓");
    expect(wrapper.find('[title="Transcribed ✓"]').exists()).toBe(true);
  });
});
