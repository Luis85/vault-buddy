import { describe, expect, it } from "vitest";

import { formatDuration } from "../src/utils/formatDuration";

// Pins RecordingBar's original inline `elapsed` formatter exactly (Task
// 9/C2) — every case here must match what that computed used to return
// byte-for-byte, since Transcriptions.vue's own inline copy (now also
// replaced by this shared util) mirrored the same logic.
describe("formatDuration", () => {
  it("formats zero as 0:00", () => {
    expect(formatDuration(0)).toBe("0:00");
  });

  it("pads seconds under a minute", () => {
    expect(formatDuration(5_000)).toBe("0:05");
  });

  it("formats minutes and seconds", () => {
    expect(formatDuration(65_000)).toBe("1:05");
  });

  it("rolls over to h:mm:ss once the duration reaches an hour", () => {
    // 3,661,000ms = 3,661s = 1h 1m 1s. The source's `h > 0` branch — copied
    // here verbatim — switches to `h:mm:ss` at this point rather than
    // continuing to count minutes past 59 (that would read "61:01").
    expect(formatDuration(3_661_000)).toBe("1:01:01");
  });

  it("clamps a negative duration to 0:00", () => {
    expect(formatDuration(-5_000)).toBe("0:00");
  });
});
