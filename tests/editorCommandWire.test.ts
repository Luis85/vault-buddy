import { describe, expect, it } from "vitest";

import { EDITOR_COMMAND_WIRE } from "./fixtures/editorCommandWire";

/**
 * Final whole-branch review I7. The fixture's own `satisfies
 * EditorCommand[]` is the TypeScript half of the pin — `vue-tsc` (the build
 * gate, which checks `tests/`) refuses an entry the union does not accept.
 * The Rust half (`core::editor::commands::wire_tests`) deserializes every
 * entry, requires the same JSON back and each variant exactly once. This
 * file keeps the table honest from this side: one entry per kind, 46 of
 * them, the count the Rust half asserts.
 */
describe("the shared EditorCommand wire table", () => {
  it("lists 46 distinct kinds, the count Rust's exhaustive match asserts", () => {
    const kinds = EDITOR_COMMAND_WIRE.map((c) => c.kind);
    expect(kinds).toHaveLength(46);
    expect(new Set(kinds).size).toBe(46);
  });
});
