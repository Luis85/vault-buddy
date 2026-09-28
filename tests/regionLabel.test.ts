import { describe, expect, it } from "vitest";

import { regionDetail, regionTitle } from "../src/utils/regionLabel";

describe("region labels", () => {
  // Must read the same as the Rust side's own title for the same source
  // (`screen/src/source.rs`'s Region arm, `format!("Region on {label}")`).
  // The capture bar and the sidecar get the Rust one, the picker row gets
  // this one; two spellings of one source is a support problem.
  it("titles a region by the monitor it is on", () => {
    expect(regionTitle("Screen 1")).toBe("Region on Screen 1");
    expect(regionTitle("Dell U2720Q")).toBe("Region on Dell U2720Q");
  });

  // Spec 7.2 renders "1280 x 720 at (320, 180) on Screen 1"; the title
  // carries the "on <monitor>" half and this carries the geometry.
  it("details a region by its size and origin", () => {
    expect(regionDetail({ x: 320, y: 180, width: 1280, height: 720 })).toBe(
      "1280x720 at (320, 180)",
    );
    expect(regionDetail({ x: 0, y: 0, width: 640, height: 480 })).toBe(
      "640x480 at (0, 0)",
    );
  });
});
