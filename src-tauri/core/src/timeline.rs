//! The editor's segment algebra: which spans of a staged Screen Capture play,
//! and in what order. Pure arithmetic with no I/O, so the editor's correctness
//! is testable on Linux even though a screen can only be captured on Windows
//! (spec §4.2, §8.1).
//!
//! Every operation returns a NEW `Timeline`, which is what makes undo/redo a
//! stack of snapshots rather than a set of inverse operations.

/// A half-open span `[source_start_ms, source_end_ms)` of the staged capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub source_start_ms: u64,
    pub source_end_ms: u64,
}

impl Segment {
    pub fn duration_ms(&self) -> u64 {
        self.source_end_ms.saturating_sub(self.source_start_ms)
    }
}

/// The ordered list of segments an editor session produces. Segments never
/// overlap, are never empty, and may appear in any order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Timeline {
    pub segments: Vec<Segment>,
}

impl Timeline {
    /// The untouched timeline: one segment spanning the whole source.
    pub fn whole(duration_ms: u64) -> Timeline {
        if duration_ms == 0 {
            return Timeline {
                segments: Vec::new(),
            };
        }
        Timeline {
            segments: vec![Segment {
                source_start_ms: 0,
                source_end_ms: duration_ms,
            }],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn output_duration_ms(&self) -> u64 {
        self.segments.iter().map(Segment::duration_ms).sum()
    }

    /// Split the segment containing `output_ms` into two at that point.
    ///
    /// A split landing exactly on a segment boundary (including 0 and the
    /// end) is a NO-OP. Allowing it would mint a zero-length segment, which
    /// the exporter cannot encode.
    pub fn split_at(&self, output_ms: u64) -> Timeline {
        let mut elapsed = 0u64;
        let mut out = Vec::with_capacity(self.segments.len() + 1);
        // No "already split" veto is needed here: segments are disjoint and
        // `elapsed` strictly increases across the loop, so `output_ms >
        // elapsed && output_ms < seg_end` can hold for at most one segment.
        // Once it does, `elapsed` becomes `seg_end`, which is already known
        // to exceed `output_ms` — so `output_ms > elapsed` is false for
        // every later segment regardless of a flag. (Same unreachable-guard
        // class as `screen_geometry::clamp_to_frame`, b4162da.)
        for seg in &self.segments {
            let seg_end = elapsed + seg.duration_ms();
            if output_ms > elapsed && output_ms < seg_end {
                let cut = seg.source_start_ms + (output_ms - elapsed);
                out.push(Segment {
                    source_start_ms: seg.source_start_ms,
                    source_end_ms: cut,
                });
                out.push(Segment {
                    source_start_ms: cut,
                    source_end_ms: seg.source_end_ms,
                });
            } else {
                out.push(*seg);
            }
            elapsed = seg_end;
        }
        Timeline { segments: out }
    }

    /// Remove one segment. Out of range is a no-op.
    pub fn delete(&self, index: usize) -> Timeline {
        if index >= self.segments.len() {
            return self.clone();
        }
        let mut segments = self.segments.clone();
        segments.remove(index);
        Timeline { segments }
    }

    /// Move the segment at `from` to position `to`. Either index out of range
    /// is a no-op.
    pub fn reorder(&self, from: usize, to: usize) -> Timeline {
        if from >= self.segments.len() || to >= self.segments.len() {
            return self.clone();
        }
        let mut segments = self.segments.clone();
        let seg = segments.remove(from);
        segments.insert(to, seg);
        Timeline { segments }
    }

    /// Map a point on the OUTPUT timeline back to a point in the source.
    /// `None` once `output_ms` reaches or passes the output duration — the
    /// span is half-open, so the end itself is not a playable instant.
    pub fn to_source_ms(&self, output_ms: u64) -> Option<u64> {
        let mut elapsed = 0u64;
        for seg in &self.segments {
            let seg_end = elapsed + seg.duration_ms();
            if output_ms < seg_end {
                return Some(seg.source_start_ms + (output_ms - elapsed));
            }
            elapsed = seg_end;
        }
        None
    }

    /// True when this timeline is exactly the whole source, unedited — the
    /// export fast path (spec §8.3), which remuxes instead of re-encoding.
    pub fn is_untouched(&self, source_duration_ms: u64) -> bool {
        matches!(
            self.segments.as_slice(),
            [Segment { source_start_ms: 0, source_end_ms }] if *source_end_ms == source_duration_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: u64, end: u64) -> Segment {
        Segment {
            source_start_ms: start,
            source_end_ms: end,
        }
    }

    #[test]
    fn whole_is_one_segment_spanning_the_source() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.segments, vec![seg(0, 5_000)]);
        assert_eq!(t.output_duration_ms(), 5_000);
    }

    // Regression: a zero-duration source (an empty/failed capture) must not
    // mint a zero-length segment. The same invariant as the boundary-split
    // no-op above — a zero-length segment reaches the exporter as an
    // unplayable frame plan (spec §8.1).
    #[test]
    fn whole_of_zero_duration_is_an_empty_timeline() {
        let t = Timeline::whole(0);
        assert!(t.is_empty());
        assert_eq!(t.output_duration_ms(), 0);
    }

    #[test]
    fn split_divides_the_containing_segment_at_the_playhead() {
        let t = Timeline::whole(5_000).split_at(2_000);
        assert_eq!(t.segments, vec![seg(0, 2_000), seg(2_000, 5_000)]);
        assert_eq!(
            t.output_duration_ms(),
            5_000,
            "a split never changes duration"
        );
    }

    // Regression: a split exactly on a boundary must be a NO-OP, not a
    // zero-length segment. A zero-length segment reaches the exporter and
    // produces an unplayable file (spec §8.1).
    #[test]
    fn split_on_an_existing_boundary_is_a_noop() {
        let t = Timeline::whole(5_000).split_at(2_000);
        assert_eq!(
            t.clone().split_at(2_000),
            t,
            "boundary split changes nothing"
        );
        assert_eq!(t.clone().split_at(0), t, "split at 0 changes nothing");
        assert_eq!(
            t.clone().split_at(5_000),
            t,
            "split at the end changes nothing"
        );
    }

    #[test]
    fn split_past_the_end_is_a_noop() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.clone().split_at(9_999), t);
    }

    #[test]
    fn delete_removes_one_segment_and_shortens_the_output() {
        let t = Timeline::whole(5_000).split_at(2_000).delete(0);
        assert_eq!(t.segments, vec![seg(2_000, 5_000)]);
        assert_eq!(t.output_duration_ms(), 3_000);
    }

    #[test]
    fn delete_out_of_range_is_a_noop() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.clone().delete(7), t);
    }

    #[test]
    fn deleting_the_last_segment_yields_an_empty_timeline() {
        let t = Timeline::whole(5_000).delete(0);
        assert!(t.is_empty());
        assert_eq!(t.output_duration_ms(), 0);
    }

    #[test]
    fn reorder_moves_a_segment_without_changing_duration() {
        let t = Timeline::whole(6_000).split_at(2_000).split_at(4_000);
        assert_eq!(
            t.segments,
            vec![seg(0, 2_000), seg(2_000, 4_000), seg(4_000, 6_000)]
        );
        let r = t.reorder(0, 2);
        assert_eq!(
            r.segments,
            vec![seg(2_000, 4_000), seg(4_000, 6_000), seg(0, 2_000)]
        );
        assert_eq!(r.output_duration_ms(), 6_000);
    }

    #[test]
    fn reorder_out_of_range_is_a_noop() {
        let t = Timeline::whole(5_000).split_at(2_000);
        assert_eq!(t.clone().reorder(0, 9), t);
        assert_eq!(t.clone().reorder(9, 0), t);
    }

    #[test]
    fn to_source_ms_maps_output_time_through_reordered_segments() {
        let t = Timeline::whole(6_000)
            .split_at(2_000)
            .split_at(4_000)
            .reorder(0, 2);
        // Output now plays [2000..4000), [4000..6000), [0..2000).
        assert_eq!(t.to_source_ms(0), Some(2_000));
        assert_eq!(t.to_source_ms(1_500), Some(3_500));
        assert_eq!(
            t.to_source_ms(2_000),
            Some(4_000),
            "first frame of segment 2"
        );
        assert_eq!(
            t.to_source_ms(4_500),
            Some(500),
            "inside the moved-to-last segment"
        );
    }

    #[test]
    fn to_source_ms_is_none_past_the_end() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.to_source_ms(5_000), None, "end is exclusive");
        assert_eq!(t.to_source_ms(9_999), None);
    }

    #[test]
    fn is_untouched_only_for_a_single_full_span_segment() {
        assert!(
            Timeline::whole(5_000).is_untouched(5_000),
            "the fast-path case"
        );
        assert!(!Timeline::whole(5_000).split_at(2_000).is_untouched(5_000));
        assert!(!Timeline::whole(5_000).delete(0).is_untouched(5_000));
        assert!(
            !Timeline {
                segments: vec![seg(0, 4_000)]
            }
            .is_untouched(5_000),
            "a trimmed tail is not untouched"
        );
    }

    // Invariant: no operation may ever produce an empty or inverted segment.
    // Anything violating this reaches the exporter as a corrupt frame plan.
    #[test]
    fn every_operation_preserves_non_empty_segments() {
        let cases = vec![
            Timeline::whole(5_000),
            Timeline::whole(5_000).split_at(2_500),
            Timeline::whole(5_000).split_at(2_500).delete(0),
            Timeline::whole(6_000)
                .split_at(2_000)
                .split_at(4_000)
                .reorder(2, 0),
        ];
        for t in cases {
            for s in &t.segments {
                assert!(
                    s.source_start_ms < s.source_end_ms,
                    "segment {s:?} is empty or inverted"
                );
            }
        }
    }

    // ---- the SHARED fixture table -------------------------------------
    //
    // This algebra exists twice, in two languages: here, and in
    // `src/utils/timelineGeometry.ts` + `src/composables/useEditorTimeline.ts`
    // -- the one phase 5's export plans on, and the one the user actually
    // watches. Each had its own tests and no fixture in common, so a
    // disagreement between them was invisible: the exported file would not
    // match the preview the user approved, with every test in the repo green
    // (docs/Gaps.md GAP-136). Running one table through both found two real
    // disagreements, a backwards segment and `whole(0)`; both are rows below.
    //
    // `tests/timelineFixtures.test.ts` reads this exact file and asserts the
    // same expectations. `include_str!` is what makes the sharing real: move
    // or delete the fixture and this crate stops compiling, rather than
    // quietly testing nothing.
    const SHARED_FIXTURES: &str = include_str!("../../../tests/fixtures/timeline-cases.json");

    fn fixtures() -> serde_json::Value {
        serde_json::from_str(SHARED_FIXTURES).expect("the shared timeline fixture table is JSON")
    }

    // Segment keys are read by the names the editor WRITES onto disk
    // (camelCase). `Segment` has no serde derives (GAP-135), so this is a
    // hand mapping on purpose -- and it is the only place in Rust that spells
    // the on-disk names at all.
    fn segments_of(v: &serde_json::Value) -> Vec<Segment> {
        v.as_array()
            .expect("segments array")
            .iter()
            .map(|s| Segment {
                source_start_ms: s["sourceStartMs"].as_u64().expect("sourceStartMs"),
                source_end_ms: s["sourceEndMs"].as_u64().expect("sourceEndMs"),
            })
            .collect()
    }

    #[test]
    fn shared_fixture_table_maps_output_time_to_source_time() {
        let table = fixtures();
        let cases = table["cases"].as_array().expect("cases");
        // A table nothing iterates proves nothing, and one that silently
        // shrinks to a single row proves almost nothing. The TypeScript half
        // asserts the same count against the same file.
        assert_eq!(cases.len(), 6, "the shared table lost or gained a case");
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let t = Timeline {
                segments: segments_of(&case["segments"]),
            };
            assert_eq!(
                t.output_duration_ms(),
                case["outputDurationMs"].as_u64().expect("outputDurationMs"),
                "output duration disagrees for {name}"
            );
            for row in case["toSourceMs"].as_array().expect("toSourceMs") {
                let output_ms = row[0].as_u64().expect("outputMs");
                let expected = row[1].as_u64();
                assert_eq!(
                    t.to_source_ms(output_ms),
                    expected,
                    "to_source_ms({output_ms}) disagrees for {name}"
                );
            }
        }
    }

    #[test]
    fn shared_fixture_table_agrees_on_whole_and_on_the_operations() {
        let table = fixtures();
        let whole = table["whole"].as_array().expect("whole");
        assert_eq!(whole.len(), 2, "the shared table lost a `whole` row");
        for row in whole {
            let duration = row["durationMs"].as_u64().expect("durationMs");
            assert_eq!(
                Timeline::whole(duration).segments,
                segments_of(&row["segments"]),
                "whole({duration}) disagrees"
            );
        }
        // Every operation row that the table carries, which is how a new row
        // becomes live on both sides at once rather than in one language.
        let mut applied = 0;
        for case in table["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().expect("name");
            let t = Timeline {
                segments: segments_of(&case["segments"]),
            };
            if let Some(op) = case.get("splitAt") {
                applied += 1;
                let at = op["outputMs"].as_u64().expect("outputMs");
                assert_eq!(
                    t.split_at(at).segments,
                    segments_of(&op["segments"]),
                    "split_at({at}) disagrees for {name}"
                );
            }
            if let Some(op) = case.get("delete") {
                applied += 1;
                let index = op["index"].as_u64().expect("index") as usize;
                assert_eq!(
                    t.delete(index).segments,
                    segments_of(&op["segments"]),
                    "delete({index}) disagrees for {name}"
                );
            }
            if let Some(op) = case.get("reorder") {
                applied += 1;
                let from = op["from"].as_u64().expect("from") as usize;
                let to = op["to"].as_u64().expect("to") as usize;
                assert_eq!(
                    t.reorder(from, to).segments,
                    segments_of(&op["segments"]),
                    "reorder({from}, {to}) disagrees for {name}"
                );
            }
        }
        // Without this, a table whose operation rows were all renamed or
        // dropped would pass by asserting nothing at all.
        assert_eq!(applied, 4, "the shared table lost an operation row");
    }
}
