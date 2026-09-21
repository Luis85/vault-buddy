//! Read-side time mapping for the tutorial editor's multi-track model
//! (R15, `DATA-MODEL.md` § Timing rules): where a clip's output span sits,
//! what source instant an output instant plays, and the reverse. Every
//! function here is pure arithmetic on plain numbers, never on
//! `model::Clip` directly (`speed` there is a `Num`, this module takes
//! `f64` and the caller converts) -- `project_duration` is the one
//! exception, since it walks the whole `Project` graph and is where that
//! conversion actually happens.
//!
//! Half-open everywhere: a clip's output span is `[start_ms, end_ms)` and
//! its source span is `[in_ms, out_ms)`. Rounding is HALF AWAY FROM ZERO
//! on `f64` (`round_half_away`) -- every value this module rounds is a
//! non-negative time or ratio of the schema's bounded integers
//! (`limits::MAX_DURATION_MS` / `limits::SPEED_MIN`), so the arithmetic
//! stays exact in `f64` and the away-from-zero tie-break matches
//! `f64::round`'s own documented behaviour without a custom helper.
//!
//! This algebra exists twice, in two languages, deliberately: here, and in
//! `src/editor/timeMap.ts` -- the one the preview, drag preview and ruler
//! read from while the user is looking at the timeline (R4). Each has its
//! own tests, and both read the SAME fixture file,
//! `tests/fixtures/editor-time-cases.json` (`include_str!`, the
//! `core::timeline` / `timeline-cases.json` precedent, GAP-136): moving or
//! deleting it breaks the Rust build rather than quietly testing nothing.

use super::model::Project;
use super::validate::{output_duration_ms, speed_or_default};

/// A clip's time-mapping span: output start, half-open source range, and
/// speed as a plain `f64`. Deliberately NOT `model::Clip` -- these
/// functions are pure numeric arithmetic and take `f64`, converting a
/// `Clip`'s `Option<Num>` speed (default 1.0x, `speed_or_default`) is the
/// caller's job, exactly as `project_duration` below does it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipSpan {
    pub start_ms: u64,
    pub in_ms: u64,
    pub out_ms: u64,
    pub speed: f64,
}

/// Round half away from zero. Every input this module ever passes here is
/// non-negative (a duration, an elapsed time, or a ratio of two of them),
/// so `f64::round`'s own "ties round away from zero" behaviour already IS
/// this rule -- no banker's-rounding correction is needed. The `as u64`
/// cast is a Rust saturating cast (never UB, never panics): a negative
/// input saturates to 0 rather than wrapping, which this module treats as
/// defense in depth rather than a reachable path.
fn round_half_away(x: f64) -> u64 {
    x.round() as u64
}

/// `round((out_ms - in_ms) / speed)` (`DATA-MODEL.md` § Timing rules).
/// Reuses `validate::output_duration_ms` -- the same formula the semantic
/// validator already computes to check a clip's `MAX_DURATION_MS` bound --
/// rather than a second copy of the arithmetic.
pub fn clip_output_duration(in_ms: u64, out_ms: u64, speed: f64) -> u64 {
    output_duration_ms(in_ms, out_ms, speed)
}

/// `start_ms + clip_output_duration(in_ms, out_ms, speed)`.
pub fn clip_output_end(clip: &ClipSpan) -> u64 {
    clip.start_ms + clip_output_duration(clip.in_ms, clip.out_ms, clip.speed)
}

/// `start_ms <= t < end_ms` -- half-open, so the end instant itself is not
/// part of the clip's active span.
pub fn clip_is_active(clip: &ClipSpan, t: u64) -> bool {
    t >= clip.start_ms && t < clip_output_end(clip)
}

/// Maps an output instant to the source instant it plays, or `None`
/// outside the clip's active `[start_ms, end_ms)` span. The mapped value
/// is clamped to `[in_ms, out_ms - 1]` -- the source range is half-open, so
/// `out_ms` itself is never a playable source frame, and a speed that
/// rounds the last output instant past it must land on the last real one
/// instead.
pub fn source_at(clip: &ClipSpan, t: u64) -> Option<u64> {
    if !clip_is_active(clip, t) {
        return None;
    }
    let raw = (t - clip.start_ms) as f64 * clip.speed;
    let s = clip.in_ms + round_half_away(raw);
    Some(s.clamp(clip.in_ms, clip.out_ms.saturating_sub(1)))
}

/// Maps a source instant back to the output instant it appears at, for
/// `source` in the clip's half-open `[in_ms, out_ms)` range; `None`
/// outside it.
pub fn output_at(clip: &ClipSpan, source: u64) -> Option<u64> {
    if source < clip.in_ms || source >= clip.out_ms {
        return None;
    }
    let raw = (source - clip.in_ms) as f64 / clip.speed;
    Some(clip.start_ms + round_half_away(raw))
}

/// The output-timeline span a source-time cue `[cue_start_src, cue_end_src)`
/// maps to, once clipped to the clip's own `[in_ms, out_ms)` source range;
/// `None` when the clipped intersection is empty (e.g. a cue that ends
/// exactly at `in_ms`, or starts at/after `out_ms`).
///
/// The start of the clipped span is mapped through `output_at` (it is
/// always strictly inside `[in_ms, out_ms)` once the intersection is
/// non-empty, so `output_at` never actually returns `None` here). The END
/// is mapped by the same raw formula directly rather than through
/// `output_at`, because it may legitimately equal `out_ms` -- a cue
/// straddling the clip's own end clips there, and `output_at` would reject
/// exactly that boundary as out of range.
pub fn cue_output_span(
    clip: &ClipSpan,
    cue_start_src: u64,
    cue_end_src: u64,
) -> Option<(u64, u64)> {
    let start = cue_start_src.max(clip.in_ms);
    let end = cue_end_src.min(clip.out_ms);
    if start >= end {
        return None;
    }
    let out_start = output_at(clip, start)?;
    let out_end = clip.start_ms + round_half_away((end - clip.in_ms) as f64 / clip.speed);
    Some((out_start, out_end))
}

/// The project's overall duration: the latest `clip_output_end` across
/// EVERY clip, on visible tracks and hidden ones alike -- a hidden track
/// still defines how long the render is, it just doesn't paint. `0` for a
/// project with no clips at all.
pub fn project_duration(project: &Project) -> u64 {
    project
        .clips
        .iter()
        .map(|clip| {
            let speed = speed_or_default(clip.speed.as_ref());
            clip.start_ms + clip_output_duration(clip.in_ms, clip.out_ms, speed)
        })
        .max()
        .unwrap_or(0)
}

/// 100-nanosecond Media Foundation frame timestamps from a rational frame
/// rate -- the starter's checked `u128` formula (`implementation-starter/
/// rust/editor_contracts.rs`). Wide intermediate arithmetic so a long
/// render at a high frame count never silently wraps; JSON transport of the
/// result must not narrow it blindly.
pub fn frame_timestamp(
    frame: u64,
    fps_numerator: u64,
    fps_denominator: u64,
) -> Result<u64, &'static str> {
    if fps_numerator == 0 || fps_denominator == 0 {
        return Err("invalid frame rate");
    }
    let ticks = u128::from(frame)
        .checked_mul(10_000_000)
        .and_then(|v| v.checked_mul(u128::from(fps_denominator)))
        .ok_or("timestamp overflow")?
        / u128::from(fps_numerator);
    u64::try_from(ticks).map_err(|_| "timestamp overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- named unit tests -------------------------------------------

    #[test]
    fn frame_timestamps_do_not_accumulate_rounding() {
        // Each timestamp is computed from the wide u128 product directly,
        // never from a running sum of per-frame durations -- a naive
        // `frame * round(10_000_000 * den / num)` would drift over a long
        // render. Values from the starter's own regression test.
        assert_eq!(frame_timestamp(1, 30, 1), Ok(333_333));
        assert_eq!(frame_timestamp(30, 30, 1), Ok(10_000_000));
        assert_eq!(frame_timestamp(30_000, 30_000, 1001), Ok(10_010_000_000));
    }

    #[test]
    fn invalid_or_overflowing_rates_are_rejected() {
        assert!(frame_timestamp(1, 0, 1).is_err(), "zero numerator");
        assert!(frame_timestamp(1, 1, 0).is_err(), "zero denominator");
        assert!(
            frame_timestamp(u64::MAX, 1, u64::MAX).is_err(),
            "the u128 product overflows"
        );
    }

    #[test]
    fn end_is_exclusive() {
        let clip = ClipSpan {
            start_ms: 0,
            in_ms: 0,
            out_ms: 1_000,
            speed: 1.0,
        };
        let end = clip_output_end(&clip);
        assert!(clip_is_active(&clip, end - 1));
        assert!(!clip_is_active(&clip, end), "t == end is not active");
        assert_eq!(source_at(&clip, end), None, "t == end maps to no source");
    }

    #[test]
    fn project_duration_is_zero_with_no_clips() {
        assert_eq!(project_duration(&empty_project()), 0);
    }

    #[test]
    fn project_duration_is_the_max_clip_output_end_including_hidden_tracks() {
        use super::super::model::{AssetKind, Clip, FadeCurve, Track, TrackKind};

        let mut project = empty_project();
        project.tracks.push(track("hidden", false));
        project.tracks.push(track("visible", true));
        project.assets.push(asset());
        // The shorter clip sits on the VISIBLE track; the longer one on the
        // HIDDEN track. If hidden tracks were excluded, project_duration
        // would report the visible clip's shorter end instead.
        project.clips.push(clip("c-short", "visible", 0, 0, 1_000));
        project.clips.push(clip("c-long", "hidden", 0, 0, 5_000));

        assert_eq!(project_duration(&project), 5_000);

        fn track(id: &str, visible: bool) -> Track {
            Track {
                id: id.into(),
                kind: TrackKind::Video,
                name: id.into(),
                visible,
                locked: false,
                muted: false,
                solo: false,
                volume: serde_json::Number::from(1),
                extra: Default::default(),
            }
        }
        fn asset() -> super::super::model::Asset {
            super::super::model::Asset {
                id: "a1".into(),
                kind: AssetKind::Video,
                name: "a1".into(),
                duration_ms: 10_000,
                width: None,
                height: None,
                size: None,
                builtin: None,
                media_type: None,
                linked_asset: None,
                original_name: None,
                extra: Default::default(),
            }
        }
        fn clip(id: &str, track_id: &str, start_ms: u64, in_ms: u64, out_ms: u64) -> Clip {
            Clip {
                id: id.into(),
                asset_id: "a1".into(),
                track_id: track_id.into(),
                name: id.into(),
                start_ms,
                in_ms,
                out_ms,
                fade_in_ms: 0,
                fade_out_ms: 0,
                fade_curve: FadeCurve::Linear,
                opacity: serde_json::Number::from(1),
                volume: serde_json::Number::from(1),
                muted: false,
                x: serde_json::Number::from(0),
                y: serde_json::Number::from(0),
                w: serde_json::Number::from(1),
                h: serde_json::Number::from(1),
                speed: None,
                rotation: None,
                frame_shape: None,
                fit: None,
                mirror: None,
                flip_y: None,
                preserve_pitch: None,
                group_id: None,
                crop_zoom: None,
                crop_x: None,
                crop_y: None,
                adjustments: None,
                card: None,
                extra: Default::default(),
            }
        }
    }

    fn empty_project() -> Project {
        Project {
            schema: super::super::PROJECT_SCHEMA.into(),
            id: "p1".into(),
            title: "t".into(),
            canvas: super::super::model::Canvas {
                width: 1280,
                height: 720,
                fps: 30,
                extra: Default::default(),
            },
            master_gain: 1.0,
            assets: Vec::new(),
            tracks: Vec::new(),
            clips: Vec::new(),
            effects: Vec::new(),
            markers: Vec::new(),
            transitions: Vec::new(),
            captions: None,
            destination: super::super::model::Destination {
                vault: String::new(),
                folder: String::new(),
                dated: false,
                extra: Default::default(),
            },
            extra: Default::default(),
        }
    }

    // ---- the SHARED fixture table -------------------------------------
    //
    // `tests/editorTimeFixtures.test.ts` reads this exact file and asserts
    // the same expectations. `include_str!` is what makes the sharing
    // real: move or delete the fixture and this crate stops compiling,
    // rather than quietly testing nothing (the `timeline-cases.json`
    // precedent, GAP-136).
    const SHARED_FIXTURES: &str = include_str!("../../../../tests/fixtures/editor-time-cases.json");

    fn fixtures() -> serde_json::Value {
        serde_json::from_str(SHARED_FIXTURES).expect("the shared editor-time fixture table is JSON")
    }

    fn clip_span_of(v: &serde_json::Value) -> ClipSpan {
        ClipSpan {
            start_ms: v["start_ms"].as_u64().expect("start_ms"),
            in_ms: v["in_ms"].as_u64().expect("in_ms"),
            out_ms: v["out_ms"].as_u64().expect("out_ms"),
            speed: v["speed"].as_f64().expect("speed"),
        }
    }

    fn as_opt_u64(v: &serde_json::Value) -> Option<u64> {
        if v.is_null() {
            None
        } else {
            Some(v.as_u64().expect("optional u64 row"))
        }
    }

    #[test]
    fn shared_fixture_table_has_ten_cases() {
        let table = fixtures();
        let cases = table["cases"].as_array().expect("cases");
        // A table nothing iterates proves nothing, and one that silently
        // shrinks proves almost nothing. The TypeScript half asserts the
        // same count against the same file.
        assert_eq!(cases.len(), 10, "the shared table lost or gained a case");
    }

    #[test]
    fn shared_fixture_table_source_and_output_mapping_agree() {
        let table = fixtures();
        let cases = table["cases"].as_array().expect("cases");
        assert!(!cases.is_empty(), "the fixture table is empty");
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let span = clip_span_of(&case["clip"]);

            let expected_duration = case["durationMs"].as_u64().expect("durationMs");
            assert_eq!(
                clip_output_duration(span.in_ms, span.out_ms, span.speed),
                expected_duration,
                "durationMs disagrees for {name}"
            );

            for row in case["sourceAt"].as_array().expect("sourceAt") {
                let t = row[0].as_u64().expect("t");
                let expected = as_opt_u64(&row[1]);
                assert_eq!(
                    source_at(&span, t),
                    expected,
                    "source_at({t}) disagrees for {name}"
                );
            }

            for row in case["outputAt"].as_array().expect("outputAt") {
                let source = row[0].as_u64().expect("source");
                let expected = as_opt_u64(&row[1]);
                assert_eq!(
                    output_at(&span, source),
                    expected,
                    "output_at({source}) disagrees for {name}"
                );
            }
        }
    }

    #[test]
    fn shared_fixture_table_cue_spans_agree() {
        let table = fixtures();
        let cases = table["cases"].as_array().expect("cases");
        let mut checked = 0usize;
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let span = clip_span_of(&case["clip"]);
            for row in case["cueSpans"].as_array().expect("cueSpans") {
                let a = row[0].as_u64().expect("cue start");
                let b = row[1].as_u64().expect("cue end");
                let expected = if row[2].is_null() {
                    None
                } else {
                    let pair = row[2].as_array().expect("cue span pair");
                    Some((
                        pair[0].as_u64().expect("span start"),
                        pair[1].as_u64().expect("span end"),
                    ))
                };
                assert_eq!(
                    cue_output_span(&span, a, b),
                    expected,
                    "cue_output_span({a},{b}) disagrees for {name}"
                );
                checked += 1;
            }
        }
        // Vacuity guard: at least the three dedicated cue cases the brief
        // requires (fully inside, straddling out_ms, ending at in_ms) must
        // each contribute a row, or this loop would pass while asserting
        // nothing about cues at all.
        assert!(checked >= 3, "only {checked} cueSpans rows were checked");
    }
}
