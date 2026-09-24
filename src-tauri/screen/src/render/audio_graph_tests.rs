//! Golden strings for the composed audio graph (Task 44).

use super::audio_graph::{build_audio_graph, OUT_LABEL};
use super::test_support::{audio, audio_plan};
use vault_buddy_core::editor::model::FadeCurve;
use vault_buddy_core::editor::model_cues::TransitionKind;
use vault_buddy_core::editor::render_plan::Cut;

fn contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "{needle:?} missing from {haystack}"
    );
}

// atempo's own documented range is 0.5..=2.0 (`ffmpeg -h filter=atempo`):
// values outside it are refused by the filter itself, so a 4x speed must be
// split into a CHAIN whose factors each land inside that range and whose
// product is still 4 -- the brief's own worked example.
#[test]
fn atempo_chain_splits_four_into_two_twos() {
    let mut c = audio(0, 0, 0, 1_000);
    c.speed = 4.0;
    let graph = build_audio_graph(&audio_plan(1_000, vec![c]));
    contains(&graph, "atempo=2,atempo=2");
}

// The other direction: a quarter-speed clip splits into two halvings, each
// still inside atempo's 0.5..=2.0 range.
#[test]
fn quarter_speed_splits_into_two_halves() {
    let mut c = audio(0, 0, 0, 1_000);
    c.speed = 0.25;
    let graph = build_audio_graph(&audio_plan(1_000, vec![c]));
    contains(&graph, "atempo=0.5,atempo=0.5");
}

// `preserve_pitch: false` resamples the signal instead of stretching it --
// a real pitch shift, not a pitch-corrected tempo change.
#[test]
fn pitch_off_uses_asetrate() {
    let mut c = audio(0, 0, 0, 1_000);
    c.speed = 2.0;
    c.preserve_pitch = false;
    let graph = build_audio_graph(&audio_plan(1_000, vec![c]));
    contains(&graph, "asetrate=48000*2,aresample=48000");
    assert!(!graph.contains("atempo"), "{graph}");
}

// F23: the fixed name mapping this render hands ffmpeg's own `afade` --
// `hsin`, never `esin`, for `smooth` (GAP-173's recorded residual).
#[test]
fn fade_curves_map_to_ffmpeg_names() {
    for (curve, name) in [
        (FadeCurve::Linear, "tri"),
        (FadeCurve::Smooth, "hsin"),
        (FadeCurve::EqualPower, "qsin"),
    ] {
        let mut c = audio(0, 0, 0, 2_000);
        c.fade_in = 500;
        c.curve = curve;
        let graph = build_audio_graph(&audio_plan(2_000, vec![c]));
        contains(&graph, &format!("afade=t=in:st=0.000:d=0.500:curve={name}"));
    }
}

// A transition's `equal-power` kind is a genuine AUDIO curve (unlike the
// video side, which dissolves both kinds identically): both crossfade
// halves get ffmpeg's constant-loudness `qsin`, not the linear `tri`.
#[test]
fn equal_power_crossfade_uses_qsin_both_sides() {
    let mut from = audio(0, 0, 0, 2_400);
    from.crossfade_out = Some((TransitionKind::EqualPower, 600));
    let mut to = audio(1, 0, 1_800, 3_000);
    to.crossfade_in = Some((TransitionKind::EqualPower, 600));
    let graph = build_audio_graph(&audio_plan(3_000, vec![from, to]));
    contains(&graph, "acrossfade=d=0.600:c1=qsin:c2=qsin");
}

// MUTATION CHECK (per the brief): flipping `normalize=0` to `normalize=1`
// in the implementation must turn this red -- amix's default `normalize`
// scales every input DOWN as more streams join, which would silently
// quiet a mix instead of the clip-level gains being the only knob.
#[test]
fn amix_does_not_normalise() {
    let graph = build_audio_graph(&audio_plan(1_000, vec![audio(0, 0, 0, 1_000)]));
    contains(&graph, "amix=inputs=1:normalize=0:dropout_transition=0");
}

// `adelay`'s `delays` option takes one value PER CHANNEL, pipe-separated;
// a single value would delay only the first channel and pan the clip's
// entrance across the stereo field for the length of the delay.
#[test]
fn delay_applies_to_both_channels() {
    let graph = build_audio_graph(&audio_plan(3_000, vec![audio(0, 0, 1_500, 3_000)]));
    contains(&graph, "adelay=1500|1500");
}

// A project with no audible clip at all (every track muted, nothing has
// sound) still must produce a real audio stream -- the capture format's
// own contract -- so the container the exporter/player expects an audio
// track in never gets a silently-dropped one.
#[test]
fn silent_project_gets_anullsrc() {
    let graph = build_audio_graph(&audio_plan(5_000, Vec::new()));
    contains(&graph, "anullsrc=r=48000:cl=stereo");
    contains(&graph, "atrim=end=5.000");
    assert!(graph.ends_with(OUT_LABEL), "{graph}");
}

// The whole mix always ends up limited to a fixed sample-peak ceiling and
// resampled/pinned to stereo at the capture's own rate, whether or not any
// clip is speed-changed or crossfaded.
#[test]
fn the_mix_is_limited_and_pinned_to_stereo_48k() {
    let graph = build_audio_graph(&audio_plan(1_000, vec![audio(0, 0, 0, 1_000)]));
    contains(&graph, "alimiter=limit=0.891");
    contains(&graph, "aresample=48000");
    contains(&graph, "channel_layouts=stereo");
}

// A render range that cuts THROUGH a crossfade overlap can leave only one
// half of the pair in the plan (Task 41 plans both halves independently
// per clip, and `place()` drops whichever clip falls entirely outside the
// window). `acrossfade` needs both streams, so the orphaned half falls
// back to a plain edge fade using the transition's own curve and duration
// rather than panicking or silently dropping the envelope.
#[test]
fn an_orphaned_crossfade_half_falls_back_to_a_plain_fade_without_panicking() {
    let mut c = audio(0, 0, 1_800, 3_000);
    c.crossfade_in = Some((TransitionKind::EqualPower, 600));
    let graph = build_audio_graph(&audio_plan(3_000, vec![c]));
    assert!(!graph.contains("acrossfade"), "{graph}");
    contains(&graph, "afade=t=in:st=0.000:d=0.600:curve=qsin");
}

// The orphaned fallback is Cut-aware too: when the range ALSO ate into the
// surviving clip's own head past where the crossfade envelope would have
// finished, there is nothing left of it to fade -- exactly `video_layers::
// alpha`'s guard on the video side.
#[test]
fn an_orphaned_crossfade_wholly_cut_away_emits_no_fade() {
    let mut c = audio(0, 0, 1_800, 3_000);
    c.crossfade_in = Some((TransitionKind::EqualPower, 600));
    c.cut = Cut {
        head_ms: 600,
        tail_ms: 0,
    };
    let graph = build_audio_graph(&audio_plan(3_000, vec![c]));
    assert!(!graph.contains("afade"), "{graph}");
    assert!(!graph.contains("acrossfade"), "{graph}");
}
