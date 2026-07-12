import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import RecordingSettings from "../src/components/RecordingSettings.vue";

const value = {
  recordingFolder: "Meetings",
  bitrateKbps: 128,
  createNote: true,
  followUpTemplate: true,
  inputDevice: "",
  outputDevice: "",
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
};
const devices = { inputs: [{ name: "USB Mic", isDefault: false }], outputs: [{ name: "Speakers", isDefault: true }] };

describe("RecordingSettings", () => {
  it("emits a merged update when the folder changes", async () => {
    const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: null } });
    await w.get('[data-testid="folder-input"]').setValue("Inbox/Audio");
    const emits = w.emitted("update:modelValue");
    expect(emits).toBeTruthy();
    // tsconfig's lib is ES2021 (no Array.prototype.at) — index from length
    // instead, same "last emitted call" intent as the brief's `.at(-1)`.
    const last = emits![emits!.length - 1][0] as typeof value;
    expect(last.recordingFolder).toBe("Inbox/Audio");
    // Untouched fields are preserved in the merge.
    expect(last.bitrateKbps).toBe(128);
  });

  it("shows the folder error", () => {
    const w = mount(RecordingSettings, { props: { modelValue: value, devices, folderError: "bad folder" } });
    expect(w.get('[data-testid="folder-error"]').text()).toContain("bad folder");
  });
});
