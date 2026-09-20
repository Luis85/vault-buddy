import { describe, expect, it } from "vitest";

import {
  dropIndex,
  outputDurationMs,
  segmentAtOutputMs,
  segmentWidths,
  toOutputMs,
  toSourceMs,
  wholeTimeline,
} from "../src/utils/timelineGeometry";

const T = {
  segments: [
    { sourceStartMs: 0, sourceEndMs: 1000 },
    { sourceStartMs: 5000, sourceEndMs: 8000 },
  ],
};

describe("timeline geometry", () => {
  it("sums segment durations, not source span", () => {
    // 1000 + 3000 — the 4000 ms cut out between them is gone.
    expect(outputDurationMs(T)).toBe(4000);
    expect(outputDurationMs({ segments: [] })).toBe(0);
  });

  it("renders widths as percentages of the OUTPUT duration", () => {
    expect(segmentWidths(T)).toEqual([25, 75]);
    // An empty timeline has no blocks rather than a divide-by-zero.
    expect(segmentWidths({ segments: [] })).toEqual([]);
  });

  // The whole point of the model: output time is not source time.
  it("maps output time back into the source", () => {
    expect(toSourceMs(T, 0)).toBe(0);
    expect(toSourceMs(T, 999)).toBe(999);
    // The first millisecond of segment 2 is source 5000, not 1000.
    expect(toSourceMs(T, 1000)).toBe(5000);
    expect(toSourceMs(T, 2500)).toBe(6500);
    // Past the end is not a source time at all.
    expect(toSourceMs(T, 4000)).toBeNull();
    expect(toSourceMs(T, 9999)).toBeNull();
    expect(toSourceMs({ segments: [] }, 0)).toBeNull();
    // Mutation-table gap: without this, deleting the `outputMs < 0` guard
    // survives the whole suite and a negative output time (a pointer dragged
    // left of the strip yields one) resolves to a source time before the
    // segment's own start instead of "nowhere".
    expect(toSourceMs(T, -1)).toBeNull();
  });

  // The INVERSE, and the preview cannot work without it: the video element
  // reports SOURCE time, while the strip, the playhead and the scrubber all
  // speak output time. A source moment that was cut out has no output time
  // at all, which is a real answer, not an error.
  it("maps source time back into the output", () => {
    expect(toOutputMs(T, 0)).toBe(0);
    expect(toOutputMs(T, 999)).toBe(999);
    expect(toOutputMs(T, 5000)).toBe(1000);
    expect(toOutputMs(T, 6500)).toBe(2500);
    // 1000..5000 was cut out — it is nowhere in the output.
    expect(toOutputMs(T, 2000)).toBeNull();
    expect(toOutputMs(T, 9000)).toBeNull();
    expect(toOutputMs({ segments: [] }, 0)).toBeNull();
    // A segment's own END is outside it: the span is half-open. Source 1000
    // is the FIRST cut-out millisecond, not the last of segment 1, and 8000
    // is one past the last playable instant of segment 2. Without these two,
    // `sourceMs <= s.sourceEndMs` survives the whole suite (the plan's
    // mutation table claims the cut-out fixtures above catch it; they do
    // not — 2000 and 9000 fall outside every segment under BOTH rules).
    // The mutant maps source 1000 AND source 5000 to output 1000, which is
    // exactly the two-clock drift the round-trip test exists to prevent.
    expect(toOutputMs(T, 1000)).toBeNull();
    expect(toOutputMs(T, 8000)).toBeNull();
  });

  // Round-trip: the two must agree, or the playhead drifts from the frame
  // on screen every time playback crosses a cut.
  it("round-trips output through source and back", () => {
    for (const ms of [0, 1, 999, 1000, 2500, 3999]) {
      expect(toOutputMs(T, toSourceMs(T, ms) as number)).toBe(ms);
    }
  });

  it("names the segment an output time falls in", () => {
    expect(segmentAtOutputMs(T, 0)).toBe(0);
    expect(segmentAtOutputMs(T, 999)).toBe(0);
    // The boundary belongs to the segment it STARTS, not the one it ends.
    expect(segmentAtOutputMs(T, 1000)).toBe(1);
    expect(segmentAtOutputMs(T, 3999)).toBe(1);
    expect(segmentAtOutputMs(T, 4000)).toBeNull();
    // Mutation-table gap, the sibling of the `toSourceMs` one: without this,
    // deleting the `outputMs < 0` guard survives the whole suite and a
    // negative output time names segment 0 instead of no segment at all.
    expect(segmentAtOutputMs(T, -1)).toBeNull();
  });

  // Drag-to-reorder: where does a drop at this fraction of the strip land?
  // Asymmetric widths on purpose — equal widths cannot distinguish a
  // midpoint rule from an accumulate-until-exceeded rule.
  it("picks a drop index from where the pointer is along the strip", () => {
    const w = [25, 75];
    expect(dropIndex(w, 0)).toBe(0);
    expect(dropIndex(w, 0.1)).toBe(0); // before block 0's midpoint (12.5%)
    expect(dropIndex(w, 0.2)).toBe(1); // past it
    expect(dropIndex(w, 0.6)).toBe(1); // before block 1's midpoint (62.5%)
    expect(dropIndex(w, 0.9)).toBe(2); // past it — dropped at the end
    expect(dropIndex([], 0.5)).toBe(0);
  });

  // The TypeScript mirror of `core::timeline::Timeline::whole`, including the
  // guard the Rust side carries a named regression test for
  // (`whole_of_zero_duration_is_an_empty_timeline`: "a zero-duration source
  // must not mint a zero-length segment ... reaches the exporter as an
  // unplayable frame plan"). The re-implementation in `EditorRoot`'s `load`
  // had dropped it and seeded `[{0, 0}]` — one of the two measured
  // divergences between the two implementations (docs/Gaps.md GAP-136).
  it("seeds the whole capture, and nothing at all for a zero-duration one", () => {
    expect(wholeTimeline(6000)).toEqual({
      segments: [{ sourceStartMs: 0, sourceEndMs: 6000 }],
    });
    expect(wholeTimeline(0)).toEqual({ segments: [] });
    // A negative duration cannot arrive from a `u64` on the Rust side, but
    // the field is JSON by the time it gets here and the answer must not be
    // a backwards segment.
    expect(wholeTimeline(-1)).toEqual({ segments: [] });
  });

  // The OTHER measured divergence (GAP-136). `Segment::duration_ms` is a
  // `saturating_sub` in Rust, so a segment whose end precedes its start
  // contributes NOTHING; a raw subtraction here contributed a NEGATIVE length
  // that walked the accumulator backwards, and the two languages then
  // disagreed about the output duration AND about where an output moment
  // lands in the source. No operation on either side mints one — a
  // hand-edited or sync-conflicted sidecar does, and nothing validates it
  // (GAP-134).
  //
  // The numbers are the ones `core::timeline` answers for this fixture, and
  // the cases are chosen so a raw subtraction cannot pass: it gives duration
  // 3000 (not 4000) and `toSourceMs(2000) === 5000` (not 4000).
  it("treats a backwards segment as empty, the way the Rust twin does", () => {
    const inverted = {
      segments: [
        { sourceStartMs: 0, sourceEndMs: 2000 },
        { sourceStartMs: 4000, sourceEndMs: 3000 },
        { sourceStartMs: 4000, sourceEndMs: 6000 },
      ],
    };
    expect(outputDurationMs(inverted)).toBe(4000);
    expect(toSourceMs(inverted, 2000)).toBe(4000);
    expect(toSourceMs(inverted, 3999)).toBe(5999);
    expect(toSourceMs(inverted, 4000)).toBeNull();
    // It occupies no width and owns no output moment either.
    expect(segmentWidths(inverted)).toEqual([50, 0, 50]);
    expect(segmentAtOutputMs(inverted, 2000)).toBe(2);
    expect(toOutputMs(inverted, 4500)).toBe(2500);
  });
});
