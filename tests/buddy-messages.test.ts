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
  updateAvailableMessage,
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

  describe("updateAvailableMessage", () => {
    it("names the version", () => {
      expect(updateAvailableMessage("0.6.0")).toContain("0.6.0");
    });

    it("falls back to a generic line when the version is blank", () => {
      // no dangling "Update v is ready" with a hole where the version goes
      const msg = updateAvailableMessage("");
      expect(msg).not.toMatch(/v\s/);
      expect(msg.toLowerCase()).toContain("update");
    });
  });

  describe("failureMessage", () => {
    it("falls back to the generic line when no reason is given", () => {
      expect(failureMessage()).toBe("Hmm, that didn't work 😕");
    });

    it("speaks the reason instead of the generic line when one is given", () => {
      const msg = failureMessage("model missing");
      expect(msg).toContain("model missing");
      expect(msg).not.toContain("didn't work");
    });

    it("truncates a long reason", () => {
      const reason = "a".repeat(80);
      const msg = failureMessage(reason);
      expect(msg).toContain("…");
      expect(msg).not.toContain("a".repeat(80));
      // truncated to 60 chars of reason, plus the surrounding copy/emoji
      expect(msg.length).toBeLessThan(reason.length);
    });
  });
});
