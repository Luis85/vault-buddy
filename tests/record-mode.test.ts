import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import RecordMode from "../src/components/RecordMode.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import { useVaultsStore } from "../src/stores/vaults";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { logWarning } from "../src/logging";

const recordingRow = (mp3: string) => ({
  mp3,
  title: mp3,
  recordedAt: "2026-07-09 10:00",
  duration: null,
  type: "Meeting",
  transcriptStatus: "none",
});

const mountView = async (
  options: { mode?: "meeting" | "voice-note"; recordings?: unknown[] } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config")
      return { mode: options.mode ?? "meeting" /* other fields unused here */ };
    if (cmd === "list_recordings") return options.recordings ?? [];
    if (cmd === "start_capture") return { recording: true, vaultId: "v1", startedAtMs: 1, paused: false, pausedTotalMs: 0, pausedSinceMs: null };
  });
  const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("RecordMode", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("no longer pre-highlights the vault's stored default mode", async () => {
    // The "default recording mode" setting is gone: the mode is a per-recording
    // choice, so neither card gets the selected treatment any more.
    const { wrapper } = await mountView({ mode: "voice-note" });
    expect(wrapper.get('[data-testid="mode-voice-note"]').classes()).not.toContain("border-violet-400");
    expect(wrapper.get('[data-testid="mode-meeting"]').classes()).not.toContain("border-violet-400");
  });

  it("renders the recording actions first and the transcription settings last", async () => {
    const { wrapper } = await mountView();
    const html = wrapper.html();
    const meeting = html.indexOf('data-testid="mode-meeting"');
    const voiceNote = html.indexOf('data-testid="mode-voice-note"');
    const browse = html.indexOf('data-testid="mode-browse"');
    const transcribe = html.indexOf('data-testid="transcribe-toggle"');
    expect(meeting).toBeGreaterThan(-1);
    expect(voiceNote).toBeGreaterThan(meeting);
    expect(browse).toBeGreaterThan(voiceNote);
    expect(transcribe).toBeGreaterThan(browse);
  });

  it("orders Import Document before Browse recordings (Browse is the last action)", async () => {
    // Since import joined the chooser, Browse recordings belongs at the bottom
    // — the two capture actions (record + import) come first.
    const { wrapper } = await mountView();
    const html = wrapper.html();
    const importDoc = html.indexOf('data-testid="import-document"');
    const browse = html.indexOf('data-testid="mode-browse"');
    expect(importDoc).toBeGreaterThan(-1);
    expect(browse).toBeGreaterThan(importDoc);
  });

  it("styles Browse recordings as a card like the recording options", async () => {
    const { wrapper } = await mountView();
    const browse = wrapper.get('[data-testid="mode-browse"]');
    for (const cls of ["rounded-lg", "border-white/10", "bg-white/5", "px-3", "py-2"]) {
      expect(browse.classes()).toContain(cls);
    }
  });

  it("shows the vault's recording count on the Browse card", async () => {
    const { wrapper } = await mountView({
      recordings: [recordingRow("a.mp3"), recordingRow("b.mp3"), recordingRow("c.mp3")],
    });
    expect(wrapper.get('[data-testid="recording-count"]').text()).toBe("3");
    expect(
      wrapper.get('[data-testid="mode-browse"]').attributes("aria-label"),
    ).toContain("3");
  });

  it("shows a zero recording count (an empty vault is worth knowing before clicking)", async () => {
    const { wrapper } = await mountView({ recordings: [] });
    expect(wrapper.get('[data-testid="recording-count"]').text()).toBe("0");
  });

  it("hides the count and warns when list_recordings fails", async () => {
    vi.mocked(logWarning).mockClear();
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") return { mode: "meeting" };
      if (cmd === "list_recordings") throw new Error("scan failed");
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(wrapper.find('[data-testid="recording-count"]').exists()).toBe(false);
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("list_recordings"),
    );
  });

  it("starts a recording and returns to the list", async () => {
    const { wrapper, calls } = await mountView();
    const store = useVaultsStore();
    store.openRecordMode("v1");
    await wrapper.get('[data-testid="mode-voice-note"]').trigger("click");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "start_capture")).toBe(true);
    expect(store.view).toBe("list");
  });

  it("navigates to recordings on Browse", async () => {
    const { wrapper } = await mountView();
    const store = useVaultsStore();
    await wrapper.get('[data-testid="mode-browse"]').trigger("click");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("v1");
  });

  it("still renders every action when the config read fails", async () => {
    clearMocks();
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") throw new Error("nope");
      if (cmd === "list_recordings") return [];
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    // A config failure must never block recording — all three cards and the
    // transcription controls stay usable against the defaults.
    expect(wrapper.find('[data-testid="mode-meeting"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="mode-voice-note"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="mode-browse"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="transcribe-toggle"]').exists()).toBe(true);
  });

  it("saves a changed transcription setting to the vault config, preserving the rest", async () => {
    const cfg = {
      mode: "meeting",
      meetingFolder: "Meetings",
      voiceNoteFolder: "Voice Notes",
      bitrateKbps: 160,
      createNote: true,
      followUpTemplate: false,
      inputDevice: "Headset Mic",
      outputDevice: "Speakers",
      transcribe: false,
      transcriptionModel: "small",
      transcriptionLanguage: null,
      transcriptTimestamps: true,
      transcriptionVocabulary: null,
      transcriptionVad: true,
    };
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_capture_config") return cfg;
      if (cmd === "list_recordings") return [];
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

  it("notifies when saving transcription settings fails", async () => {
    // Regression: persist()'s catch used to only logWarning — a failed save
    // had no user-visible signal at all in this view (RecordMode has no
    // save button/banner of its own, unlike CaptureSettings).
    const cfg = {
      mode: "meeting",
      meetingFolder: "Meetings",
      voiceNoteFolder: "Voice Notes",
      bitrateKbps: 160,
      createNote: true,
      followUpTemplate: false,
      inputDevice: "Headset Mic",
      outputDevice: "Speakers",
      transcribe: false,
      transcriptionModel: "small",
      transcriptionLanguage: null,
      transcriptTimestamps: true,
      transcriptionVocabulary: null,
      transcriptionVad: true,
    };
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") return cfg;
      if (cmd === "list_recordings") return [];
      if (cmd === "set_capture_config") throw new Error("disk full");
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    const notes = useNotificationsStore();

    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await flushPromises();

    expect(
      notes.items.some(
        (i) =>
          i.kind === "error" &&
          i.message.includes("Couldn't save transcription settings") &&
          i.message.includes("disk full"),
      ),
    ).toBe(true);
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("transcription settings save failed"),
    );
  });

  it("does not persist a transcription toggle made before the config read resolves", async () => {
    // Regression guard: RecordMode seeds `config` with hardcoded defaults and
    // renders TranscriptionSettings immediately (recording must never block
    // on the config read), but the vault's REAL config only lands once
    // get_capture_config resolves in onMounted. Toggling a transcription
    // field before that read resolves used to persist() the default-seeded
    // config to disk — silently clobbering the vault's real
    // meetingFolder/voiceNoteFolder/bitrateKbps/devices/createNote/followUpTemplate
    // — and the in-flight read would then overwrite config.value with the
    // pre-persist config anyway, discarding the toggle too. persist() must
    // stay gated until the real config has loaded (or the load has failed).
    const cfg = {
      mode: "voice-note",
      meetingFolder: "Meetings",
      voiceNoteFolder: "Voice Notes",
      bitrateKbps: 160,
      createNote: true,
      followUpTemplate: false,
      inputDevice: "Headset Mic",
      outputDevice: "Speakers",
      transcribe: false,
      transcriptionModel: "small",
      transcriptionLanguage: null,
      transcriptTimestamps: true,
      transcriptionVocabulary: null,
      transcriptionVad: true,
    };
    let resolveConfig!: (v: unknown) => void;
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_capture_config") {
        return new Promise((resolve) => {
          resolveConfig = resolve;
        });
      }
      if (cmd === "list_recordings") return [];
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    // Toggle while the config read is still in flight (unresolved).
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);

    // Now let the real config land.
    resolveConfig(cfg);
    await flushPromises();

    // The resolved read must never itself trigger a save either.
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);
    // The real config replaced the default-seeded state — the pre-resolve
    // toggle is superseded by the resolved cfg (transcribe: false).
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="transcribe-toggle"]').element.checked,
    ).toBe(false);

    // Once loaded, a toggle persists normally against the real config.
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await flushPromises();
    const saveCall = calls.find((c) => c.cmd === "set_capture_config");
    expect(saveCall?.args).toEqual({
      id: "v1",
      cfg: { ...cfg, transcribe: true },
    });
  });

  it("does not persist after a failed config load (GAP-30)", async () => {
    // loadConfig's finally set loaded=true even on failure, so one
    // transcription toggle persisted the default-seeded config — wiping the
    // vault's real meetingFolder/voiceNoteFolder/bitrate/devices on disk.
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "get_capture_config") throw new Error("read failed");
      if (cmd === "list_recordings") return [];
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    // Flip the transcription toggle after the failed config read.
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    await flushPromises();

    // Should not have persisted the default-seeded config.
    expect(calls).not.toContain("set_capture_config");
  });

  describe("vocabulary autosave debounce", () => {
    // Regression: the transcription computed's setter used to call persist()
    // synchronously on EVERY update:modelValue. The vocabulary textarea emits
    // on every keystroke, so typing a vocabulary list fired a
    // set_capture_config (a main-thread Rust command) per character. Fake
    // timers drive useAutosave's debounce deterministically — mirrors how
    // recording-config-tab.test.ts exercises the same composable.
    const vocabCfg = {
      mode: "meeting",
      meetingFolder: "Meetings",
      voiceNoteFolder: "Voice Notes",
      bitrateKbps: 160,
      createNote: true,
      followUpTemplate: false,
      inputDevice: "Headset Mic",
      outputDevice: "Speakers",
      transcribe: true,
      transcriptionModel: "small",
      transcriptionLanguage: null,
      transcriptTimestamps: true,
      transcriptionVocabulary: null,
      transcriptionVad: true,
    };

    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("collapses successive vocabulary edits into a single trailing save, trimmed", async () => {
      const calls: Array<{ cmd: string; args: unknown }> = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args });
        if (cmd === "get_capture_config") return vocabCfg;
        if (cmd === "list_recordings") return [];
      });
      const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
      await flushPromises();

      const textarea = wrapper.get('[data-testid="transcription-vocabulary-input"]');
      await textarea.setValue("A");
      await textarea.setValue("Anna K");
      await textarea.setValue("  Anna Kowalska, Kubernetes  ");
      await flushPromises();

      // Still within the debounce window — none of the three edits saved yet.
      expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);

      vi.advanceTimersByTime(600);
      await flushPromises();

      const saveCalls = calls.filter((c) => c.cmd === "set_capture_config");
      expect(saveCalls).toHaveLength(1);
      expect(saveCalls[0].args).toEqual({
        id: "v1",
        cfg: { ...vocabCfg, transcriptionVocabulary: "Anna Kowalska, Kubernetes" },
      });
    });

    it("does not eat a trailing space typed into the textarea between keystrokes (Fix 2)", async () => {
      // Regression: the transcription computed's SETTER used to store
      // v.transcriptionVocabulary.trim() into config.value, which feeds
      // straight back into the textarea's v-model — Vue then resets the
      // DOM to the trimmed value on the next render, deleting a trailing
      // space the user just typed (e.g. "Anna " -> "Anna" mid-keystroke,
      // so continuing to type "Kowalska" produced "AnnaKowalska"). The RAW
      // (untrimmed) string must survive in the bundle/textarea state
      // between keystrokes; only the persisted payload trims, at save time.
      const calls: Array<{ cmd: string; args: unknown }> = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args });
        if (cmd === "get_capture_config") return vocabCfg;
        if (cmd === "list_recordings") return [];
      });
      const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
      await flushPromises();

      const textarea = wrapper.get<HTMLTextAreaElement>(
        '[data-testid="transcription-vocabulary-input"]',
      );
      await textarea.setValue("Anna");
      await textarea.setValue("Anna ");

      // The trailing space must survive in the DOM: Vue must not have
      // reset the textarea back to the trimmed "Anna" mid-edit.
      expect(textarea.element.value).toBe("Anna ");

      vi.advanceTimersByTime(600);
      await flushPromises();

      const saveCalls = calls.filter((c) => c.cmd === "set_capture_config");
      expect(saveCalls).toHaveLength(1);
      expect(saveCalls[0].args).toEqual({
        id: "v1",
        cfg: { ...vocabCfg, transcriptionVocabulary: "Anna" },
      });
    });

    it("saves null vocabulary (not an empty string) when the final edit is blank", async () => {
      const seeded = { ...vocabCfg, transcriptionVocabulary: "Old notes" };
      const calls: Array<{ cmd: string; args: unknown }> = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args });
        if (cmd === "get_capture_config") return seeded;
        if (cmd === "list_recordings") return [];
      });
      const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
      await flushPromises();

      await wrapper.get('[data-testid="transcription-vocabulary-input"]').setValue("   ");
      await flushPromises();
      expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);

      vi.advanceTimersByTime(600);
      await flushPromises();

      const saveCalls = calls.filter((c) => c.cmd === "set_capture_config");
      expect(saveCalls).toHaveLength(1);
      expect(saveCalls[0].args).toEqual({
        id: "v1",
        cfg: { ...seeded, transcriptionVocabulary: null },
      });
    });

    it("still saves a toggle change (transcriptionVad) immediately, not debounced", async () => {
      const calls: Array<{ cmd: string; args: unknown }> = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args });
        if (cmd === "get_capture_config") return vocabCfg;
        if (cmd === "list_recordings") return [];
      });
      const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
      await flushPromises();

      await wrapper.get('[data-testid="transcription-vad-toggle"]').setValue(false);
      await flushPromises();

      // No timer advance: a debounced save would NOT have landed yet, so this
      // also proves the toggle bypassed the debounce entirely.
      const saveCalls = calls.filter((c) => c.cmd === "set_capture_config");
      expect(saveCalls).toHaveLength(1);
      expect(saveCalls[0].args).toEqual({
        id: "v1",
        cfg: { ...vocabCfg, transcriptionVad: false },
      });
    });
  });
});
