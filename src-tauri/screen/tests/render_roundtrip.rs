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
//! installs ffmpeg so these run there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use serde_json::{json, Value};
use tempfile::TempDir;
use vault_buddy_core::editor::model::Project;
use vault_buddy_core::editor::render_plan::{plan, PlanSource, RenderPlan};
use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_screen::ffmpeg_args::EncodeSettings;
use vault_buddy_screen::render::run::{render, RenderOutcome, RenderRequestNative};
use vault_buddy_screen::render::{parse_filters_output, FfmpegCapabilities};
use vault_buddy_screen::ScreenError;

const W: u32 = 1280;
const H: u32 = 720;
const FPS: u32 = 30;
/// Every synthesized input is this long.
const INPUT_MS: u64 = 4_000;
/// The brief's duration tolerance for a range render.
const DURATION_TOLERANCE_MS: i64 = 40;
/// A band whose RMS is above this carries its tone; the other band of a
/// single-tone fixture measures about -60 dB (checked in test 3's control).
const BAND_PRESENT_DB: f64 = -40.0;

fn announce(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{message}");
}

fn tool_on_path(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&exe))
        .find(|candidate| candidate.is_file())
}

/// Everything one round trip needs: the tools, what the installed ffmpeg
/// can do, and a scratch directory that doubles as the render's job dir.
struct Fixture {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    caps: FfmpegCapabilities,
    dir: TempDir,
}

macro_rules! fixture_or_skip {
    () => {
        match fixture() {
            Some(fx) => fx,
            None => {
                let thread = std::thread::current();
                let who = thread.name().unwrap_or(module_path!()).to_string();
                announce(&format!(
                    "SKIPPED {who}: no ffmpeg/ffprobe on PATH, so this render round trip \
                     is UNPROVEN in this run. Install ffmpeg (CI does) to execute it."
                ));
                return;
            }
        }
    };
}

fn fixture() -> Option<Fixture> {
    let ffmpeg = tool_on_path("ffmpeg")?;
    let ffprobe = tool_on_path("ffprobe")?;
    let filters =
        String::from_utf8_lossy(&tool_stdout(&ffmpeg, &["-hide_banner", "-filters"])).into_owned();
    let encoders =
        String::from_utf8_lossy(&tool_stdout(&ffmpeg, &["-hide_banner", "-encoders"])).into_owned();
    let caps = parse_filters_output(&filters).with_encoders_output(&encoders);
    let dir = tempfile::tempdir().expect("tempdir");
    Some(Fixture {
        ffmpeg,
        ffprobe,
        caps,
        dir,
    })
}

fn tool_stdout<S: AsRef<std::ffi::OsStr>>(tool: &Path, args: &[S]) -> Vec<u8> {
    let out = Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("the tool ran");
    assert!(
        out.status.success(),
        "{} failed: {}",
        tool.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// ffmpeg's stderr at `-v info` (where `astats` reports).
fn ffmpeg_stderr(ffmpeg: &Path, args: &[String]) -> String {
    let out = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("ffmpeg ran");
    assert!(
        out.status.success(),
        "ffmpeg {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

// ---------------------------------------------------------------- inputs

/// Encodes the lavfi `sources` (each `-f lavfi -i <spec>`) to `name`.
fn synthesize(fx: &Fixture, name: &str, sources: &[&str], codecs: &[&str]) -> PathBuf {
    let path = fx.dir.path().join(name);
    let mut args = strings(&["-v", "error"]);
    for spec in sources {
        args.extend(strings(&["-f", "lavfi", "-i", spec]));
    }
    args.extend(strings(codecs));
    args.push("-y".into());
    args.push(path.to_string_lossy().into_owned());
    tool_stdout(&fx.ffmpeg, &args);
    path
}

const VIDEO: [&str; 4] = ["-c:v", "libx264", "-pix_fmt", "yuv420p"];

/// A: `testsrc2` 1280x720 with a 440 Hz tone.
fn input_a(fx: &Fixture) -> PathBuf {
    let mut codecs = VIDEO.to_vec();
    codecs.extend(["-c:a", "aac"]);
    synthesize(
        fx,
        "a.mp4",
        &[
            "testsrc2=s=1280x720:r=30:d=4",
            "sine=frequency=440:sample_rate=48000:duration=4",
        ],
        &codecs,
    )
}

/// B: solid red 640x360, no audio.
fn input_b(fx: &Fixture) -> PathBuf {
    synthesize(fx, "b.mp4", &["color=c=red:s=640x360:r=30:d=4"], &VIDEO)
}

/// C: a 1000 Hz tone, no picture.
fn input_c(fx: &Fixture) -> PathBuf {
    synthesize(
        fx,
        "c.m4a",
        &["sine=frequency=1000:sample_rate=48000:duration=4"],
        &["-c:a", "aac"],
    )
}

/// D: 2 s of blue, then 2 s of green (lavfi `concat`).
fn input_d(fx: &Fixture) -> PathBuf {
    synthesize(
        fx,
        "d.mp4",
        &[
            "color=c=blue:s=1280x720:r=30:d=2[x];color=c=green:s=1280x720:r=30:d=2[y];\
           [x][y]concat=n=2:v=1:a=0",
        ],
        &VIDEO,
    )
}

/// E: plain black, for the text cue.
fn input_e(fx: &Fixture) -> PathBuf {
    synthesize(fx, "e.mp4", &["color=c=black:s=1280x720:r=30:d=4"], &VIDEO)
}

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

// ---------------------------------------------------------------- reading

type Rgb = (i32, i32, i32);

/// The whole RGB frame of `path` (at `w`x`h`) at `at_ms`.
fn frame_at(fx: &Fixture, path: &Path, at_ms: u64, (w, h): (u32, u32)) -> Vec<u8> {
    let mut args = strings(&["-v", "error", "-ss"]);
    args.push(format!("{}.{:03}", at_ms / 1000, at_ms % 1000));
    args.push("-i".into());
    args.push(path.to_string_lossy().into_owned());
    args.extend(strings(&[
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
    ]));
    let frame = tool_stdout(&fx.ffmpeg, &args);
    assert_eq!(
        frame.len(),
        (w * h * 3) as usize,
        "{} is not {w}x{h} at {at_ms} ms",
        path.display()
    );
    frame
}

fn canvas_frame(fx: &Fixture, path: &Path, at_ms: u64) -> Vec<u8> {
    frame_at(fx, path, at_ms, (W, H))
}

fn pixel(frame: &[u8], x: u32, y: u32) -> Rgb {
    let i = ((y * W + x) * 3) as usize;
    (frame[i] as i32, frame[i + 1] as i32, frame[i + 2] as i32)
}

fn is_red(p: Rgb) -> bool {
    p.0 > 200 && p.1 < 60 && p.2 < 60
}

fn is_green(p: Rgb) -> bool {
    p.0 < 60 && p.1 > 100 && p.2 < 60
}

fn is_blue(p: Rgb) -> bool {
    p.0 < 60 && p.1 < 60 && p.2 > 200
}

/// Mean of each channel over the rectangle `[x0,x1) x [y0,y1)`.
fn region_mean(frame: &[u8], (x0, y0, x1, y1): (u32, u32, u32, u32)) -> (f64, f64, f64) {
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = pixel(frame, x, y);
            r += f64::from(p.0);
            g += f64::from(p.1);
            b += f64::from(p.2);
            n += 1.0;
        }
    }
    (r / n, g / n, b / n)
}

/// Mean absolute per-byte difference of two equal-sized frames.
fn frame_distance(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let total: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.len() as f64
}

/// `astats`' overall RMS level (dB) of `path`'s audio over `[start, start +
/// len)` ms, through `bandpass` at `band` Hz (50 Hz wide) when given.
fn rms_db(fx: &Fixture, path: &Path, start_ms: u64, len_ms: u64, band: Option<u32>) -> f64 {
    let mut args = strings(&["-hide_banner", "-v", "info", "-ss"]);
    args.push(format!("{:.3}", start_ms as f64 / 1_000.0));
    args.push("-t".into());
    args.push(format!("{:.3}", len_ms as f64 / 1_000.0));
    args.push("-i".into());
    args.push(path.to_string_lossy().into_owned());
    args.push("-af".into());
    args.push(match band {
        Some(f) => format!("bandpass=f={f}:w=50,astats"),
        None => "astats".into(),
    });
    args.extend(strings(&["-f", "null", "-"]));
    let stderr = ffmpeg_stderr(&fx.ffmpeg, &args);
    // The LAST "RMS level dB" line is astats' "Overall" section.
    stderr
        .lines()
        .rev()
        .find_map(|l| l.split_once("RMS level dB: ").map(|(_, v)| v.trim()))
        .map(|v| {
            if v == "-inf" {
                f64::NEG_INFINITY
            } else {
                v.parse().expect("an RMS level")
            }
        })
        .unwrap_or_else(|| panic!("astats reported no RMS level: {stderr}"))
}

fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The container duration of `path` in ms, read independently of the
/// render's own verification.
fn probe_duration_ms(fx: &Fixture, path: &Path) -> i64 {
    let mut args = strings(&[
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "csv=p=0",
    ]);
    args.push(path.to_string_lossy().into_owned());
    let text = String::from_utf8_lossy(&tool_stdout(&fx.ffprobe, &args)).into_owned();
    let seconds: f64 = text.trim().parse().expect("a duration");
    (seconds * 1_000.0).round() as i64
}

fn assert_duration_near(got_ms: i64, want_ms: i64, what: &str) {
    assert!(
        (got_ms - want_ms).abs() <= DURATION_TOLERANCE_MS,
        "{what}: the output is {got_ms} ms, want {want_ms} +/- {DURATION_TOLERANCE_MS} ms"
    );
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
    let boxed = pixel(&canvas_frame(&fx, &out.path, 1_000), 1_100, 90);
    assert!(
        !is_red(boxed),
        "the hidden layer's box still renders: {boxed:?}"
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
