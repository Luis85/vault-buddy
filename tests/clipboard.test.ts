import { afterEach, describe, expect, it, vi } from "vitest";

import { logWarning } from "../src/logging";
import { copyToClipboard } from "../src/utils/clipboard";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

// The shared copy helper behind McpSettings' snippets and the task editor's
// copy-id. Copy buttons have no failure UI by design, so the contract is:
// write on success, warn (never throw) on failure, and degrade silently when
// the Clipboard API is absent entirely.
describe("copyToClipboard", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("writes the text through the Clipboard API", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    copyToClipboard("secret token", "test ctx");
    await Promise.resolve();
    expect(writeText).toHaveBeenCalledWith("secret token");
    expect(logWarning).not.toHaveBeenCalled();
  });

  it("logs (never throws) when the write rejects", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    copyToClipboard("x", "test ctx");
    await Promise.resolve();
    await Promise.resolve(); // let the .catch settle
    expect(logWarning).toHaveBeenCalledWith(expect.stringContaining("test ctx: clipboard copy failed"));
  });

  it("degrades silently when the Clipboard API is unavailable", () => {
    // A hardened/older webview may expose no navigator.clipboard at all —
    // the optional chain must short-circuit instead of throwing a TypeError
    // out of a click handler.
    vi.stubGlobal("navigator", {});
    expect(() => copyToClipboard("x", "test ctx")).not.toThrow();
  });
});
