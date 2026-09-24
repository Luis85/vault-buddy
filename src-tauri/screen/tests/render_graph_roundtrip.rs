//! The render graph's REAL round trips (tutorial-editor Task 42): build a
//! `RenderPlan` by hand, turn it into an argv with `render::render_args`,
//! run it through the SAME runner the export uses, and read pixels back.
//!
//! The golden-string tests in `src/render/` pin what the graph SAYS; only
//! a real ffmpeg can say whether it is a graph at all -- whether `xfade`
//! accepts the two members, whether the `perspective` zoom really ramps on
//! the frame counter, whether a transparent pad stays transparent through
//! the overlay. Each assertion samples a pixel whose colour changes if the
//! thing it is about is wrong: a zoom that never engages leaves the right
//! edge BLUE, a dissolve at the wrong offset is not half-way at 2.1 s, and
//! a picture-in-picture drawn under the base layer is not green.
//!
//! Skips VISIBLY without ffmpeg (the `export_roundtrip.rs` rule): a silent
//! skip is indistinguishable from a pass.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use serde_json::json;
use vault_buddy_core::editor::model::{
    Canvas, Card, CardPreset, FadeCurve, Fit, FrameShape, Rotation,
};
use vault_buddy_core::editor::model_cues::{Effect, EffectKind, TransitionKind};
use vault_buddy_core::editor::render_plan::{
    AudioContribution, Cut, PixelBox, PlanInput, PlannedCard, PlannedCue, RenderPlan, VideoLayer,
};
use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_screen::ffmpeg_args::EncodeSettings;
use vault_buddy_screen::ffmpeg_run::run;
use vault_buddy_screen::render::ass::build_cue_ass;
use vault_buddy_screen::render::{render_args, AssHooks};

const W: u32 = 1280;
const H: u32 = 720;
/// Per-channel tolerance for a sampled pixel: H.264 at yuv420p moves a
/// flat colour by a few levels and blurs an edge, never by this much.
const TOLERANCE: i32 = 60;

fn announce(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{message}");
}

fn ffmpeg_on_path() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
}

macro_rules! ffmpeg_or_skip {
    () => {
        match ffmpeg_on_path() {
            Some(path) => path,
            None => {
                let thread = std::thread::current();
                let who = thread.name().unwrap_or(module_path!()).to_string();
                announce(&format!(
                    "SKIPPED {who}: no ffmpeg on PATH, so this render round trip is \
                     UNPROVEN in this run. Install ffmpeg (CI does) to execute it."
                ));
                return;
            }
        }
    };
}

fn ffmpeg_stdout(ffmpeg: &Path, args: &[String]) -> Vec<u8> {
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
    out.stdout
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

/// A 3 s, 30 fps H.264 clip of the lavfi `graph`.
fn make_clip(ffmpeg: &Path, dir: &Path, name: &str, graph: &str) -> PathBuf {
    let path = dir.join(name);
    let mut args = argv(&["-v", "error", "-f", "lavfi", "-i", graph]);
    args.extend(argv(&[
        "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-y",
    ]));
    args.push(path.to_string_lossy().into_owned());
    ffmpeg_stdout(ffmpeg, &args);
    path
}

/// A 640x360 clip whose LEFT half is red and RIGHT half blue.
fn halves(ffmpeg: &Path, dir: &Path) -> PathBuf {
    make_clip(
        ffmpeg,
        dir,
        "halves.mp4",
        "color=c=red:s=320x360:r=30:d=3[a];color=c=blue:s=320x360:r=30:d=3[b];[a][b]hstack",
    )
}

fn green(ffmpeg: &Path, dir: &Path) -> PathBuf {
    make_clip(
        ffmpeg,
        dir,
        "green.mp4",
        "color=c=0x00ff00:s=400x300:r=30:d=3",
    )
}

/// A 3 s clip like `make_clip`, but carrying a real 440 Hz tone alongside
/// its black video -- the one fixture the audio round trip below needs a
/// probeable signal in (Task 44).
fn tone(ffmpeg: &Path, dir: &Path) -> PathBuf {
    let path = dir.join("tone.mp4");
    let mut args = argv(&[
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=640x360:r=30:d=3",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=3",
    ]);
    args.extend(argv(&[
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-y",
    ]));
    args.push(path.to_string_lossy().into_owned());
    ffmpeg_stdout(ffmpeg, &args);
    path
}

fn settings() -> EncodeSettings {
    EncodeSettings {
        width: W,
        height: H,
        fps: 30,
        quality: ScreenQuality::Balanced,
        h264_encoder: "libx264".into(),
        has_audio: false,
    }
}

fn layer(input: usize, track_index: usize, start: u64, end: u64, bounds: PixelBox) -> VideoLayer {
    VideoLayer {
        clip_id: format!("c{input}-{start}"),
        track_index,
        input,
        output_start: start,
        output_end: end,
        source_in: 0,
        source_out: end - start,
        speed: 1.0,
        bounds,
        fit: Fit::Cover,
        frame_shape: FrameShape::Rectangle,
        rotation: Rotation::Deg0,
        mirror: false,
        flip_y: false,
        crop: None,
        opacity: 1.0,
        fade_in: 0,
        fade_out: 0,
        fade_curve: FadeCurve::Linear,
        adjustments: None,
        transition_in: None,
        transition_out: None,
        cut: Cut::default(),
    }
}

fn input(index: usize, width: u32, height: u32) -> PlanInput {
    PlanInput {
        asset_id: format!("a{index}"),
        input_index: index,
        has_video: true,
        has_audio: false,
        width,
        height,
        is_image: false,
        duration_ms: 3_000,
    }
}

const FULL: PixelBox = PixelBox {
    x: 0,
    y: 0,
    w: W,
    h: H,
};

fn plan(layers: Vec<VideoLayer>) -> RenderPlan {
    RenderPlan {
        canvas: Canvas {
            width: W,
            height: H,
            fps: 30,
            extra: Default::default(),
        },
        duration_ms: 3_000,
        inputs: vec![input(0, 640, 360), input(1, 400, 300)],
        video_layers: layers,
        audio: Vec::new(),
        cues: Vec::new(),
        captions: None,
        chapters: Vec::new(),
        cards: Vec::new(),
        master_gain: 1.0,
    }
}

fn render(ffmpeg: &Path, dir: &Path, plan: &RenderPlan, inputs: &[PathBuf]) -> PathBuf {
    render_with_ass(ffmpeg, dir, plan, inputs, AssHooks::default())
}

fn render_with_ass(
    ffmpeg: &Path,
    dir: &Path,
    plan: &RenderPlan,
    inputs: &[PathBuf],
    ass: AssHooks<'_>,
) -> PathBuf {
    let dest = dir.join("render.mp4.part");
    let args = render_args(plan, inputs, &dest, ass, &settings());
    run(
        ffmpeg,
        &args,
        &dest,
        plan.duration_ms,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("the render failed: {e}\nargs: {args:?}"));
    dest
}

/// The whole RGB frame at `at_ms`.
fn frame_at(ffmpeg: &Path, path: &Path, at_ms: u64) -> Vec<u8> {
    let mut args = argv(&["-v", "error", "-ss"]);
    args.push(format!("{}.{:03}", at_ms / 1000, at_ms % 1000));
    args.push("-i".into());
    args.push(path.to_string_lossy().into_owned());
    args.extend(argv(&[
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
    ]));
    let frame = ffmpeg_stdout(ffmpeg, &args);
    assert_eq!(
        frame.len(),
        (W * H * 3) as usize,
        "the render is not {W}x{H} at {at_ms} ms"
    );
    frame
}

fn assert_pixel(frame: &[u8], x: u32, y: u32, want: (i32, i32, i32), what: &str) {
    let i = ((y * W + x) * 3) as usize;
    let got = (frame[i] as i32, frame[i + 1] as i32, frame[i + 2] as i32);
    let close = (got.0 - want.0).abs() <= TOLERANCE
        && (got.1 - want.1).abs() <= TOLERANCE
        && (got.2 - want.2).abs() <= TOLERANCE;
    assert!(
        close,
        "{what}: pixel ({x},{y}) is {got:?}, want about {want:?}"
    );
}

const RED: (i32, i32, i32) = (255, 0, 0);
const BLUE: (i32, i32, i32) = (0, 0, 255);
const GREEN: (i32, i32, i32) = (0, 255, 0);
const YELLOW: (i32, i32, i32) = (255, 255, 0);

// Bottom: the red|blue halves filling the canvas (track 2). Top: a green
// circle-masked, turned, mirrored picture-in-picture at the fixture's box
// (track 0). Between them: a yellow card (track 1). Last: a 2x zoom on the
// stage's left half with a 0.2 s ramp, plateau 2.2-2.8 s.
#[test]
fn a_composed_render_draws_every_layer_in_order_and_zooms_the_stage() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let inputs = [halves(&ffmpeg, dir.path()), green(&ffmpeg, dir.path())];

    let base = layer(0, 2, 0, 3_000, FULL);
    let pip_box = PixelBox {
        x: 992,
        y: 44,
        w: 244,
        h: 242,
    };
    let mut pip = layer(1, 0, 500, 2_000, pip_box);
    pip.frame_shape = FrameShape::Circle;
    pip.rotation = Rotation::Deg90;
    pip.mirror = true;
    pip.fade_in = 300;
    let mut p = plan(vec![base, pip]);
    p.cards = vec![PlannedCard {
        clip_id: "card".into(),
        track_index: 1,
        output_start: 1_000,
        output_end: 1_800,
        bounds: PixelBox {
            x: 100,
            y: 500,
            w: 200,
            h: 120,
        },
        opacity: 1.0,
        fade_in: 0,
        fade_out: 0,
        fade_curve: FadeCurve::Linear,
        transition_in: None,
        transition_out: None,
        card: Some(Card {
            preset: CardPreset::Chapter,
            title: "t".into(),
            subtitle: "s".into(),
            background: "#ffff00".into(),
            foreground: "#000000".into(),
            accent: "#000000".into(),
            extra: Default::default(),
        }),
        cut: Cut::default(),
    }];
    let effect: Effect = serde_json::from_value(json!({
        "id": "z", "clip_id": "c0-0", "kind": "zoom", "start_ms": 2_000, "end_ms": 3_000,
        "x": 0.25, "y": 0.5, "color": "#ffffff", "factor": 2, "easing": 200,
    }))
    .expect("zoom");
    p.cues = vec![PlannedCue {
        effect_id: "z".into(),
        clip_id: "c0-0".into(),
        kind: EffectKind::Zoom,
        output_start: 2_000,
        output_end: 3_000,
        effect,
        cut: Cut::default(),
    }];
    let out = render(&ffmpeg, dir.path(), &p, &inputs);

    let early = frame_at(&ffmpeg, &out, 250);
    assert_pixel(&early, 100, 100, RED, "the base's left half");
    assert_pixel(&early, 1200, 600, BLUE, "the base's right half, unzoomed");
    assert_pixel(
        &early,
        1114,
        165,
        BLUE,
        "no picture-in-picture before 0.5 s",
    );

    let middle = frame_at(&ffmpeg, &out, 1_300);
    assert_pixel(
        &middle,
        1114,
        165,
        GREEN,
        "the picture-in-picture is drawn ON TOP",
    );
    // The circle mask: the box's corner is the base showing through.
    assert_pixel(&middle, 994, 46, BLUE, "the circle's transparent corner");
    assert_pixel(&middle, 200, 560, YELLOW, "the card over the base");

    let zoomed = frame_at(&ffmpeg, &out, 2_500);
    assert_pixel(
        &zoomed,
        1200,
        600,
        RED,
        "a 2x zoom on the left half fills the frame",
    );
    assert_pixel(&zoomed, 640, 360, RED, "the zoomed stage's centre");
}

// F-19: a dissolve over the 0.6 s overlap of two clips on ONE track.
// Half-way through it the picture is half of each; before it only the
// first plays and after it only the second. ASYMMETRIC on purpose: the
// overlap (0.6 s) differs from where it starts (1.8 s), so an xfade handed
// the duration as its offset dissolves at the wrong time and fails here --
// a first draft with a 1 s overlap starting at 1 s could not tell them apart.
#[test]
fn a_dissolve_blends_the_pair_across_their_overlap() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let inputs = [halves(&ffmpeg, dir.path()), green(&ffmpeg, dir.path())];
    let mut from = layer(0, 0, 0, 2_400, FULL);
    from.transition_out = Some((TransitionKind::Dissolve, 600));
    let mut to = layer(1, 0, 1_800, 3_000, FULL);
    to.transition_in = Some((TransitionKind::Dissolve, 600));
    let out = render(&ffmpeg, dir.path(), &plan(vec![from, to]), &inputs);

    assert_pixel(
        &frame_at(&ffmpeg, &out, 1_000),
        100,
        100,
        RED,
        "before the overlap",
    );
    assert_pixel(
        &frame_at(&ffmpeg, &out, 2_100),
        100,
        100,
        (128, 128, 0),
        "half-way through the dissolve",
    );
    assert_pixel(
        &frame_at(&ffmpeg, &out, 2_700),
        100,
        100,
        GREEN,
        "after the overlap",
    );
}

const MAGENTA: (i32, i32, i32) = (255, 0, 255);

// A REAL burn: `render::ass::build_cue_ass`'s output, written to disk and
// handed to a real ffmpeg through the `ass=` hook (Task 43). The golden
// string tests in `src/render/ass_tests.rs` pin what the document SAYS;
// only real libass can say whether ffmpeg 9.0.1 actually parses it -- the
// two escaping levels, the style block, the `\p1` drawing syntax. A mask
// cue is the cheapest possible probe: its drawn colour is deterministic
// (no font/glyph rendering to tolerate), so a wrong escape, a malformed
// `\p1` path or a swapped coordinate all fail as a WRONG PIXEL rather than
// a silent parse warning ffmpeg would otherwise swallow with `-loglevel
// error`.
#[test]
fn a_burned_mask_cue_is_a_real_opaque_rectangle() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    // `plan()` always reserves two file inputs (its own fixture shape); the
    // second is unused by this test's one layer.
    let inputs = [halves(&ffmpeg, dir.path()), green(&ffmpeg, dir.path())];

    let base = layer(0, 0, 0, 3_000, FULL);
    let mut p = plan(vec![base]);
    let effect: Effect = serde_json::from_value(json!({
        "id": "m", "clip_id": "c0-0", "kind": "mask", "start_ms": 0, "end_ms": 3_000,
        "x": 0.5, "y": 0.0, "w": 0.5, "h": 1.0, "color": "#ff00ff",
    }))
    .expect("mask effect");
    p.cues = vec![PlannedCue {
        effect_id: "m".into(),
        clip_id: "c0-0".into(),
        kind: EffectKind::Mask,
        output_start: 0,
        output_end: 3_000,
        effect,
        cut: Cut::default(),
    }];

    let ass_text = build_cue_ass(&p).expect("the mask cue produces a document");
    let ass_path = dir.path().join("cues.ass");
    std::fs::write(&ass_path, ass_text).expect("write the ass document");

    let out = render_with_ass(
        &ffmpeg,
        dir.path(),
        &p,
        &inputs,
        AssHooks {
            cues: Some(&ass_path),
            ..AssHooks::default()
        },
    );

    let frame = frame_at(&ffmpeg, &out, 1_500);
    assert_pixel(
        &frame,
        100,
        100,
        RED,
        "the base's left half, outside the mask",
    );
    assert_pixel(
        &frame,
        1_200,
        600,
        MAGENTA,
        "the mask covers the right half where the base was blue",
    );
}

/// The audio path end to end (Task 44): `adelay` really positions a
/// clip's sound in the rendered file, not just in the golden `filter_
/// complex` string `src/render/audio_graph_tests.rs` pins. A tone placed
/// 1 s into the plan renders SILENCE up to about 1 s and the tone after
/// it; `silencedetect` on the rendered file's own audio stream is the
/// cheapest real proof that ffmpeg actually accepted the graph (`amix`,
/// `alimiter`, `aresample`, `aformat` included) and produced the timing
/// it claims, which no golden string can prove by itself.
#[test]
fn a_clips_audio_lands_at_its_planned_delay() {
    let ffmpeg = ffmpeg_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    // `plan()` always reserves two file inputs (its own fixture shape); the
    // second is unused by this test's one layer and one contribution.
    let inputs = [tone(&ffmpeg, dir.path()), green(&ffmpeg, dir.path())];

    let base = layer(0, 0, 0, 3_000, FULL);
    let mut p = plan(vec![base]);
    p.audio = vec![AudioContribution {
        clip_id: "c0-0".into(),
        track_index: 0,
        input: 0,
        output_start: 1_000,
        output_end: 3_000,
        source_in: 0,
        source_out: 2_000,
        speed: 1.0,
        preserve_pitch: true,
        gain: 1.0,
        fade_in: 0,
        fade_out: 0,
        curve: FadeCurve::Linear,
        crossfade_in: None,
        crossfade_out: None,
        cut: Cut::default(),
    }];
    let out = render(&ffmpeg, dir.path(), &p, &inputs);

    let mut args = argv(&["-v", "info", "-i"]);
    args.push(out.to_string_lossy().into_owned());
    args.extend(argv(&[
        "-af",
        "silencedetect=noise=-30dB:d=0.1",
        "-f",
        "null",
        "-",
    ]));
    let result = Command::new(&ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .expect("ffmpeg ran");
    let stderr = String::from_utf8_lossy(&result.stderr);
    let silence_end: f64 = stderr
        .lines()
        .find_map(|line| {
            let after = line.split_once("silence_end: ")?.1;
            after.split_whitespace().next()
        })
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no silence_end reported by silencedetect: {stderr}"));
    assert!(
        (silence_end - 1.0).abs() < 0.15,
        "the tone should start about 1.0 s in (adelay=1000), not {silence_end}: {stderr}"
    );
}
