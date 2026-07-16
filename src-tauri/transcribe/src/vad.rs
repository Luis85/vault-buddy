//! Voice Activity Detection: convert Silero's raw centisecond speech
//! segments into sample-space spans, build a filtered (speech-only) sample
//! buffer, and remap whisper's output timestamps from the filtered
//! timeline back to the original recording's timeline.
//!
//! Why this module exists at all: whisper-rs 0.16 routes inference through
//! `whisper_full_with_state`, and whisper.cpp only applies its own VAD
//! filtering inside `whisper_full`/`whisper_full_parallel` — the no-state
//! entry points, unreachable from whisper-rs (verified against the
//! vendored whisper.cpp source: `whisper_full_with_state` never reads
//! `params.vad` at all). So `FullParams`' `enable_vad`/
//! `set_vad_model_path`/`set_vad_params` are dead code on our call path —
//! they configure a VAD run that whisper.cpp's own no-state entry point
//! never performs. This module reimplements the filter+remap step in
//! Rust, using whisper-rs's separate standalone `WhisperVadContext` API
//! (`detect_speech_centiseconds` below, feature-gated) to actually run
//! Silero, and does the sample-span math in pure, feature-ungated code so
//! it is unit-tested on every platform (including Linux, where the FFI
//! never builds).
//!
//! Pipeline: `detect_speech_centiseconds` (FFI) -> `spans_from_centiseconds`
//! (centiseconds -> clamped/merged sample spans) -> `filter_samples`
//! (concatenate the speech spans into one buffer + a `SpanMap`) -> whisper
//! runs on the filtered buffer -> `remap_ms` translates each output
//! segment's timestamp back to the original timeline via the `SpanMap`
//! (start and end timestamps use different boundary-tie rules — see
//! `TimestampKind`).

/// 16 kHz: 1 centisecond (10 ms) = 160 samples exactly.
const SAMPLES_PER_CS: f64 = 160.0;
/// 16 kHz: 1 millisecond = 16 samples exactly (an integer ratio, so
/// ms<->sample conversions below need no rounding).
const SAMPLES_PER_MS: u64 = 16;

/// A half-open span `[start, end)` of the ORIGINAL 16 kHz sample buffer
/// judged to be speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSpan {
    pub start: usize,
    pub end: usize,
}

/// Convert Silero's raw centisecond speech segments into ordered,
/// non-overlapping sample-space spans clamped to `total_samples`. Drops
/// empty/inverted segments (defensive — a malformed result must not panic
/// downstream) and merges overlapping/touching spans: the C side's
/// `speech_pad_ms`/`samples_overlap` params can legitimately produce them,
/// and an unmerged overlap would duplicate audio in the filtered buffer.
pub fn spans_from_centiseconds(segs: &[(f32, f32)], total_samples: usize) -> Vec<SpeechSpan> {
    let mut spans: Vec<SpeechSpan> = segs
        .iter()
        .filter_map(|&(start_cs, end_cs)| {
            let start = cs_to_samples(start_cs).min(total_samples);
            let end = cs_to_samples(end_cs).min(total_samples);
            (end > start).then_some(SpeechSpan { start, end })
        })
        .collect();
    spans.sort_by_key(|s| s.start);
    merge_overlapping(spans)
}

/// `as usize` on a float saturates (NaN/negative -> 0, overflow -> MAX)
/// rather than panicking or wrapping, so a malformed centisecond value
/// degrades to a clamped span instead of corrupting memory.
fn cs_to_samples(cs: f32) -> usize {
    (cs as f64 * SAMPLES_PER_CS).round() as usize
}

/// Merge a start-sorted span list wherever a later span begins at or before
/// the running span's end.
fn merge_overlapping(spans: Vec<SpeechSpan>) -> Vec<SpeechSpan> {
    let mut out: Vec<SpeechSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => out.push(span),
        }
    }
    out
}

/// One contiguous run inside the concatenated filtered (speech-only)
/// buffer, paired with where it started in the ORIGINAL buffer — what
/// `remap_ms` needs to translate a filtered-timeline position back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanMap {
    pub filtered_start: usize,
    pub original_start: usize,
    pub len: usize,
}

/// Concatenate `samples` at each of `spans` into one filtered buffer, plus
/// the map `remap_ms` needs. `spans` must already be ordered and
/// non-overlapping (see `spans_from_centiseconds`); each is also clamped to
/// `samples.len()` here as a second line of defense.
pub fn filter_samples(samples: &[f32], spans: &[SpeechSpan]) -> (Vec<f32>, Vec<SpanMap>) {
    let mut filtered = Vec::new();
    let mut map = Vec::with_capacity(spans.len());
    for span in spans {
        let start = span.start.min(samples.len());
        let end = span.end.min(samples.len());
        if end <= start {
            continue;
        }
        map.push(SpanMap {
            filtered_start: filtered.len(),
            original_start: start,
            len: end - start,
        });
        filtered.extend_from_slice(&samples[start..end]);
    }
    (filtered, map)
}

/// Which end of a whisper segment a timestamp represents. `remap_ms` needs
/// this because the correct resolution of an exact span-boundary tie
/// differs by which end it is: an END that lands exactly on a boundary
/// means "speech just stopped" (the earlier span's original end, the
/// closer original-timeline instant); a START that lands exactly on the
/// same boundary means "speech just resumed" (the NEXT span's original
/// start). Using the END rule for both would render a segment that begins
/// right after a VAD-collapsed silence gap as starting at the end of the
/// PREVIOUS span — early by the whole removed gap (Codex review finding,
/// GAP-60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampKind {
    Start,
    End,
}

/// Map a timestamp on the FILTERED timeline (ms) back to the ORIGINAL
/// timeline (ms) via the `SpanMap` list `filter_samples` returned. Since
/// the filtered buffer is the spans concatenated back-to-back with no gap,
/// a position always lands inside the map entry whose filtered range
/// covers it. `kind` (see `TimestampKind`) decides how an exact
/// span-to-span boundary tie resolves:
/// - `End`: the EARLIER entry, checked first via `<=` — whisper can
///   legitimately emit a segment's end timestamp exactly at a boundary,
///   and the earlier entry's end is the closer original-timeline instant.
/// - `Start`: the NEXT entry, via a strict `<` that makes the tied
///   boundary fail the earlier entry's check and fall through to the
///   following iteration, whose offset from ITS `filtered_start` (equal to
///   the tie point) is then zero — landing exactly on that entry's
///   original start.
///
/// The two rules agree everywhere except exactly at a boundary: interior
/// positions (strictly inside one entry's filtered range) and the very
/// first entry's own start take the same branch either way. A position
/// past every entry (whisper emitting a timestamp at, or a hair past, the
/// filtered buffer's very end) clamps to the last entry's original end for
/// both kinds — there is no "next entry" for a `Start` tie to advance to,
/// so it falls through to the same post-loop clamp as `End`. An empty map
/// (defensive — should not happen once VAD produced any segment) returns
/// `filtered_ms` unchanged: the safest identity when there is nothing to
/// map through.
pub fn remap_ms(filtered_ms: u64, map: &[SpanMap], kind: TimestampKind) -> u64 {
    let Some(last) = map.last() else {
        return filtered_ms;
    };
    let filtered_sample = filtered_ms * SAMPLES_PER_MS;
    for entry in map {
        let entry_end = (entry.filtered_start + entry.len) as u64;
        let reached = match kind {
            TimestampKind::End => filtered_sample <= entry_end,
            TimestampKind::Start => filtered_sample < entry_end,
        };
        if reached {
            let offset = filtered_sample.saturating_sub(entry.filtered_start as u64);
            let original_sample = entry.original_start as u64 + offset;
            return original_sample / SAMPLES_PER_MS;
        }
    }
    (last.original_start + last.len) as u64 / SAMPLES_PER_MS
}

/// Silero speech spans for `samples`, in centiseconds — the FFI half of
/// this module, kept as thin as possible so the span/remap math above stays
/// unit-tested on every platform. Errors are returned rather than panicking
/// (the caller — the engine — decides how to degrade); the only realistic
/// failure is a missing/corrupt model file. `WhisperVadContext` and the
/// `WhisperVadSegments` iterator it produces are constructed and fully
/// drained inside this one function: `WhisperVadSegments` is `!Send` (it
/// holds a raw C pointer with no explicit Send/Sync impl), so it must never
/// escape this scope. Per the design, the context is built fresh per job
/// rather than cached — the Silero model is ~1 MB, cheap to reload, and a
/// per-job context avoids holding a second whisper-adjacent native
/// allocation alive for the process lifetime.
#[cfg(feature = "whisper")]
pub fn detect_speech_centiseconds(
    model: &std::path::Path,
    samples: &[f32],
) -> Result<Vec<(f32, f32)>, String> {
    let model_path = model.to_string_lossy();
    let mut ctx = whisper_rs::WhisperVadContext::new(
        &model_path,
        whisper_rs::WhisperVadContextParams::default(),
    )
    .map_err(|e| format!("load VAD model {}: {e}", model.display()))?;
    let segments = ctx
        .segments_from_samples(whisper_rs::WhisperVadParams::default(), samples)
        .map_err(|e| format!("VAD detect on {}: {e}", model.display()))?;
    Ok(segments.map(|s| (s.start, s.end)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_when_one_span_covers_everything() {
        // total_samples = 16000 (1s @ 16kHz); one segment covering it all
        // (0..100 cs = 0..1000 ms = 0..16000 samples). The filtered
        // timeline IS the original timeline, so remap must be a no-op —
        // for both timestamp kinds, since none of these positions sit on a
        // span boundary (the only place Start and End can disagree).
        let spans = spans_from_centiseconds(&[(0.0, 100.0)], 16_000);
        assert_eq!(
            spans,
            vec![SpeechSpan {
                start: 0,
                end: 16_000
            }]
        );
        let samples = vec![0.0f32; 16_000];
        let (filtered, map) = filter_samples(&samples, &spans);
        assert_eq!(filtered.len(), 16_000);
        for ms in [0, 1, 250, 500, 999] {
            assert_eq!(
                remap_ms(ms, &map, TimestampKind::Start),
                ms,
                "identity at {ms}ms (Start)"
            );
            assert_eq!(
                remap_ms(ms, &map, TimestampKind::End),
                ms,
                "identity at {ms}ms (End)"
            );
        }
    }

    #[test]
    fn filtered_timestamp_inside_span_two_maps_to_original_span_two() {
        // span1: original [0, 16000) samples (0..1000ms); a gap; span2:
        // original [32000, 48000) samples (2000..3000ms). Filtered buffer =
        // span1 ++ span2 = 32000 samples (0..2000ms on the filtered
        // timeline). A filtered timestamp of 1500ms is 500ms into span2's
        // contribution -> original 2000 + 500 = 2500ms. 1500ms is strictly
        // inside span2 (not on a boundary), so both kinds agree.
        let spans = vec![
            SpeechSpan {
                start: 0,
                end: 16_000,
            },
            SpeechSpan {
                start: 32_000,
                end: 48_000,
            },
        ];
        let samples = vec![0.0f32; 48_000];
        let (filtered, map) = filter_samples(&samples, &spans);
        assert_eq!(filtered.len(), 32_000);
        assert_eq!(remap_ms(1500, &map, TimestampKind::Start), 2500);
        assert_eq!(remap_ms(1500, &map, TimestampKind::End), 2500);
    }

    #[test]
    fn gap_boundary_end_clamps_to_the_preceding_spans_original_end() {
        // The exact filtered-timeline boundary between span1 and span2
        // (filtered sample 16000 = 1000ms) is shared by both entries on the
        // filtered timeline (they're concatenated back-to-back) but an END
        // timestamp there must resolve to span1's ORIGINAL end (1000ms),
        // not span2's original start (2000ms) — whisper emitting a
        // segment's END exactly here means "speech stopped at the end of
        // span1". The two differ by the entire collapsed silence gap.
        let spans = vec![
            SpeechSpan {
                start: 0,
                end: 16_000,
            },
            SpeechSpan {
                start: 32_000,
                end: 48_000,
            },
        ];
        let samples = vec![0.0f32; 48_000];
        let (_filtered, map) = filter_samples(&samples, &spans);
        assert_eq!(remap_ms(1000, &map, TimestampKind::End), 1000);
    }

    #[test]
    fn gap_boundary_start_advances_to_the_next_spans_original_start() {
        // Same boundary as the sibling `_end` test above (filtered sample
        // 16000 = 1000ms), but for a START timestamp: whisper emitting a
        // segment's START exactly here means "speech resumed at span2", so
        // it must resolve to span2's ORIGINAL start (2000ms) — not span1's
        // original end (1000ms), which would render the segment as
        // beginning a whole collapsed silence gap too early (GAP-60, the
        // Codex review finding this test regresses).
        let spans = vec![
            SpeechSpan {
                start: 0,
                end: 16_000,
            },
            SpeechSpan {
                start: 32_000,
                end: 48_000,
            },
        ];
        let samples = vec![0.0f32; 48_000];
        let (_filtered, map) = filter_samples(&samples, &spans);
        assert_eq!(remap_ms(1000, &map, TimestampKind::Start), 2000);
    }

    #[test]
    fn segment_spanning_one_full_span_keeps_start_le_end_across_two_boundary_ties() {
        // Three spans separated by two collapsed 1s gaps: span1 orig
        // [0,16000) (0..1000ms), span2 orig [32000,48000) (2000..3000ms),
        // span3 orig [64000,80000) (4000..5000ms). Filtered layout: span1 ->
        // [0,16000) (0..1000ms), span2 -> [16000,32000) (1000..2000ms),
        // span3 -> [32000,48000) (2000..3000ms). A whisper segment exactly
        // co-incident with span2 — filtered t0=1000ms (the span1/span2
        // boundary) and filtered t1=2000ms (the span2/span3 boundary) — is
        // the realistic cross-boundary case: its START ties the FIRST
        // boundary (must advance to span2's original start, 2000ms) and its
        // END ties the SECOND boundary (must stay at span2's original end,
        // 3000ms). Both remapped bounds must land exactly on span2's own
        // original range, and start must never exceed end.
        let spans = vec![
            SpeechSpan {
                start: 0,
                end: 16_000,
            },
            SpeechSpan {
                start: 32_000,
                end: 48_000,
            },
            SpeechSpan {
                start: 64_000,
                end: 80_000,
            },
        ];
        let samples = vec![0.0f32; 80_000];
        let (_filtered, map) = filter_samples(&samples, &spans);
        let start = remap_ms(1000, &map, TimestampKind::Start);
        let end = remap_ms(2000, &map, TimestampKind::End);
        assert_eq!(start, 2000, "start ties forward onto span2's own start");
        assert_eq!(end, 3000, "end ties backward onto span2's own end");
        assert!(start <= end, "remapped segment must not invert: {start}..{end}");
    }

    #[test]
    fn past_the_last_span_clamps_to_its_original_end() {
        // whisper can emit a segment end at, or a hair past, the filtered
        // buffer's very end (frame-boundary rounding). That must clamp to
        // the last span's original end, not extrapolate past it — for
        // either kind, since there is no "next span" for a Start tie to
        // advance to.
        let spans = vec![SpeechSpan {
            start: 0,
            end: 16_000,
        }];
        let samples = vec![0.0f32; 16_000];
        let (_filtered, map) = filter_samples(&samples, &spans);
        assert_eq!(
            remap_ms(5_000, &map, TimestampKind::End),
            1_000,
            "5s is past the 1s filtered buffer (End)"
        );
        assert_eq!(
            remap_ms(5_000, &map, TimestampKind::Start),
            1_000,
            "5s is past the 1s filtered buffer (Start)"
        );
    }

    #[test]
    fn inverted_and_empty_segments_are_dropped() {
        let spans = spans_from_centiseconds(
            &[
                (50.0, 50.0),  // empty: start == end
                (100.0, 80.0), // inverted: end < start
                (0.0, 10.0),   // valid: 0..1600 samples
            ],
            100_000,
        );
        assert_eq!(
            spans,
            vec![SpeechSpan {
                start: 0,
                end: 1_600
            }]
        );
    }

    #[test]
    fn overlapping_segments_merge_into_one_span() {
        // seg1: 0..100cs (0..16000 samples); seg2: 80..200cs (12800..32000
        // samples) overlaps seg1 by 20cs (3200 samples) — exactly the kind
        // of overlap the C side's speech_pad_ms/samples_overlap can
        // produce.
        let spans = spans_from_centiseconds(&[(0.0, 100.0), (80.0, 200.0)], 100_000);
        assert_eq!(
            spans,
            vec![SpeechSpan {
                start: 0,
                end: 32_000
            }]
        );
    }

    #[test]
    fn spans_are_clamped_to_total_samples() {
        // A segment extending past the decoded buffer's end (VAD frame
        // rounding) must clamp rather than read/copy out of bounds.
        let spans = spans_from_centiseconds(&[(0.0, 1_000.0)], 10_000);
        assert_eq!(
            spans,
            vec![SpeechSpan {
                start: 0,
                end: 10_000
            }]
        );
    }

    #[test]
    fn no_segments_yields_no_spans() {
        assert_eq!(spans_from_centiseconds(&[], 10_000), Vec::new());
    }

    #[test]
    fn empty_spans_yield_an_empty_filtered_buffer() {
        let samples = vec![0.0f32; 1_000];
        let (filtered, map) = filter_samples(&samples, &[]);
        assert!(filtered.is_empty());
        assert!(map.is_empty());
    }

    #[test]
    fn remap_with_an_empty_map_returns_the_input_unchanged() {
        // The safest identity when there is nothing to map through — see
        // remap_ms's doc comment. Defensive: the engine never calls this
        // with an empty map in practice (empty spans short-circuit before
        // whisper runs at all), but the function must still degrade
        // sanely rather than panic or invent a value — for either kind.
        assert_eq!(remap_ms(0, &[], TimestampKind::Start), 0);
        assert_eq!(remap_ms(0, &[], TimestampKind::End), 0);
        assert_eq!(remap_ms(1_234, &[], TimestampKind::Start), 1_234);
        assert_eq!(remap_ms(1_234, &[], TimestampKind::End), 1_234);
    }
}
