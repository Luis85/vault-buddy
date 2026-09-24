import { describe, expect, it } from "vitest";

import { webcamsFrom } from "../src/utils/captureWebcams";

// `list_capture_webcams`' reply, decoded defensively: the literal wire shape
// Rust pins (`{"id":"webcam:…","label":"…"}`), and anything else dropped
// rather than rendered as an option Start would send back as "undefined".
describe("webcamsFrom", () => {
  it("keeps well-formed rows in order", () => {
    const wire = JSON.parse(
      '[{"id":"webcam:0f3a","label":"Integrated Camera"},{"id":"webcam:77","label":"BRIO"}]',
    );
    expect(webcamsFrom(wire)).toEqual([
      { id: "webcam:0f3a", label: "Integrated Camera" },
      { id: "webcam:77", label: "BRIO" },
    ]);
  });

  it("drops malformed rows and non-lists", () => {
    expect(webcamsFrom(undefined)).toEqual([]);
    expect(webcamsFrom({ id: "webcam:1", label: "x" })).toEqual([]);
    expect(
      webcamsFrom([
        { id: "webcam:1" },
        { id: 7, label: "x" },
        { id: "screen:0", label: "not a webcam" },
        null,
        { id: "webcam:2", label: "Kept" },
      ]),
    ).toEqual([{ id: "webcam:2", label: "Kept" }]);
  });
});
