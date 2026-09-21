import { describe, expect, it } from "vitest";

import {
  clipIsActive,
  clipOutputDuration,
  clipOutputEnd,
  cueOutputSpan,
  outputAt,
  roundHalfAway,
  sourceAt,
} from "../src/editor/timeMap";
import type { ClipSpan } from "../src/editorTypes";
import rawTable from "./fixtures/editor-time-cases.json";

/**
 * The TypeScript half of the SHARED editor-time fixture table.
 *
 * `core::editor::time` (Rust — what every command execution and the
 * render plan use) and `src/editor/timeMap.ts` (what the preview, drag
 * preview and ruler read from while the user is just looking) implement
 * the same time-mapping algebra twice, in two languages. Each has its own
 * tests; this file and `core/src/editor/time.rs` both read the exact same
 * `tests/fixtures/editor-time-cases.json` by the same relative path, so a
 * disagreement between the two has to redden one of the two suites rather
 * than staying invisible with every test in the repo green — the
 * `timeline-cases.json` / GAP-136 precedent.
 */

interface Case {
  name: string;
  clip: ClipSpan;
  durationMs: number;
  sourceAt: [number, number | null][];
  outputAt: [number, number | null][];
  cueSpans: [number, number, [number, number] | null][];
}

// Imported rather than read at runtime, so a moved or malformed file fails
// the typecheck and the build, not just this run.
const table = rawTable as unknown as { version: number; cases: Case[] };

describe("the shared editor-time fixture table", () => {
  // A table nothing iterates proves nothing, and one that silently shrinks
  // proves almost nothing. The Rust half asserts the same count against the
  // same file, so a case deleted on one side reddens both.
  it("the fixture table has 10 cases", () => {
    expect(table.cases).toHaveLength(10);
  });

  it.each(table.cases.map((c) => [c.name, c] as const))("maps %s identically to Rust", (_name, c) => {
    expect(clipOutputDuration(c.clip.in_ms, c.clip.out_ms, c.clip.speed)).toBe(c.durationMs);

    for (const [t, expected] of c.sourceAt) {
      expect({ t, source: sourceAt(c.clip, t) }).toEqual({ t, source: expected });
    }

    for (const [source, expected] of c.outputAt) {
      expect({ source, output: outputAt(c.clip, source) }).toEqual({ source, output: expected });
    }

    for (const [a, b, expected] of c.cueSpans) {
      expect({ a, b, span: cueOutputSpan(c.clip, a, b) }).toEqual({ a, b, span: expected });
    }
  });

  // `clipOutputEnd`/`clipIsActive` aren't in the mapping loop above (the
  // fixture only lists `t`/source rows), so this asserts them directly
  // against the same case the Rust twin's `end_is_exclusive` uses: the
  // end instant itself must fall outside the clip's active span.
  it("clipIsActive is false, and clipOutputEnd is not reached, at the end instant", () => {
    const clip = table.cases[0].clip;
    const end = clipOutputEnd(clip);
    expect(clipIsActive(clip, end - 1)).toBe(true);
    expect(clipIsActive(clip, end)).toBe(false);
  });

  // DELIBERATELY NOT a fixture-driven assertion (unlike the mapping test
  // above): this is the mutation check's own probe, direct against
  // `roundHalfAway` at exactly the boundary a banker's-rounding
  // implementation would answer differently (1.5 -> 2 either way, but a
  // round-half-to-even helper answers 2.5 -> 2, not 3).
  it("rounds half away from zero, not banker's", () => {
    expect(roundHalfAway(1.5)).toBe(2);
    expect(roundHalfAway(2.5)).toBe(3);
  });
});
