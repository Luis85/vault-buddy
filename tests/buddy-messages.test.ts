import { describe, expect, it } from "vitest";
import {
  vaultOpenedMessage,
  dailyNoteOpenedMessage,
  recordingStartedMessage,
  recordingPausedMessage,
  recordingResumedMessage,
  recordingSavedMessage,
  transcribingMessage,
  transcribedMessage,
  failureMessage,
} from "../src/buddyMessages";

describe("buddyMessages", () => {
  it("names the vault when opening one", () => {
    expect(vaultOpenedMessage("Personal")).toContain("Personal");
  });

  it("falls back to a generic line when the vault name is blank", () => {
    // no dangling "Opening  ✨" with a hole where the name should be
    expect(vaultOpenedMessage("")).not.toMatch(/Opening\s{2,}/);
    expect(vaultOpenedMessage("   ").trim().length).toBeGreaterThan(0);
  });

  it("has a distinct, non-empty line for each moment", () => {
    const lines = [
      vaultOpenedMessage("Personal"),
      dailyNoteOpenedMessage(),
      recordingStartedMessage(),
      recordingPausedMessage(),
      recordingResumedMessage(),
      recordingSavedMessage(),
      transcribingMessage(),
      transcribedMessage(),
      failureMessage(),
    ];
    for (const line of lines) expect(line.trim().length).toBeGreaterThan(0);
    // each moment reads differently, so the buddy never repeats itself across
    // two different events
    expect(new Set(lines).size).toBe(lines.length);
  });
});
