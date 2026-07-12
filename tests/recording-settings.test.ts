import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import RecordingSettings from "../src/components/RecordingSettings.vue";

const value = {
  meetingFolder: "Meetings",
  voiceNoteFolder: "Voice Notes",
  bitrateKbps: 128,
  createNote: true,
  followUpTemplate: true,
  inputDevice: "",
  outputDevice: "",
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
  recordingDateFolders: true,
};
const devices = { inputs: [{ name: "USB Mic", isDefault: false }], outputs: [{ name: "Speakers", isDefault: true }] };

describe("RecordingSettings", () => {
  it("emits per-mode folder updates", async () => {
    const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: null } });
    await w.get('[data-testid="meeting-folder-input"]').setValue("Mtgs");
    await w.get('[data-testid="voice-note-folder-input"]').setValue("Notes");
    const emits = w.emitted("update:modelValue");
    expect(emits).toBeTruthy();
    // tsconfig's lib is ES2021 (no Array.prototype.at) — index from length
    // instead, same "last emitted call" intent as the brief's `.at(-1)`.
    const last = emits![emits!.length - 1][0] as typeof value;
    expect(last.voiceNoteFolder).toBe("Notes");
    // Untouched fields are preserved in the merge.
    expect(last.bitrateKbps).toBe(128);
    // The FIRST emit (from the meeting-folder edit) independently proves that
    // input patches meetingFolder — it's a separate emitted call because this
    // controlled component always spread-merges onto the static modelValue
    // PROP (never a locally accumulated copy), so the second edit's emit
    // doesn't carry the first edit's change forward without a parent
    // re-feeding the merged value back in (which CaptureSettings.vue does,
    // via recordingBundle, but this isolated mount does not).
    const first = emits![0][0] as typeof value;
    expect(first.meetingFolder).toBe("Mtgs");
  });

  it("shows the folder error", () => {
    const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: "bad folder" } });
    expect(w.get('[data-testid="folder-error"]').text()).toContain("bad folder");
  });

  it("emits the dated-folders toggle", async () => {
    const v = { ...value, recordingDateFolders: true };
    const w = mount(RecordingSettings, { props: { modelValue: v, devices, folderError: null } });
    await w.get('[data-testid="recording-date-folders-toggle"]').setValue(false);
    const emits = w.emitted("update:modelValue");
    const last = emits![emits!.length - 1][0] as typeof v;
    expect(last.recordingDateFolders).toBe(false);
  });
});
