import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";

import { useEditorTimeline } from "../src/composables/useEditorTimeline";
import type { SegmentDto, TimelineDto } from "../src/types";
import { outputDurationMs, toSourceMs, wholeTimeline } from "../src/utils/timelineGeometry";
import rawTable from "./fixtures/timeline-cases.json";

/**
 * The TypeScript half of the SHARED fixture table.
 *
 * The editor's segment algebra exists TWICE, in two languages:
 * `core::timeline` (Rust — what phase 5's export will plan on) and
 * `timelineGeometry.ts` + `useEditorTimeline.ts` (what the user actually
 * watches while deciding the edit is right). Each had its own unit tests and
 * no fixture in common, so a disagreement between them was invisible: the
 * exported file would not match the preview the user approved, with every
 * test in the repo green (docs/Gaps.md GAP-136).
 *
 * `src-tauri/core/src/timeline.rs` reads the same JSON file by the same path
 * and asserts the same expectations, so a divergence now has to redden one
 * of the two suites. The expectations live in the FILE rather than in either
 * test, so neither language can quietly become the definition of "correct".
 *
 * Two rows are there because the two implementations really did disagree
 * when the table was first run through both:
 *
 * - **a backwards segment** (`[0,2000] [4000,3000] [4000,6000]`). Rust's
 *   `Segment::duration_ms` is a `saturating_sub`, so the middle segment
 *   contributes nothing; TypeScript subtracted raw, so it contributed a
 *   NEGATIVE length that walked the accumulator backwards — duration 3000
 *   against Rust's 4000, and `toSourceMs(2000)` 5000 against Rust's 4000.
 * - **`whole(0)`**. Rust returns an EMPTY timeline and carries a named
 *   regression test against minting a zero-length segment; the TypeScript
 *   re-implementation seeded `[{0, 0}]` with no such guard.
 *
 * Neither is reachable from any operation either side offers — a hand-edited
 * or sync-conflicted sidecar produces the first, an empty capture the second
 * — but the sidecar is a file this code repeatedly and correctly calls
 * untrusted, and nothing validates the timeline field (GAP-134).
 */interface Case {
  name: string;
  segments: SegmentDto[];
  outputDurationMs: number;
  toSourceMs: [number, number | null][];
  splitAt?: { outputMs: number; segments: SegmentDto[] };
  delete?: { index: number; segments: SegmentDto[] };
  reorder?: { from: number; to: number; segments: SegmentDto[] };
}

// Imported rather than read at runtime, so a moved or malformed file fails
// the typecheck and the build, not just this run.
const table = rawTable as unknown as {
  whole: { durationMs: number; segments: SegmentDto[] }[];
  cases: Case[];
};

/** The operations live on the composable, which persists on every edit; the
 * sidecar write is not what this file is about, so the IPC is a stub. */
function applyTo(segments: SegmentDto[], op: (e: Editor) => void): SegmentDto[] {
  const editor = useEditorTimeline("cap one", { segments });
  op(editor);
  return editor.timeline.value.segments;
}

type Editor = ReturnType<typeof useEditorTimeline>;

/** The operation rows, flattened so each becomes its own named case. Written
 * as a filter-and-map rather than three `it.each`es with non-null assertions
 * because the table is allowed to omit any of them. */
const ops = table.cases.flatMap((c) => {
  const run: { name: string; apply: (e: Editor) => void; segments: SegmentDto[] }[] = [];
  const { splitAt, delete: remove, reorder } = c;
  if (splitAt) {
    run.push({
      name: `splits ${c.name}`,
      apply: (e) => e.splitAt(splitAt.outputMs),
      segments: splitAt.segments,
    });
  }
  if (remove) {
    run.push({
      name: `deletes from ${c.name}`,
      apply: (e) => e.deleteSegment(remove.index),
      segments: remove.segments,
    });
  }
  if (reorder) {
    run.push({
      name: `reorders ${c.name}`,
      apply: (e) => e.reorder(reorder.from, reorder.to),
      segments: reorder.segments,
    });
  }
  return run.map((r) => ({ ...r, source: c.segments }));
});

describe("the shared timeline fixture table", () => {
  beforeEach(() => mockIPC(() => undefined));

  // A table nothing iterates proves nothing, and one that silently shrinks to
  // a single row proves almost nothing. The Rust half asserts the same counts
  // against the same file, so a row deleted on one side reddens both.
  it("covers every case the Rust twin reads from the same file", () => {
    expect(table.cases).toHaveLength(6);
    expect(table.whole).toHaveLength(2);
    expect(ops).toHaveLength(4);
  });

  it.each(table.whole)("seeds an unedited capture of $durationMs ms", (row) => {
    expect(wholeTimeline(row.durationMs)).toEqual({ segments: row.segments });
  });

  // The mapping GAP-136 is actually about: where an output moment lands in
  // the source, and how long the output is. Each expectation is compared as
  // an object carrying its own input, so a failure names the moment rather
  // than just the value.
  it.each(table.cases)("maps output time to source time: $name", (c) => {
    const t: TimelineDto = { segments: c.segments };
    expect(outputDurationMs(t)).toBe(c.outputDurationMs);
    for (const [outputMs, expected] of c.toSourceMs) {
      expect({ outputMs, source: toSourceMs(t, outputMs) }).toEqual({ outputMs, source: expected });
    }
  });

  it.each(ops)("$name", (row) => {
    expect(applyTo(row.source, row.apply)).toEqual(row.segments);
  });
});
