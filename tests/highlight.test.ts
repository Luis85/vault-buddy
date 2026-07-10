import { describe, expect, it } from "vitest";

import { highlightParts } from "../src/utils/highlight";

describe("highlightParts", () => {
  it("marks case-insensitive occurrences", () => {
    expect(highlightParts("Alpha and ALPHA", "alpha")).toEqual([
      { text: "Alpha", match: true },
      { text: " and ", match: false },
      { text: "ALPHA", match: true },
    ]);
  });

  it("returns one unmatched part when nothing matches", () => {
    expect(highlightParts("nothing here", "alpha")).toEqual([
      { text: "nothing here", match: false },
    ]);
  });

  it("produces no empty parts for matches at the start and end", () => {
    expect(highlightParts("alpha mid alpha", "alpha")).toEqual([
      { text: "alpha", match: true },
      { text: " mid ", match: false },
      { text: "alpha", match: true },
    ]);
  });

  it("empty or whitespace query yields a single unmatched part", () => {
    expect(highlightParts("text", "")).toEqual([{ text: "text", match: false }]);
    expect(highlightParts("text", "  ")).toEqual([{ text: "text", match: false }]);
  });

  it("falls back to no highlight when lowercasing shifts lengths", () => {
    // 'İ'.toLowerCase() is two code units — index math against the lowered
    // string would mis-slice the original, so the helper must refuse to
    // highlight rather than corrupt the text.
    expect(highlightParts("İstanbul note", "i")).toEqual([
      { text: "İstanbul note", match: false },
    ]);
  });
});
