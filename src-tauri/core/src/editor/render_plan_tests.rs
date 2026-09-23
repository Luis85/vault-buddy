//! `render_plan`'s tests (Task 41), split out of `render_plan.rs` for the
//! 800-nonblank-line cap. Fixtures are ASYMMETRIC on purpose: distinct
//! start times, non-unit volumes and a non-unit speed, so a swapped field
//! or a forgotten scale fails rather than coinciding.

use std::collections::BTreeMap;

use super::*;
use crate::editor::migrate::{from_staged, StagedInput};
use crate::editor::model::{AssetKind, Builtin, Project, Track, TrackKind};
use crate::editor::model_cues::{CaptionCue, CaptionSettings, Marker};
use crate::editor::test_support::{asset, clip, effect, minimal_project, track};
use crate::editor::{EditorErrorCode, Map, Num};
use crate::timeline::Timeline;

fn num(v: f64) -> Num {
    Num::from_f64(v).expect("finite")
}

fn source(input_index: usize, has_audio: bool) -> PlanSource {
    PlanSource {
        input_index,
        has_video: true,
        has_audio,
        width: 1920,
        height: 1080,
        is_image: false,
    }
}

fn audio_source(input_index: usize) -> PlanSource {
    PlanSource {
        input_index,
        has_video: false,
        has_audio: true,
        width: 0,
        height: 0,
        is_image: false,
    }
}

fn sources(entries: &[(&str, PlanSource)]) -> BTreeMap<String, PlanSource> {
    entries
        .iter()
        .map(|(id, s)| ((*id).to_string(), *s))
        .collect()
}

fn layer_ids(plan: &RenderPlan) -> Vec<&str> {
    plan.video_layers
        .iter()
        .map(|l| l.clip_id.as_str())
        .collect()
}

fn audio_ids(plan: &RenderPlan) -> Vec<&str> {
    plan.audio.iter().map(|a| a.clip_id.as_str()).collect()
}

fn hidden(mut t: Track) -> Track {
    t.visible = false;
    t
}

fn marker(id: &str, clip_id: &str, source_ms: u64, title: &str) -> Marker {
    Marker {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        source_ms,
        title: title.to_string(),
        extra: Map::new(),
    }
}

fn caption(id: &str, clip_id: &str, start_ms: u64, end_ms: u64) -> CaptionCue {
    CaptionCue {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        start_ms,
        end_ms,
        text: format!("text {id}"),
        extra: Map::new(),
    }
}

fn burned_in(cues: Vec<CaptionCue>) -> CaptionSettings {
    CaptionSettings {
        enabled: true,
        burn_in: true,
        font_size: Num::from(29),
        position: CaptionPosition::Bottom,
        background: true,
        cues,
        extra: Map::new(),
    }
}

// ---- compositing order -------------------------------------------------------

/// `tracks[0]` is the TOP layer, so the renderer -- which overlays
/// `video_layers` in Vec order -- must see it LAST. Listing top-first
/// would paint the webcam under the screen.
#[test]
fn upper_tracks_are_composited_last() {
    let mut project = minimal_project();
    project.tracks = vec![
        track("top", TrackKind::Video, false),
        track("middle", TrackKind::Video, false),
        track("bottom", TrackKind::Video, false),
    ];
    project.assets = vec![
        asset("a-top", AssetKind::Video, 10_000),
        asset("a-mid", AssetKind::Video, 10_000),
        asset("a-bot", AssetKind::Video, 10_000),
    ];
    project.clips = vec![
        clip("c-top", "top", "a-top", 300, 0, 4_000),
        clip("c-mid", "middle", "a-mid", 200, 0, 4_000),
        clip("c-bot", "bottom", "a-bot", 100, 0, 4_000),
    ];
    let src = sources(&[
        ("a-top", source(2, false)),
        ("a-mid", source(0, false)),
        ("a-bot", source(1, false)),
    ]);

    let plan = plan(&project, &src, None).expect("plan");

    assert_eq!(
        layer_ids(&plan),
        ["c-bot", "c-mid", "c-top"],
        "bottom -> top"
    );
    let tracks: Vec<usize> = plan.video_layers.iter().map(|l| l.track_index).collect();
    assert_eq!(tracks, [2, 1, 0]);
    let inputs: Vec<usize> = plan.video_layers.iter().map(|l| l.input).collect();
    assert_eq!(inputs, [1, 0, 2], "each layer reads its own source");
}

// ---- hidden tracks ---------------------------------------------------------------

/// F-04: an invisible track is excluded from output -- no picture, and (the
/// preview's rule, `previewLayers.ts`) no sound either. Its source is not
/// even an input, so a hidden track holding an unreachable file does not
/// block the render.
#[test]
fn hidden_tracks_are_excluded() {
    let mut project = minimal_project();
    project.tracks = vec![
        hidden(track("v-hidden", TrackKind::Video, false)),
        track("v-shown", TrackKind::Video, false),
        hidden(track("a-hidden", TrackKind::Audio, false)),
    ];
    project.assets = vec![
        asset("hidden-src", AssetKind::Video, 10_000),
        asset("shown-src", AssetKind::Video, 10_000),
        asset("voice", AssetKind::Audio, 10_000),
    ];
    project.clips = vec![
        clip("c-hidden", "v-hidden", "hidden-src", 0, 0, 5_000),
        clip("c-shown", "v-shown", "shown-src", 500, 0, 5_000),
        clip("c-voice", "a-hidden", "voice", 0, 0, 5_000),
    ];
    let all = sources(&[
        ("hidden-src", source(0, true)),
        ("shown-src", source(1, true)),
        ("voice", audio_source(2)),
    ]);

    let plan = plan(&project, &all, None).expect("plan");
    assert_eq!(layer_ids(&plan), ["c-shown"]);
    assert_eq!(
        audio_ids(&plan),
        ["c-shown"],
        "a hidden track is silent too"
    );
    let inputs: Vec<&str> = plan.inputs.iter().map(|i| i.asset_id.as_str()).collect();
    assert_eq!(inputs, ["shown-src"]);

    let only_shown = sources(&[("shown-src", source(1, true))]);
    assert!(
        super::plan(&project, &only_shown, None).is_ok(),
        "a hidden clip's missing source must not block the render"
    );
}

// ---- solo ----------------------------------------------------------------------

/// `commands::tracks`' solo rule: once ANY track is soloed only soloed
/// tracks are audible, and a soloed track's own mute still wins. Solo is an
/// AUDIO control -- the picture of a non-soloed video track still renders.
#[test]
fn solo_silences_non_soloed_tracks() {
    let mut project = minimal_project();
    project.master_gain = 0.9;
    let mut soloed = track("a-solo", TrackKind::Audio, false);
    soloed.solo = true;
    soloed.volume = num(0.8);
    let mut soloed_muted = track("a-solo-muted", TrackKind::Audio, false);
    soloed_muted.solo = true;
    soloed_muted.muted = true;
    project.tracks = vec![
        track("v1", TrackKind::Video, false),
        soloed,
        track("a-plain", TrackKind::Audio, false),
        soloed_muted,
    ];
    project.assets = vec![
        asset("screen", AssetKind::Video, 10_000),
        asset("voice", AssetKind::Audio, 10_000),
        asset("music", AssetKind::Audio, 10_000),
        asset("noise", AssetKind::Audio, 10_000),
    ];
    let mut voice = clip("c-voice", "a-solo", "voice", 250, 0, 5_000);
    voice.volume = num(0.5);
    project.clips = vec![
        clip("c-screen", "v1", "screen", 0, 0, 5_000),
        voice,
        clip("c-music", "a-plain", "music", 0, 0, 5_000),
        clip("c-noise", "a-solo-muted", "noise", 0, 0, 5_000),
    ];
    let src = sources(&[
        ("screen", source(0, true)),
        ("voice", audio_source(1)),
        ("music", audio_source(2)),
        ("noise", audio_source(3)),
    ]);

    let plan = plan(&project, &src, None).expect("plan");
    assert_eq!(audio_ids(&plan), ["c-voice"]);
    assert_eq!(plan.audio[0].gain, 0.5 * 0.8, "clip x track, master apart");
    assert_eq!(plan.master_gain, 0.9);
    assert_eq!(plan.audio[0].output_start, 250);
    assert_eq!(layer_ids(&plan), ["c-screen"], "solo never hides a picture");

    // Control: with nothing soloed, every unmuted track is heard.
    let mut unsoloed = project.clone();
    for t in &mut unsoloed.tracks {
        t.solo = false;
    }
    let heard = super::plan(&unsoloed, &src, None).expect("plan");
    assert_eq!(audio_ids(&heard), ["c-screen", "c-music", "c-voice"]);
}

// ---- A06 --------------------------------------------------------------------------

/// `plan`'s whole signature: a project, its sources and a range -- and no
/// workspace.
type PlanFn = fn(
    &Project,
    &BTreeMap<String, PlanSource>,
    Option<(u64, u64)>,
) -> Result<RenderPlan, EditorError>;

/// A06: monitoring is not export mute. The COMPILE-level half: `plan` takes
/// no workspace, so `Workspace.monitor_muted` has no way in -- adding such
/// a parameter breaks this binding. The behavioural half: two audible
/// tracks both contribute, and planning twice gives the same plan.
#[test]
fn monitor_mute_cannot_affect_the_plan() {
    let signature: PlanFn = plan;

    let mut project = minimal_project();
    project.tracks = vec![
        track("v1", TrackKind::Video, false),
        track("a1", TrackKind::Audio, false),
    ];
    project.assets = vec![
        asset("screen", AssetKind::Video, 10_000),
        asset("voice", AssetKind::Audio, 10_000),
    ];
    project.clips = vec![
        clip("c-screen", "v1", "screen", 0, 0, 6_000),
        clip("c-voice", "a1", "voice", 1_000, 0, 4_000),
    ];
    let src = sources(&[("screen", source(0, true)), ("voice", audio_source(1))]);

    let first = signature(&project, &src, Some((0, 3_000))).expect("plan");
    let second = signature(&project, &src, Some((0, 3_000))).expect("plan");
    assert_eq!(first, second);
    assert_eq!(audio_ids(&first), ["c-screen", "c-voice"]);
}

// ---- A13 --------------------------------------------------------------------------

/// A13: a range render rebases every timestamp to output zero and clips it,
/// while the project itself stays untrimmed. Chapter at 12 000 -> 2 000; the
/// ones at 5 000 and 25 000 fall outside `[10 000, 20 000)` and are dropped.
#[test]
fn range_rebases_every_timestamp_and_clips_chapters() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 40_000)];
    let mut c1 = clip("c1", "v1", "screen", 0, 3_000, 33_000);
    c1.fade_in_ms = 1_500;
    project.clips = vec![c1];
    // Source = output + 3 000 for this clip.
    project.markers = vec![
        marker("m-after", "c1", 28_000, "After"),
        marker("m-inside", "c1", 15_000, "Inside"),
        marker("m-before", "c1", 8_000, "Before"),
    ];
    project.captions = Some(burned_in(vec![
        caption("q-straddle", "c1", 11_000, 15_000),
        caption("q-inside", "c1", 18_000, 19_500),
        caption("q-after", "c1", 24_000, 25_000),
    ]));
    project.effects = vec![effect("e-tail", "c1", 21_000, 27_000)];
    let src = sources(&[("screen", source(0, true))]);

    let plan = plan(&project, &src, Some((10_000, 20_000))).expect("plan");

    assert_eq!(plan.duration_ms, 10_000);
    assert_eq!(plan.chapters, vec![(2_000, "Inside".to_string())]);

    let layer = &plan.video_layers[0];
    assert_eq!((layer.output_start, layer.output_end), (0, 10_000));
    assert_eq!((layer.source_in, layer.source_out), (13_000, 23_000));
    assert_eq!(
        layer.cut,
        Cut {
            head_ms: 10_000,
            tail_ms: 10_000
        }
    );
    assert_eq!(layer.fade_in, 1_500, "kept, measured from the real edge");

    let audio = &plan.audio[0];
    assert_eq!((audio.output_start, audio.output_end), (0, 10_000));
    assert_eq!((audio.source_in, audio.source_out), (13_000, 23_000));

    let captions = plan.captions.as_ref().expect("burned-in captions");
    let spans: Vec<(&str, u64, u64)> = captions
        .cues
        .iter()
        .map(|q| (q.id.as_str(), q.output_start, q.output_end))
        .collect();
    assert_eq!(
        spans,
        [("q-straddle", 0, 2_000), ("q-inside", 5_000, 6_500)]
    );

    let cue = &plan.cues[0];
    assert_eq!((cue.output_start, cue.output_end), (8_000, 10_000));
    assert_eq!(
        cue.cut,
        Cut {
            head_ms: 0,
            tail_ms: 4_000
        }
    );
}

#[test]
fn a_range_outside_the_project_is_refused() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 10_000)];
    project.clips = vec![clip("c1", "v1", "screen", 0, 0, 10_000)];
    let src = sources(&[("screen", source(0, true))]);
    for range in [(5_000, 5_000), (6_000, 5_000), (5_000, 10_001)] {
        let err = plan(&project, &src, Some(range)).expect_err("refused");
        assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{range:?}");
    }
}

// ---- missing sources ------------------------------------------------------------

/// A needed clip with no source file is an error naming the record to
/// reconnect -- never a silent black layer. A detached audio clip names its
/// ROOT (its bytes are the root's); a card needs no file; a hidden clip is
/// not needed.
#[test]
fn missing_source_is_reported_not_blacked_out() {
    let mut project = minimal_project();
    project.tracks = vec![
        track("v1", TrackKind::Video, false),
        hidden(track("v-hidden", TrackKind::Video, false)),
        track("a1", TrackKind::Audio, false),
    ];
    let mut card = asset("card", AssetKind::Video, 7_200_000);
    card.builtin = Some(Builtin::Card);
    let mut gone_audio = asset("gone-audio", AssetKind::Audio, 20_000);
    gone_audio.linked_asset = Some("gone".to_string());
    project.assets = vec![
        asset("here", AssetKind::Video, 20_000),
        asset("gone", AssetKind::Video, 20_000),
        gone_audio,
        asset("zz-hidden-gone", AssetKind::Video, 20_000),
        card,
    ];
    let mut muted_original = clip("c-gone", "v1", "gone", 5_000, 0, 4_000);
    muted_original.muted = true;
    project.clips = vec![
        clip("c-here", "v1", "here", 0, 0, 5_000),
        muted_original,
        clip("c-gone-audio", "a1", "gone-audio", 5_000, 0, 4_000),
        clip("c-hidden", "v-hidden", "zz-hidden-gone", 0, 0, 5_000),
        clip("c-card", "v1", "card", 9_000, 0, 2_000),
    ];
    let src = sources(&[("here", source(0, true))]);

    let err = plan(&project, &src, None).expect_err("a missing source fails the plan");
    assert_eq!(err.code, EditorErrorCode::SourceMissing);
    assert_eq!(err.retained_asset_ids, Some(vec!["gone".to_string()]));
}

// ---- identity (R1, F5) ------------------------------------------------------------

const TIMELINE_CASES: &str = include_str!("../../../../tests/fixtures/timeline-cases.json");

/// Four capture sizes, each EXACT-aspect for one of the four canvases.
const EXACT_DIMS: [(u32, u32); 4] = [(1920, 1080), (1080, 1920), (1000, 1000), (1024, 768)];

fn migrated(segments: &serde_json::Value, duration_ms: u64, dims: (u32, u32)) -> Project {
    let legacy = serde_json::json!({ "segments": segments });
    let input = StagedInput {
        base: "base",
        vault_id: "vault-1",
        source_title: "Capture",
        duration_ms,
        width: dims.0,
        height: dims.1,
        has_audio: true,
        legacy_timeline: Some(&legacy),
        stems: &[],
        webcam: None,
    };
    from_staged(&input, "proj-1").project
}

fn capture_source(dims: (u32, u32)) -> BTreeMap<String, PlanSource> {
    sources(&[(
        "src",
        PlanSource {
            input_index: 0,
            has_video: true,
            has_audio: true,
            width: dims.0,
            height: dims.1,
            is_image: false,
        },
    )])
}

/// R1's successor of `Timeline::is_untouched`: for every shared timeline
/// case migrated through Task 4 (audio present, exact-aspect geometry --
/// the two facts that would otherwise refuse identity for reasons this
/// test is not about), the plan's identity answer equals the legacy
/// predicate at every declared source duration.
#[test]
fn identity_detection_matches_is_untouched() {
    let table: serde_json::Value = serde_json::from_str(TIMELINE_CASES).expect("json");
    let (mut rows, mut identities) = (0usize, 0usize);
    for (n, case) in table["cases"].as_array().expect("cases").iter().enumerate() {
        let name = case["name"].as_str().expect("name");
        for row in case["isUntouched"].as_array().expect("isUntouched") {
            let duration_ms = row[0].as_u64().expect("duration");
            let dims = EXACT_DIMS[(n + rows) % EXACT_DIMS.len()];
            let project = migrated(&case["segments"], duration_ms, dims);
            let legacy = Timeline::from_sidecar_value(
                &serde_json::json!({ "segments": case["segments"] }),
                duration_ms,
            );
            let plan = plan(&project, &capture_source(dims), None).expect("plan");
            assert_eq!(
                plan.is_identity(),
                legacy.is_untouched(duration_ms),
                "case {name:?} at {duration_ms} ms ({dims:?})"
            );
            rows += 1;
            identities += usize::from(plan.is_identity());
        }
    }
    assert!(rows >= 12, "only {rows} rows were cross-checked");
    assert_eq!(identities, 2, "exactly two rows are untouched");
}

/// The GAP-136 trap: one segment that covers the source's LENGTH but starts
/// late is not untouched -- a fast path keyed on the segment count or the
/// end alone would remux the footage the user cut from the front.
#[test]
fn trimmed_front_is_not_identity() {
    let dims = (1920, 1080);
    let trimmed = serde_json::json!([{ "sourceStartMs": 1_000, "sourceEndMs": 6_000 }]);
    let project = migrated(&trimmed, 6_000, dims);
    let plan = plan(&project, &capture_source(dims), None).expect("plan");
    assert!(!plan.is_identity());

    // Control: the same capture uncut IS identity, so the refusal above is
    // the front trim's alone.
    let whole = serde_json::json!([{ "sourceStartMs": 0, "sourceEndMs": 6_000 }]);
    let project = migrated(&whole, 6_000, dims);
    assert!(super::plan(&project, &capture_source(dims), None)
        .expect("plan")
        .is_identity());
}

// ---- speed ----------------------------------------------------------------------

/// At 2x a clip's output span halves, and a clip-linked cue -- SOURCE time --
/// follows through `time::cue_output_span` without being touched.
#[test]
fn speed_two_halves_the_layer_and_its_cues() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 20_000)];
    let mut fast = clip("c-fast", "v1", "screen", 1_000, 2_000, 10_000);
    fast.speed = Some(num(2.0));
    fast.preserve_pitch = Some(false);
    project.clips = vec![fast];
    project.effects = vec![effect("e1", "c-fast", 4_000, 8_000)];
    let src = sources(&[("screen", source(0, true))]);

    let plan = plan(&project, &src, None).expect("plan");
    assert_eq!(layer_ids(&plan), ["c-fast"]);
    let layer = &plan.video_layers[0];
    assert_eq!((layer.output_start, layer.output_end), (1_000, 5_000));
    assert_eq!((layer.source_in, layer.source_out), (2_000, 10_000));
    assert_eq!(layer.speed, 2.0);
    assert_eq!(plan.duration_ms, 5_000);

    let cue = &plan.cues[0];
    assert_eq!((cue.output_start, cue.output_end), (2_000, 4_000));

    let audio = &plan.audio[0];
    assert_eq!((audio.output_start, audio.output_end), (1_000, 5_000));
    assert_eq!(audio.speed, 2.0);
    assert!(!audio.preserve_pitch);
    assert!(!plan.is_identity());
}

// ---- sources, cards, cues, geometry ----------------------------------------------

/// A detached audio clip has no record of its own: its bytes are its
/// video's (`media_commands::resolve_asset`), so its contribution reads the
/// ROOT's input -- and the muted original adds nothing, so the sound is
/// never counted twice.
#[test]
fn detached_audio_reads_its_linked_source() {
    let mut project = minimal_project();
    project.tracks = vec![
        track("v1", TrackKind::Video, false),
        track("a1", TrackKind::Audio, false),
    ];
    let mut detached = asset("screen-audio", AssetKind::Audio, 9_000);
    detached.linked_asset = Some("screen".to_string());
    project.assets = vec![asset("screen", AssetKind::Video, 9_000), detached];
    let mut original = clip("c-video", "v1", "screen", 0, 0, 9_000);
    original.muted = true;
    project.clips = vec![original, clip("c-audio", "a1", "screen-audio", 0, 0, 9_000)];
    let src = sources(&[("screen", source(4, true))]);

    let plan = plan(&project, &src, None).expect("plan");
    assert_eq!(audio_ids(&plan), ["c-audio"]);
    assert_eq!(plan.audio[0].input, 4);
    assert_eq!(layer_ids(&plan), ["c-video"]);
    let inputs: Vec<&str> = plan.inputs.iter().map(|i| i.asset_id.as_str()).collect();
    assert_eq!(inputs, ["screen"], "one record, read by both");
    assert!(
        !plan.is_identity(),
        "a muted picture plus separate audio is an edit"
    );
}

/// A title card is synthesized: it needs no source, is not a video layer,
/// and keeps its track position so the renderer can stack it. A migrated
/// capture's `builtin: screen` is NOT synthesized -- it is a real file
/// (GAP-175) and plans as an ordinary input.
#[test]
fn cards_are_planned_apart_and_screen_builtins_are_inputs() {
    let mut project = minimal_project();
    project.tracks = vec![
        track("titles", TrackKind::Video, false),
        track("v1", TrackKind::Video, false),
    ];
    let mut card_asset = asset("card", AssetKind::Video, 7_200_000);
    card_asset.builtin = Some(Builtin::Card);
    let mut screen = asset("src", AssetKind::Video, 8_000);
    screen.builtin = Some(Builtin::Screen);
    project.assets = vec![card_asset, screen];
    project.clips = vec![
        clip("c-card", "titles", "card", 1_000, 0, 2_000),
        clip("c-screen", "v1", "src", 0, 0, 8_000),
    ];
    let src = sources(&[("src", source(0, false))]);

    let plan = plan(&project, &src, None).expect("plan");
    assert_eq!(layer_ids(&plan), ["c-screen"]);
    assert_eq!(plan.cards.len(), 1);
    let card = &plan.cards[0];
    assert_eq!(
        (
            card.clip_id.as_str(),
            card.track_index,
            card.output_start,
            card.output_end
        ),
        ("c-card", 0, 1_000, 3_000)
    );

    let err = super::plan(&project, &BTreeMap::new(), None).expect_err("screen needs its file");
    assert_eq!(err.retained_asset_ids, Some(vec!["src".to_string()]));
}

/// A clip's box is canvas pixels, each value the nearest EVEN integer, on a
/// non-square canvas so a swapped axis fails.
#[test]
fn layer_boxes_are_even_canvas_pixels() {
    let mut project = minimal_project();
    project.canvas.width = 960;
    project.canvas.height = 720;
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("cam", AssetKind::Video, 5_000)];
    let mut pip = clip("c-pip", "v1", "cam", 0, 0, 5_000);
    pip.x = num(0.6);
    pip.y = num(0.05);
    pip.w = num(0.3);
    pip.h = num(0.25);
    project.clips = vec![pip];
    let src = sources(&[("cam", source(0, false))]);

    let plan = plan(&project, &src, None).expect("plan");
    let layer = &plan.video_layers[0];
    // 0.6*960 = 576, 0.05*720 = 36, 0.3*960 = 288, 0.25*720 = 180.
    assert_eq!(
        layer.bounds,
        PixelBox {
            x: 576,
            y: 36,
            w: 288,
            h: 180
        }
    );
    assert_eq!(
        layer.frame_shape,
        FrameShape::Rounded,
        "a PiP box defaults to rounded"
    );
}

/// A zoom's ramp is capped at half its WHOLE mapped span, so a range that
/// clips the cue does not shorten the ramp (the preview's `zoomAmount`).
#[test]
fn a_zoom_keeps_its_ramp_through_a_range() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 20_000)];
    project.clips = vec![clip("c1", "v1", "screen", 0, 0, 20_000)];
    let mut zoom = effect("z1", "c1", 4_000, 6_000);
    zoom.kind = EffectKind::Zoom;
    let mut slow = effect("z2", "c1", 8_000, 18_000);
    slow.kind = EffectKind::Zoom;
    slow.easing = Some(Num::from(900));
    project.effects = vec![zoom, slow, effect("h1", "c1", 1_000, 2_000)];
    let src = sources(&[("screen", source(0, true))]);

    let whole = plan(&project, &src, None).expect("plan");
    let eases: Vec<Option<f64>> = whole.cues.iter().map(PlannedCue::zoom_ease_ms).collect();
    assert_eq!(
        eases,
        [Some(600.0), Some(900.0), None],
        "600 default; own easing; not a zoom"
    );

    let ranged = super::plan(&project, &src, Some((5_000, 9_000))).expect("plan");
    let short = &ranged.cues[0];
    assert_eq!((short.output_start, short.output_end), (0, 1_000));
    assert_eq!(
        short.zoom_ease_ms(),
        Some(600.0),
        "half of 2 000, not of 1 000"
    );
}

/// F5: a source is identity-eligible against the canvas the PROJECT uses,
/// not just any exact one -- a 16:9 capture on a square canvas must be
/// re-encoded, or the remux would ignore the user's canvas.
#[test]
fn identity_needs_the_project_canvas_to_be_the_sources_aspect() {
    let dims = (1920, 1080);
    let whole = serde_json::json!([{ "sourceStartMs": 0, "sourceEndMs": 6_000 }]);
    let mut project = migrated(&whole, 6_000, dims);
    assert!(super::plan(&project, &capture_source(dims), None)
        .expect("plan")
        .is_identity());
    project.canvas.width = 720;
    project.canvas.height = 720;
    assert!(!super::plan(&project, &capture_source(dims), None)
        .expect("plan")
        .is_identity());
}

/// Captions burn in only when enabled AND set to burn in -- a caption track
/// the user keeps as a separate file (burn-in off), or has switched off,
/// must not be painted into the picture.
#[test]
fn captions_burn_in_only_when_enabled_and_burned_in() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 8_000)];
    project.clips = vec![clip("c1", "v1", "screen", 0, 0, 8_000)];
    let src = sources(&[("screen", source(0, true))]);
    let cues = vec![caption("q1", "c1", 1_000, 3_000)];

    for (enabled, burn_in, expected) in [(true, true, 1), (true, false, 0), (false, true, 0)] {
        let mut settings = burned_in(cues.clone());
        settings.enabled = enabled;
        settings.burn_in = burn_in;
        project.captions = Some(settings);
        let plan = plan(&project, &src, None).expect("plan");
        let planned = plan.captions.as_ref().map_or(0, |c| c.cues.len());
        assert_eq!(planned, expected, "enabled {enabled}, burn_in {burn_in}");
    }
}

/// A clip whose track or asset does not resolve is a malformed graph --
/// `invalidProject`, never a clip quietly dropped from the render.
#[test]
fn an_unresolvable_clip_is_an_invalid_project() {
    let mut project = minimal_project();
    project.tracks = vec![track("v1", TrackKind::Video, false)];
    project.assets = vec![asset("screen", AssetKind::Video, 8_000)];
    let src = sources(&[("screen", source(0, true))]);
    for broken in [
        clip("c-no-track", "ghost-track", "screen", 0, 0, 8_000),
        clip("c-no-asset", "v1", "ghost-asset", 0, 0, 8_000),
    ] {
        project.clips = vec![broken];
        let err = plan(&project, &src, None).expect_err("refused");
        assert_eq!(err.code, EditorErrorCode::InvalidProject, "{}", err.message);
    }
}
