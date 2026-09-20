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

    /// Map a SOURCE timestamp onto the OUTPUT timeline, or `None` when it
    /// does not fall inside this span.
    ///
    /// HALF-OPEN, exactly like `Timeline::to_source_ms`: `source_end_ms`
    /// itself is not in the span. The two rules must agree or the exported
    /// file does not match the preview the user approved — and a `<=` here
    /// would put one source instant in two spans at every cut, so the
    /// exporter would write the same frame at two different output times
    /// and the muxer would see a backwards timestamp.
    pub fn restamp(&self, source_ms: u64) -> Option<u64> {
        if source_ms < self.source_start_ms || source_ms >= self.source_end_ms {
            return None;
        }
        Some(self.output_start_ms + (source_ms - self.source_start_ms))
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

/// Total length of the exported file, in output milliseconds.
pub fn plan_output_duration_ms(spans: &[PlanSpan]) -> u64 {
    spans
        .iter()
        .fold(0u64, |acc, s| acc.saturating_add(s.duration_ms()))
}

/// Export progress as a whole percent, 0..=100.
///
/// Integer arithmetic on purpose: this is what `core::throttle` gates on,
/// and `progress_fraction` derives the emitted float from it, so the number
/// that passes the gate and the number the user sees are the same number.
/// A zero-length plan is COMPLETE rather than a division by zero — there is
/// nothing left to write.
pub fn progress_percent(written_output_ms: u64, total_output_ms: u64) -> u64 {
    if total_output_ms == 0 {
        return 100;
    }
    let scaled = written_output_ms.saturating_mul(100) / total_output_ms;
    scaled.min(100)
}

/// The same progress as the fraction spec §8.3's event carries.
pub fn progress_fraction(written_output_ms: u64, total_output_ms: u64) -> f64 {
    progress_percent(written_output_ms, total_output_ms) as f64 / 100.0
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

    // The inverse of Timeline::to_source_ms, and the direction the phase-4
    // review found easiest to get subtly wrong: a `<=` on source_end_ms
    // stayed green against every fixture that probed only cut-out points,
    // because those fall outside every span under BOTH rules. These probe a
    // span's OWN far edge, which is the only place the two rules differ.
    #[test]
    fn a_span_restamps_its_own_source_range_and_refuses_its_far_edge() {
        let span = PlanSpan {
            source_start_ms: 4_000,
            source_end_ms: 6_000,
            output_start_ms: 1_000,
        };
        assert_eq!(span.restamp(4_000), Some(1_000));
        assert_eq!(span.restamp(5_999), Some(2_999));
        // Half-open, exactly like Timeline::to_source_ms: the end instant
        // belongs to whatever span comes next, never to this one.
        assert_eq!(span.restamp(6_000), None);
        assert_eq!(span.restamp(3_999), None);
    }

    #[test]
    fn restamping_a_reordered_plan_puts_later_source_earlier_in_the_output() {
        let t = Timeline::whole(6_000)
            .split_at(2_000)
            .split_at(4_000)
            .reorder(0, 2);
        let spans = plan(&t);
        // Source 4_500 lives in the span that now plays SECOND, so it lands
        // after the first span's 2_000 ms but before the third's.
        let hit: Vec<u64> = spans.iter().filter_map(|s| s.restamp(4_500)).collect();
        assert_eq!(hit, vec![2_500]);
        // Source 500 is the block moved to the END, so it lands last.
        let hit: Vec<u64> = spans.iter().filter_map(|s| s.restamp(500)).collect();
        assert_eq!(hit, vec![4_500]);
    }

    #[test]
    fn a_source_instant_belongs_to_exactly_one_span_of_a_disjoint_plan() {
        let t = Timeline::whole(9_000).split_at(3_000).split_at(6_000);
        let spans = plan(&t);
        for source_ms in [0u64, 2_999, 3_000, 5_999, 6_000, 8_999] {
            let hits = spans
                .iter()
                .filter(|s| s.restamp(source_ms).is_some())
                .count();
            assert_eq!(hits, 1, "source {source_ms} matched {hits} spans");
        }
        assert!(spans.iter().all(|s| s.restamp(9_000).is_none()));
    }

    #[test]
    fn the_plans_output_duration_is_the_sum_of_its_spans() {
        let t = Timeline::whole(9_000)
            .split_at(2_000)
            .split_at(5_000)
            .delete(1);
        let spans = plan(&t);
        assert_eq!(plan_output_duration_ms(&spans), 6_000);
        assert_eq!(plan_output_duration_ms(&[]), 0);
    }

    #[test]
    fn progress_is_an_integer_percent_that_clamps_at_both_ends() {
        assert_eq!(progress_percent(0, 8_000), 0);
        assert_eq!(progress_percent(2_000, 8_000), 25);
        assert_eq!(progress_percent(8_000, 8_000), 100);
        // A restamped sample can sit a hair past the planned end (the last
        // frame's duration runs off the edge); it must read 100, not 101.
        assert_eq!(progress_percent(8_400, 8_000), 100);
    }

    #[test]
    fn progress_of_an_empty_export_is_complete_rather_than_a_division_by_zero() {
        assert_eq!(progress_percent(0, 0), 100);
        assert_eq!(progress_fraction(0, 0), 1.0);
    }

    // ONE computation, two shapes: the number the throttle gates on and the
    // number the user sees must never disagree at a boundary.
    #[test]
    fn the_emitted_fraction_is_exactly_the_gated_percent() {
        for (written, total) in [
            (0u64, 7_000u64),
            (1_000, 7_000),
            (3_500, 7_000),
            (7_000, 7_000),
        ] {
            let pct = progress_percent(written, total);
            assert_eq!(progress_fraction(written, total), pct as f64 / 100.0);
        }
    }
}
