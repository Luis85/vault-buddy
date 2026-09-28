/**
 * The Rust-source reader the wire-enum pins stand on (Task 20; hardening
 * Task 21, carried): it models serde's `rename_all = "camelCase"` spelling
 * and nothing else, so a per-variant `#[serde(rename = ...)]` would be
 * silently mis-modelled. It must fail loudly instead.
 */
import { describe, expect, it } from "vitest";

import { rustVariantsIn } from "./helpers/rustSource";

describe("rustVariantsIn", () => {
  it("reads the camelCase wire spelling, skipping comments and attributes", () => {
    const source = [
      "#[serde(rename_all = \"camelCase\")]",
      "pub enum Phase {",
      "    /// Waiting.",
      "    #[default]",
      "    Queued,",
      "    // Running now.",
      "    InProgress,",
      "}",
    ].join("\n");
    expect(rustVariantsIn(source, "Phase", "fixture.rs")).toEqual(["inProgress", "queued"]);
  });

  it("refuses an enum with a per-variant serde rename rather than mis-model it", () => {
    const source = [
      "#[serde(rename_all = \"camelCase\")]",
      "pub enum Phase {",
      "    Queued,",
      "    #[serde(rename = \"running\")]",
      "    InProgress,",
      "}",
    ].join("\n");
    expect(() => rustVariantsIn(source, "Phase", "fixture.rs")).toThrow(/serde\(rename/);
  });

  it("names the enum and file when the enum is missing", () => {
    expect(() => rustVariantsIn("pub enum Other {}", "Phase", "fixture.rs")).toThrow(
      "enum Phase not found in fixture.rs",
    );
  });
});
