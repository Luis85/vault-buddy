//! Turn an edited `Timeline` into the ordered list of source spans the
//! exporter reads, each carrying the output timestamp its first frame lands
//! on.
//!
//! Pure, so the export ordering is testable on Linux even though decoding
//! and encoding are Windows-only (spec §8.3).

use vault_buddy_core::timeline::Timeline;

/// One source span plus where it starts on the OUTPUT timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSpan {
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub output_start_ms: u64,
}

impl PlanSpan {
    pub fn duration_ms(&self) -> u64 {
        self.source_end_ms.saturating_sub(self.source_start_ms)
    }
}

/// Build the export plan. Output timestamps are contiguous and monotonic by
/// construction — a gap or overlap produces a file that stutters or that the
/// muxer rejects.
///
/// Zero-length segments are dropped. `Timeline`'s own operations never make
/// one, but this is the last gate before the encoder, which cannot encode an
/// empty span.
pub fn plan(timeline: &Timeline) -> Vec<PlanSpan> {
    let mut output_start_ms = 0u64;
    let mut spans = Vec::with_capacity(timeline.segments.len());
    for seg in &timeline.segments {
        if seg.source_start_ms >= seg.source_end_ms {
            continue;
        }
        spans.push(PlanSpan {
            source_start_ms: seg.source_start_ms,
            source_end_ms: seg.source_end_ms,
            output_start_ms,
        });
        output_start_ms = output_start_ms.saturating_add(seg.duration_ms());
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_core::timeline::{Segment, Timeline};

    #[test]
    fn an_untouched_timeline_plans_one_span_at_output_zero() {
        let spans = plan(&Timeline::whole(5_000));
        assert_eq!(
            spans,
            vec![PlanSpan {
                source_start_ms: 0,
                source_end_ms: 5_000,
                output_start_ms: 0
            }]
        );
    }

    #[test]
    fn an_empty_timeline_plans_nothing() {
        assert!(plan(&Timeline::default()).is_empty());
    }

    #[test]
    fn a_deleted_segment_is_absent_and_later_output_shifts_earlier() {
        let t = Timeline::whole(6_000).split_at(2_000).delete(0);
        assert_eq!(
            plan(&t),
            vec![PlanSpan {
                source_start_ms: 2_000,
                source_end_ms: 6_000,
                output_start_ms: 0
            }]
        );
    }

    #[test]
    fn reordered_segments_are_planned_in_output_order_with_rewritten_timestamps() {
        let t = Timeline::whole(6_000)
            .split_at(2_000)
            .split_at(4_000)
            .reorder(0, 2);
        assert_eq!(
            plan(&t),
            vec![
                PlanSpan {
                    source_start_ms: 2_000,
                    source_end_ms: 4_000,
                    output_start_ms: 0
                },
                PlanSpan {
                    source_start_ms: 4_000,
                    source_end_ms: 6_000,
                    output_start_ms: 2_000
                },
                PlanSpan {
                    source_start_ms: 0,
                    source_end_ms: 2_000,
                    output_start_ms: 4_000
                },
            ]
        );
    }

    // The exporter writes these timestamps straight into the muxer. A gap or
    // an overlap in output time produces a file that stutters or is rejected.
    #[test]
    fn output_time_is_contiguous_and_monotonic() {
        let t = Timeline::whole(9_000)
            .split_at(2_000)
            .split_at(5_000)
            .reorder(2, 0)
            .reorder(1, 2);
        let spans = plan(&t);
        let mut expected_next = 0u64;
        for s in &spans {
            assert_eq!(s.output_start_ms, expected_next, "contiguous: {spans:?}");
            expected_next += s.duration_ms();
        }
        assert_eq!(
            expected_next,
            t.output_duration_ms(),
            "plan covers the whole output"
        );
    }

    #[test]
    fn every_planned_span_is_non_empty() {
        let t = Timeline::whole(6_000).split_at(3_000);
        for s in plan(&t) {
            assert!(
                s.source_start_ms < s.source_end_ms,
                "empty span {s:?} would break the encoder"
            );
        }
    }

    // Pins the round-trip Phase 2's seek/scrub path leans on: every output
    // timestamp inside a planned span must map back through
    // Timeline::to_source_ms to the matching source timestamp, and the plan
    // must not claim any output time past the timeline's own duration. Uses
    // the reorder fixture above, where source and output order genuinely
    // differ, so a plan/to_source_ms disagreement about ordering would show.
    #[test]
    fn plan_spans_round_trip_through_to_source_ms() {
        let t = Timeline::whole(6_000)
            .split_at(2_000)
            .split_at(4_000)
            .reorder(0, 2);
        let spans = plan(&t);
        for span in &spans {
            let duration = span.duration_ms();
            for k in [0, 1, duration.saturating_sub(1)] {
                assert_eq!(
                    t.to_source_ms(span.output_start_ms + k),
                    Some(span.source_start_ms + k),
                    "span {span:?} offset {k} did not round-trip"
                );
            }
        }
        assert_eq!(
            t.to_source_ms(t.output_duration_ms()),
            None,
            "output duration itself is past the end, exclusive"
        );
    }

    #[test]
    fn plan_skips_a_hand_constructed_empty_segment() {
        // Timeline's own operations never produce one, but plan() is the last
        // gate before the encoder and must not forward it.
        let t = Timeline {
            segments: vec![
                Segment {
                    source_start_ms: 0,
                    source_end_ms: 1_000,
                },
                Segment {
                    source_start_ms: 2_000,
                    source_end_ms: 2_000,
                },
                Segment {
                    source_start_ms: 3_000,
                    source_end_ms: 4_000,
                },
            ],
        };
        let spans = plan(&t);
        assert_eq!(spans.len(), 2, "the zero-length segment is dropped");
        assert_eq!(
            spans[1].output_start_ms, 1_000,
            "output time stays contiguous"
        );
    }
}
