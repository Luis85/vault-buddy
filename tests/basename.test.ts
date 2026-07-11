import { describe, expect, it } from "vitest";

import { basename } from "../src/utils/basename";

describe("basename", () => {
  it("returns the last segment of a POSIX path", () => {
    expect(basename("Documents/2026/07/Report.md")).toBe("Report.md");
  });

  it("returns the last segment of a Windows backslash path", () => {
    // Rust hands back backslash paths even though the suite runs on Unix.
    expect(basename("C:\\Users\\me\\Report.docx")).toBe("Report.docx");
  });

  it("handles mixed separators", () => {
    expect(basename("C:\\vault/Documents\\2026/note.md")).toBe("note.md");
  });

  it("returns a bare filename unchanged", () => {
    expect(basename("note.md")).toBe("note.md");
  });

  it("returns an empty string for empty input (never undefined)", () => {
    expect(basename("")).toBe("");
  });
});
