//! `render_plan`'s transition, geometry and range-edge tests (Task 41 fix
//! round 1), a sibling of `render_plan_tests.rs` for the 800-nonblank-line
//! cap; the fixture builders are that module's.

use super::tests::{burned_in, caption, hidden, layer_ids, marker, num, source, sources};
use super::*;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::model_cues::Transition;
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::Map;

fn transition(from: &str, to: &str, duration_ms: u64, kind: TransitionKind) -> Transition {
    Transition {
        id: format!("t-{from}-{to}"),
        from: from.to_string(),
        to: to.to_string(),
        duration_ms,
        kind,
        extra: Map::new(),
    }
}

/// A transition is an OVERLAP with two halves: the `to` clip fades in and
/// the `from` clip fades out across it. Without the outgoing half the
/// renderer plays `from` at full level under `to`'s fade-in (the mix gets
/// LOUDER across an equal-power crossfade) and could find the pair only by
/// inference, which a range cut inside the overlap breaks.
#[test]
fn transitions_carry_both_halves_through_a_range() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![
        asset("a-from", AssetKind::Video, 9_000),
        asset("a-to", AssetKind::Video, 9_000),
    ];
    // `from` ends 1 000 ms after `to` starts: the transition's geometry.
    project.clips = vec![
        clip("c-from", "v1", "a-from", 0, 0, 5_000),
        clip("c-to", "v1", "a-to", 4_000, 1_000, 5_000),
    ];
    project.transitions = vec![transition(
        "c-from",
        "c-to",
        1_000,
        TransitionKind::EqualPower,
    )];
    let src = sources(&[("a-from", source(0, true)), ("a-to", source(1, true))]);
    let pair = Some((TransitionKind::EqualPower, 1_000));

    let whole = plan(&project, &src, None).expect("plan");
    assert_eq!(layer_ids(&whole), ["c-from", "c-to"]);
    let (from, to) = (&whole.video_layers[0], &whole.video_layers[1]);
    assert_eq!((from.transition_in, from.transition_out), (None, pair));
    assert_eq!((to.transition_in, to.transition_out), (pair, None));
    let (from_a, to_a) = (&whole.audio[0], &whole.audio[1]);
    assert_eq!(
        (from_a.clip_id.as_str(), to_a.clip_id.as_str()),
        ("c-from", "c-to")
    );
    assert_eq!((from_a.crossfade_in, from_a.crossfade_out), (None, pair));
    assert_eq!((to_a.crossfade_in, to_a.crossfade_out), (pair, None));

    // A range starting INSIDE the overlap (4 500) keeps both halves, rebased,
    // each with the cut that says how much of it already elapsed.
    let ranged = super::plan(&project, &src, Some((4_500, 7_000))).expect("plan");
    let (from, to) = (&ranged.video_layers[0], &ranged.video_layers[1]);
    assert_eq!((from.output_start, from.output_end), (0, 500));
    assert_eq!((from.source_in, from.source_out), (4_500, 5_000));
    assert_eq!(
        from.cut,
        Cut {
            head_ms: 4_500,
            tail_ms: 0
        }
    );
    assert_eq!(from.transition_out, pair);
    assert_eq!((to.output_start, to.output_end), (0, 2_500));
    assert_eq!((to.source_in, to.source_out), (1_500, 4_000));
    assert_eq!(
        to.cut,
        Cut {
            head_ms: 500,
            tail_ms: 1_000
        }
    );
    assert_eq!(to.transition_in, pair);
    let halves: Vec<_> = ranged
        .audio
        .iter()
        .map(|a| (a.crossfade_in, a.crossfade_out))
        .collect();
    assert_eq!(halves, [(None, pair), (pair, None)]);
}

/// Even rounding must round the EDGES, not the origin and the size apart:
/// y 0.3375 and h 0.6625 on 720 are 243 and 477, both odd ties, which
/// rounded separately give 244 + 478 = a bottom edge at 722 -- 2 px past a
/// box that ends exactly at the canvas edge.
#[test]
fn an_edge_aligned_box_never_leaves_the_canvas() {
    let mut project = minimal_project();
    project.canvas.width = 960;
    project.canvas.height = 720;
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("cam", AssetKind::Video, 5_000)];
    let mut low = clip("c-low", "v1", "cam", 0, 0, 5_000);
    low.y = num(0.3375);
    low.h = num(0.6625);
    project.clips = vec![low];
    let src = sources(&[("cam", source(0, false))]);

    let plan = plan(&project, &src, None).expect("plan");
    let b = plan.video_layers[0].bounds;
    assert!(
        b.y + b.h <= 720 && b.x + b.w <= 960,
        "{b:?} leaves the canvas"
    );
    assert_eq!(
        b,
        PixelBox {
            x: 0,
            y: 244,
            w: 960,
            h: 476
        }
    );
}

/// A range over a NON-UNIT speed clip moves each cut source edge by the cut
/// OUTPUT time x speed: at 2x, 1 000 output ms cut from the head is 2 000
/// source ms.
#[test]
fn a_range_over_a_fast_clip_moves_the_source_by_speed() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 20_000)];
    let mut fast = clip("c-fast", "v1", "screen", 1_000, 2_000, 10_000);
    fast.speed = Some(num(2.0));
    project.clips = vec![fast];
    let src = sources(&[("screen", source(0, true))]);

    let plan = plan(&project, &src, Some((2_000, 4_500))).expect("plan");
    let layer = &plan.video_layers[0];
    assert_eq!((layer.output_start, layer.output_end), (0, 2_500));
    assert_eq!((layer.source_in, layer.source_out), (4_000, 9_000));
    let audio = &plan.audio[0];
    assert_eq!((audio.source_in, audio.source_out), (4_000, 9_000));
}

/// A fade-out whose end the range cuts off keeps its length, measured from
/// the clip's REAL end, and the cut says how far past the range that is --
/// so the renderer evaluates the envelope where it really is instead of
/// restarting the whole fade before the new end.
#[test]
fn a_fade_out_cut_by_a_range_keeps_its_real_edge() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 10_000)];
    let mut fading = clip("c1", "v1", "screen", 0, 0, 10_000);
    fading.fade_out_ms = 2_000;
    fading.fade_curve = FadeCurve::EqualPower;
    project.clips = vec![fading];
    let src = sources(&[("screen", source(0, true))]);

    let plan = plan(&project, &src, Some((3_000, 9_000))).expect("plan");
    let layer = &plan.video_layers[0];
    assert_eq!((layer.output_start, layer.output_end), (0, 6_000));
    assert_eq!((layer.fade_out, layer.cut.tail_ms), (2_000, 1_000));
    let audio = &plan.audio[0];
    assert_eq!((audio.fade_out, audio.cut.tail_ms), (2_000, 1_000));
    assert_eq!(audio.curve, FadeCurve::EqualPower);
}

/// Chapters and burned-in captions follow what their lists show
/// (`captionRules.ts`' `chapterRows` and `captionRows`, and the preview's
/// caption overlay): a cue inside its clip's range, WHATEVER the track's
/// visibility -- one rule for both, stated in the module doc.
#[test]
fn chapters_and_captions_ignore_track_visibility() {
    let mut project = minimal_project();
    project.tracks = vec![
        track("v1", TrackKind::Video, false),
        hidden(track("v-hidden", TrackKind::Video, false)),
    ];
    project.assets = vec![
        asset("screen", AssetKind::Video, 10_000),
        asset("aside", AssetKind::Video, 10_000),
    ];
    project.clips = vec![
        clip("c1", "v1", "screen", 0, 0, 10_000),
        clip("c-hidden", "v-hidden", "aside", 2_000, 0, 6_000),
    ];
    project.markers = vec![marker("m-hidden", "c-hidden", 1_000, "Aside")];
    project.captions = Some(burned_in(vec![caption("q-hidden", "c-hidden", 500, 1_500)]));
    let src = sources(&[("screen", source(0, true))]);

    let plan = plan(&project, &src, None).expect("plan");
    assert_eq!(plan.chapters, vec![(3_000, "Aside".to_string())]);
    let captions = plan.captions.expect("captions");
    let q = &captions.cues[0];
    assert_eq!((q.output_start, q.output_end), (2_500, 3_500));
}
