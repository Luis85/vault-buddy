//! The editor render's END-TO-END round trips (tutorial-editor Task 45;
//! F-04, F-05, F-17--F-19, F-27--F-33, F-41, F-42): a real `Project`,
//! frozen by `core::editor::render_plan::plan`, rendered by
//! `screen::render::run::render` -- refusal, ASS documents in the job dir,
//! the argv, the runner and the output verification all together -- and
//! the result DECODED back: pixels read out of real frames and band
//! energies measured on the real audio stream.
//!
//! `render_graph_roundtrip.rs` drives hand-built plans through
//! `render_args`; this file is the one place a project the editor could
//! actually save goes the whole way. Each assertion reads a value that is
//! different if the thing it names is wrong: a lower layer drawn on top
//! is not red at the box, a hidden track that still renders IS red there,
//! a dropped source leaves its band silent, a reorder that plays in source
//! order is blue where it should be green.
//!
//! Inputs are synthesized with lavfi (NATIVE-MEDIA § fixtures): A is
//! `testsrc2` 1280x720 with a 440 Hz tone, B a solid red 640x360, C a
//! 1000 Hz tone with no picture, D two seconds of blue then two of green,
//! E plain black -- all 4 s at 30 fps.
//!
//! Skips VISIBLY without ffmpeg/ffprobe (the `export_roundtrip.rs` rule):
//! a silent skip is indistinguishable from a pass. CI's `rust-core` job
//! installs ffmpeg so these run there. The tools, the synthesized inputs
//! and the decoders live in `render_support/` (the 800-line cap).

mod render_support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use render_support::*;
use serde_json::{json, Value};
use vault_buddy_core::editor::model::Project;
use vault_buddy_core::editor::render_plan::{plan, PlanSource, RenderPlan};
use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_screen::ffmpeg_args::EncodeSettings;
use vault_buddy_screen::render::run::{render, RenderOutcome, RenderRequestNative};
use vault_buddy_screen::ScreenError;

// ---------------------------------------------------------------- projects

fn video_source(input_index: usize, width: u32, height: u32, has_audio: bool) -> PlanSource {
    PlanSource {
        input_index,
        has_video: true,
        has_audio,
        width,
        height,
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

fn asset(id: &str, kind: &str) -> Value {
    json!({ "id": id, "kind": kind, "name": id, "duration_ms": INPUT_MS })
}

fn track(id: &str, kind: &str) -> Value {
    json!({
        "id": id, "kind": kind, "name": id, "visible": true, "locked": false,
        "muted": false, "solo": false, "volume": 1,
    })
}

/// A full-frame, unfaded 1x clip of `asset` on `track`.
fn clip(id: &str, asset: &str, track: &str, start: u64, in_ms: u64, out_ms: u64) -> Value {
    json!({
        "id": id, "asset_id": asset, "track_id": track, "name": id,
        "start_ms": start, "in_ms": in_ms, "out_ms": out_ms,
        "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
        "opacity": 1, "volume": 1, "muted": false,
        "x": 0, "y": 0, "w": 1, "h": 1,
    })
}

/// A 1280x720 project in document spelling; tracks[0] is the TOP layer.
fn project(assets: Vec<Value>, tracks: Vec<Value>, clips: Vec<Value>) -> Value {
    json!({
        "schema": "vault-buddy-video-project/3",
        "id": "roundtrip",
        "title": "Round trip",
        "canvas": { "width": W, "height": H, "fps": FPS },
        "master_gain": 1,
        "assets": assets,
        "tracks": tracks,
        "clips": clips,
        "effects": [],
        "markers": [],
        "transitions": [],
        "destination": { "vault": "", "folder": "", "dated": false },
    })
}

/// A rendered project: its plan, the render's own outcome and the file.
struct Rendered {
    plan: RenderPlan,
    outcome: RenderOutcome,
    path: PathBuf,
}

/// Plans `doc` against `sources` (record id -> facts + file) over `range`
/// and renders it through `render::run::render`.
fn render_project(
    fx: &Fixture,
    doc: Value,
    sources: &[(&str, PlanSource, &Path)],
    range: Option<(u64, u64)>,
) -> Rendered {
    let (plan, result, path) = try_render(fx, doc, sources, range, &fx.ffprobe);
    let outcome = result.unwrap_or_else(|e| panic!("the render failed: {e}"));
    Rendered {
        plan,
        outcome,
        path,
    }
}

/// `render_project` with an explicit ffprobe, returning the raw result.
fn try_render(
    fx: &Fixture,
    doc: Value,
    sources: &[(&str, PlanSource, &Path)],
    range: Option<(u64, u64)>,
    ffprobe: &Path,
) -> (RenderPlan, Result<RenderOutcome, ScreenError>, PathBuf) {
    let project: Project = serde_json::from_value(doc).expect("a valid project document");
    let facts: BTreeMap<String, PlanSource> = sources
        .iter()
        .map(|(id, s, _)| ((*id).to_string(), *s))
        .collect();
    let plan = plan(&project, &facts, range).expect("the project plans");
    let mut files: Vec<(usize, PathBuf)> = sources
        .iter()
        .map(|(_, s, p)| (s.input_index, p.to_path_buf()))
        .collect();
    files.sort_by_key(|(i, _)| *i);
    let inputs: Vec<PathBuf> = files.into_iter().map(|(_, p)| p).collect();
    let dest = fx.dir.path().join("render.mp4.part");
    let settings = EncodeSettings {
        width: W,
        height: H,
        fps: FPS,
        quality: ScreenQuality::High,
        h264_encoder: "libx264".into(),
        has_audio: true,
    };
    let request = RenderRequestNative {
        ffmpeg: &fx.ffmpeg,
        ffprobe,
        plan: &plan,
        inputs: &inputs,
        job_dir: fx.dir.path(),
        dest: &dest,
        fontsdir: None,
        caps: &fx.caps,
        settings,
    };
    let result = render(request, &AtomicBool::new(false), &mut |_| {});
    (plan, result, dest)
}

// ---------------------------------------------------------------- tests

/// A over its whole length on the LOWER track, B as a 0.25 x 0.25 box at
/// (0.7, 0.05) on the UPPER one.
fn layered(visible_b: bool) -> Value {
    let mut b = clip("cb", "b", "top", 0, 0, INPUT_MS);
    b["x"] = json!(0.7);
    b["y"] = json!(0.05);
    b["w"] = json!(0.25);
    b["h"] = json!(0.25);
    let mut top = track("top", "video");
    top["visible"] = json!(visible_b);
    project(
        vec![asset("a", "video"), asset("b", "video")],
        vec![top, track("bottom", "video")],
        vec![clip("ca", "a", "bottom", 0, 0, INPUT_MS), b],
    )
}

// F-17/F-04: `Project.tracks[0]` is the top layer. Composited in the wrong
// order, A's test pattern (cyan at that point) paints over the red box and
// (1100, 90) is not red. The box is a box, so a point well outside it is
// A's own pixel. NOT the brief's (100, 600): `testsrc2` is itself RED
// there (measured: (253, 0, 0) at 1 s), so "not red" at (100, 600) fails
// on a correct render. (300, 650) is green in A and far from the box.
#[test]
fn upper_layer_covers_lower_layer() {
    let fx = fixture_or_skip!();
    let (a, b) = (input_a(&fx), input_b(&fx));
    let out = render_project(
        &fx,
        layered(true),
        &[
            ("a", video_source(0, W, H, true), &a),
            ("b", video_source(1, 640, 360, false), &b),
        ],
        None,
    );
    let frame = canvas_frame(&fx, &out.path, 1_000);
    let boxed = pixel(&frame, 1_100, 90);
    assert!(
        is_red(boxed),
        "the upper layer's box at (1100,90) is {boxed:?}, want red"
    );
    let outside = pixel(&frame, 300, 650);
    let base = pixel(&frame_at(&fx, &a, 1_000, (W, H)), 300, 650);
    assert!(
        !is_red(base),
        "the fixture flaw: A itself is red at (300,650)"
    );
    assert!(
        !is_red(outside) && near(outside, base),
        "(300,650) is outside the box: {outside:?}, want A's own {base:?}"
    );
}

fn near(p: Rgb, q: Rgb) -> bool {
    (p.0 - q.0).abs() <= 40 && (p.1 - q.1).abs() <= 40 && (p.2 - q.2).abs() <= 40
}

// F-04: an invisible track is excluded from output. Rendered anyway, the
// box would still be red.
#[test]
fn hidden_track_is_absent() {
    let fx = fixture_or_skip!();
    let (a, b) = (input_a(&fx), input_b(&fx));
    let out = render_project(
        &fx,
        layered(false),
        &[
            ("a", video_source(0, W, H, true), &a),
            ("b", video_source(1, 640, 360, false), &b),
        ],
        None,
    );
    assert_eq!(out.plan.inputs.len(), 1, "the hidden track reads no input");
    // A's own pixel, not merely "not red": an all-black render (A lost
    // along with B) is not red either (fix round 1).
    let boxed = pixel(&canvas_frame(&fx, &out.path, 1_000), 1_100, 90);
    let base = pixel(&frame_at(&fx, &a, 1_000, (W, H)), 1_100, 90);
    assert!(
        !is_red(base),
        "the fixture flaw: A itself is red at (1100,90)"
    );
    assert!(
        near(boxed, base),
        "the hidden layer's box: (1100,90) is {boxed:?}, want A's own {base:?}"
    );
}

// F-18/F-41: every audible source reaches the mix. A carries 440 Hz and C
// 1000 Hz; both bands must be loud in the render. The control reads A's
// OWN file at 1000 Hz first, so the threshold is shown to separate a band
// that is there from one that is not.
#[test]
fn two_audible_sources_both_present() {
    let fx = fixture_or_skip!();
    let (a, c) = (input_a(&fx), input_c(&fx));
    let control = rms_db(&fx, &a, 0, INPUT_MS, Some(1_000));
    assert!(
        control < BAND_PRESENT_DB,
        "the fixture flaw: A alone already measures {control} dB at 1000 Hz"
    );
    let doc = project(
        vec![asset("a", "video"), asset("c", "audio")],
        vec![track("v", "video"), track("sound", "audio")],
        vec![
            clip("ca", "a", "v", 0, 0, INPUT_MS),
            clip("cc", "c", "sound", 0, 0, INPUT_MS),
        ],
    );
    let out = render_project(
        &fx,
        doc,
        &[
            ("a", video_source(0, W, H, true), &a),
            ("c", audio_source(1), &c),
        ],
        None,
    );
    let low = rms_db(&fx, &out.path, 500, 3_000, Some(440));
    let high = rms_db(&fx, &out.path, 500, 3_000, Some(1_000));
    announce(&format!(
        "measured: control {control} dB, 440 Hz {low} dB, 1000 Hz {high} dB"
    ));
    assert!(
        low > BAND_PRESENT_DB,
        "A's 440 Hz band is {low} dB in the mix"
    );
    assert!(
        high > BAND_PRESENT_DB,
        "C's 1000 Hz band is {high} dB in the mix"
    );
}

// F-18: a clip's fade-in ramps its sound. Linear over 1 s, the first
// 0.2 s averages about a tenth of full level; without the fade it is the
// same as 1.5-2 s.
#[test]
fn audio_fade_ramps() {
    let fx = fixture_or_skip!();
    let a = input_a(&fx);
    let mut faded = clip("ca", "a", "v", 0, 0, INPUT_MS);
    faded["fade_in_ms"] = json!(1_000);
    let doc = project(
        vec![asset("a", "video")],
        vec![track("v", "video")],
        vec![faded],
    );
    let out = render_project(&fx, doc, &[("a", video_source(0, W, H, true), &a)], None);
    let early = db_to_linear(rms_db(&fx, &out.path, 0, 200, None));
    let steady = db_to_linear(rms_db(&fx, &out.path, 1_500, 500, None));
    announce(&format!("measured: RMS 0-0.2 s {early}, 1.5-2 s {steady}"));
    assert!(
        early < 0.3 * steady,
        "RMS over 0-0.2 s is {early}, over 1.5-2 s {steady}: the fade does not ramp"
    );
}

// F-19: a 1 s dissolve from A (0-3 s) into B (2-4 s). At its midpoint,
// 2.5 s, the picture is neither A's own frame nor pure red but about the
// average of the two.
#[test]
fn dissolve_blends_midpoint() {
    let fx = fixture_or_skip!();
    let (a, b) = (input_a(&fx), input_b(&fx));
    let mut doc = project(
        vec![asset("a", "video"), asset("b", "video")],
        vec![track("v", "video")],
        vec![
            clip("ca", "a", "v", 0, 0, 3_000),
            clip("cb", "b", "v", 2_000, 0, 2_000),
        ],
    );
    doc["transitions"] = json!([{
        "id": "t", "from": "ca", "to": "cb", "duration_ms": 1_000, "kind": "dissolve",
    }]);
    let out = render_project(
        &fx,
        doc,
        &[
            ("a", video_source(0, W, H, true), &a),
            ("b", video_source(1, 640, 360, false), &b),
        ],
        None,
    );
    let whole = (0, 0, W, H);
    let mid = region_mean(&canvas_frame(&fx, &out.path, 2_500), whole);
    let pure_a = region_mean(&frame_at(&fx, &a, 2_500, (W, H)), whole);
    let pure_b = (255.0, 0.0, 0.0);
    announce(&format!("measured: midpoint {mid:?}, A {pure_a:?}"));
    let far = |p: (f64, f64, f64), q: (f64, f64, f64)| {
        (p.0 - q.0)
            .abs()
            .max((p.1 - q.1).abs())
            .max((p.2 - q.2).abs())
    };
    assert!(
        far(mid, pure_a) > 20.0,
        "the midpoint {mid:?} is still pure A {pure_a:?}"
    );
    assert!(
        far(mid, pure_b) > 20.0,
        "the midpoint {mid:?} is already pure B"
    );
    let half = (
        (pure_a.0 + pure_b.0) / 2.0,
        (pure_a.1 + pure_b.1) / 2.0,
        (pure_a.2 + pure_b.2) / 2.0,
    );
    assert!(
        far(mid, half) < 25.0,
        "the midpoint {mid:?} is not about half-way between A {pure_a:?} and B"
    );
}

// F-05/F-42: D split at 2 s and the halves swapped plays green first. A
// render in SOURCE order would be blue at 0.5 s.
#[test]
fn split_and_reorder_timing() {
    let fx = fixture_or_skip!();
    let d = input_d(&fx);
    let doc = project(
        vec![asset("d", "video")],
        vec![track("v", "video")],
        vec![
            clip("second", "d", "v", 0, 2_000, 4_000),
            clip("first", "d", "v", 2_000, 0, 2_000),
        ],
    );
    let out = render_project(&fx, doc, &[("d", video_source(0, W, H, false), &d)], None);
    let early = pixel(&canvas_frame(&fx, &out.path, 500), 640, 360);
    let late = pixel(&canvas_frame(&fx, &out.path, 2_500), 640, 360);
    assert!(
        is_green(early),
        "0.5 s is {early:?}, want green (the swapped second half)"
    );
    assert!(
        is_blue(late),
        "2.5 s is {late:?}, want blue (the swapped first half)"
    );
}

// F-27: a white text cue over black burns in over ITS span only -- the
// region is brighter during 1.0-2.5 s than before or after it.
#[test]
fn text_cue_is_burned_in() {
    let fx = fixture_or_skip!();
    if !fx.caps.has_filter("ass") {
        announce(
            "SKIPPED text_cue_is_burned_in: the installed ffmpeg has no `ass` filter \
             (libass), so the burned-in cue is UNPROVEN in this run.",
        );
        return;
    }
    let e = input_e(&fx);
    let mut doc = project(
        vec![asset("e", "video")],
        vec![track("v", "video")],
        vec![clip("ce", "e", "v", 0, 0, INPUT_MS)],
    );
    doc["effects"] = json!([{
        "id": "cue", "clip_id": "ce", "kind": "text", "start_ms": 1_000, "end_ms": 2_500,
        "x": 0.1, "y": 0.1, "color": "#ffffff", "text": "HELLO WORLD", "fontSize": 72,
    }]);
    let out = render_project(&fx, doc, &[("e", video_source(0, W, H, false), &e)], None);
    let region = (100, 60, 700, 220);
    let luma = |at| {
        let (r, g, b) = region_mean(&canvas_frame(&fx, &out.path, at), region);
        (r + g + b) / 3.0
    };
    let (before, during, after) = (luma(500), luma(1_750), luma(3_250));
    announce(&format!(
        "measured: cue region {before} / {during} / {after}"
    ));
    assert!(
        during > before + 5.0 && during > after + 5.0,
        "the cue region is {before} before, {during} during and {after} after the cue"
    );
}

// F-42/A13: a range render starts at output zero. 1.0-3.0 s is 2 s long
// and its first frame is A at 1 s -- not A at 0 s.
#[test]
fn range_render_starts_at_zero() {
    let fx = fixture_or_skip!();
    let a = input_a(&fx);
    let doc = project(
        vec![asset("a", "video")],
        vec![track("v", "video")],
        vec![clip("ca", "a", "v", 0, 0, INPUT_MS)],
    );
    let out = render_project(
        &fx,
        doc,
        &[("a", video_source(0, W, H, true), &a)],
        Some((1_000, 3_000)),
    );
    assert!(!out.outcome.remuxed, "a range is never the untouched remux");
    assert_duration_near(probe_duration_ms(&fx, &out.path), 2_000, "the range render");
    assert_duration_near(out.outcome.duration_ms as i64, 2_000, "the outcome");
    let first = canvas_frame(&fx, &out.path, 0);
    let at_one = frame_distance(&first, &frame_at(&fx, &a, 1_000, (W, H)));
    let at_zero = frame_distance(&first, &frame_at(&fx, &a, 0, (W, H)));
    announce(&format!(
        "measured: first frame {at_one} from A@1s, {at_zero} from A@0s"
    ));
    assert!(
        at_one < 6.0 && at_one * 2.0 < at_zero,
        "the first frame is {at_one} from A at 1 s and {at_zero} from A at 0 s"
    );
}

// F-31: a 2x clip plays its 4 s in 2 s -- picture AND sound (`atempo`).
#[test]
fn speed_two_halves_duration() {
    let fx = fixture_or_skip!();
    let a = input_a(&fx);
    let mut fast = clip("ca", "a", "v", 0, 0, INPUT_MS);
    fast["speed"] = json!(2);
    let doc = project(
        vec![asset("a", "video")],
        vec![track("v", "video")],
        vec![fast],
    );
    let out = render_project(&fx, doc, &[("a", video_source(0, W, H, true), &a)], None);
    assert_eq!(out.plan.duration_ms, 2_000, "the plan halves the duration");
    assert_duration_near(probe_duration_ms(&fx, &out.path), 2_000, "the 2x render");
    let kept = rms_db(&fx, &out.path, 200, 1_500, Some(440));
    let doubled = rms_db(&fx, &out.path, 200, 1_500, Some(880));
    assert!(
        kept > BAND_PRESENT_DB && doubled < BAND_PRESENT_DB,
        "a pitch-preserving 2x keeps 440 Hz ({kept} dB) rather than jumping to 880 Hz \
         ({doubled} dB)"
    );
}

// R1: an untouched single source filling its canvas is copied, not
// re-encoded, and keeps the source's own length.
#[test]
fn identity_render_is_a_remux() {
    let fx = fixture_or_skip!();
    let a = input_a(&fx);
    let doc = project(
        vec![asset("a", "video")],
        vec![track("v", "video")],
        vec![clip("ca", "a", "v", 0, 0, INPUT_MS)],
    );
    let out = render_project(&fx, doc, &[("a", video_source(0, W, H, true), &a)], None);
    assert!(out.plan.is_identity(), "the fixture must be R1's identity");
    assert!(out.outcome.remuxed, "an identity render must be a remux");
    let source = probe_duration_ms(&fx, &a);
    assert_duration_near(out.outcome.duration_ms as i64, source, "the remux");
    assert_duration_near(probe_duration_ms(&fx, &out.path), source, "the remux file");
}

// Fix round 1 (review, Important): a staged capture's asset duration is
// the SIDECAR's capture-clock figure, and the fMP4 container can be longer
// or shorter than it by more than a frame + 40 ms (GAP-112's heartbeat,
// GAP-113's unclocked audio). A stream copy cannot change a file's length,
// so an untouched remux is checked against the SOURCE container, never the
// recorded duration -- or the commonest render of all would be refused and
// its output deleted. Here the project records 3.8 s for a 4 s file.
#[test]
fn identity_remux_is_verified_against_the_source_container() {
    let fx = fixture_or_skip!();
    let a = input_a(&fx);
    let recorded = 3_800;
    let mut short = asset("a", "video");
    short["duration_ms"] = json!(recorded);
    let doc = project(
        vec![short],
        vec![track("v", "video")],
        vec![clip("ca", "a", "v", 0, 0, recorded)],
    );
    let out = render_project(&fx, doc, &[("a", video_source(0, W, H, true), &a)], None);
    assert!(out.plan.is_identity(), "the fixture must be R1's identity");
    assert_eq!(out.plan.duration_ms, recorded);
    let source = probe_duration_ms(&fx, &a);
    assert!(
        (source - recorded as i64).abs() > 74,
        "the fixture flaw: the container ({source} ms) must differ from the recorded \
         duration by more than the tolerance"
    );
    assert!(out.outcome.remuxed);
    assert_duration_near(out.outcome.duration_ms as i64, source, "the remux");
}

// A render ffmpeg finished but nobody could VERIFY is not a product: the
// file is deleted rather than offered (the runner's own rule for a failed
// run). An ffprobe that cannot start is the cheapest way to fail the
// verification of an otherwise perfect remux.
#[test]
fn an_unverifiable_render_is_deleted() {
    let fx = fixture_or_skip!();
    let a = input_a(&fx);
    let doc = project(
        vec![asset("a", "video")],
        vec![track("v", "video")],
        vec![clip("ca", "a", "v", 0, 0, INPUT_MS)],
    );
    let missing = fx.dir.path().join("no-such-ffprobe");
    let (_, result, path) = try_render(
        &fx,
        doc,
        &[("a", video_source(0, W, H, true), &a)],
        None,
        &missing,
    );
    assert_eq!(result, Err(ScreenError::ToolMissing));
    assert!(!path.exists(), "the unverified output was left on disk");
}
