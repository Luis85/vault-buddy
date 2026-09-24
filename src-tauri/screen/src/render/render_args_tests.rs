//! The render's argv (Task 42).

use std::path::{Path, PathBuf};

use super::render_args;
use super::test_support::{input, layer, plan, FULL, PIP};
use crate::ffmpeg_args::{remux_args, EncodeSettings};
use vault_buddy_core::screen_capture_config::ScreenQuality;

fn settings() -> EncodeSettings {
    EncodeSettings {
        width: 1280,
        height: 720,
        fps: 30,
        quality: ScreenQuality::Balanced,
        h264_encoder: "libx264".into(),
        has_audio: false,
    }
}

/// One untouched 1920x1080 silent capture filling its 1280x720 canvas: the
/// plan R1 remuxes (Task 41's controller ruling covers the silent case).
fn identity_plan() -> vault_buddy_core::editor::render_plan::RenderPlan {
    let mut whole = layer(0, 0, 0, 60_000, FULL);
    whole.source_in = 0;
    whole.source_out = 60_000;
    plan(60_000, vec![whole])
}

// R1: an untouched single-source project is a lossless remux at SOURCE
// resolution -- no decode, no re-encode, no filter graph. Re-encoding it
// instead would silently cost quality and minutes on the commonest render.
#[test]
fn identity_plan_remuxes() {
    let p = identity_plan();
    assert!(p.is_identity(), "fixture must be R1's identity");
    let src = PathBuf::from("staged capture.mp4");
    let dest = Path::new("out.mp4.part");
    let args = render_args(&p, std::slice::from_ref(&src), dest, None, &settings());
    assert_eq!(args, remux_args(&src, dest));

    // One edit (2x) and it is a graph render, never a copy.
    let mut edited = identity_plan();
    edited.video_layers[0].speed = 2.0;
    edited.video_layers[0].output_end = 30_000;
    edited.duration_ms = 30_000;
    assert!(!edited.is_identity());
    let args = render_args(&edited, &[src], dest, None, &settings());
    assert!(args.iter().any(|a| a == "-filter_complex"), "{args:?}");
    assert!(!args.iter().any(|a| a == "copy"), "{args:?}");
}

#[test]
fn a_render_maps_the_final_label_and_encodes_video() {
    let p = plan(
        5_000,
        vec![layer(0, 1, 0, 5_000, FULL), layer(1, 0, 1_000, 4_000, PIP)],
    );
    let inputs = [PathBuf::from("a.mp4"), PathBuf::from("b cam.mp4")];
    let args = render_args(&p, &inputs, Path::new("out.part"), None, &settings());
    let at = |needle: &str| {
        args.iter()
            .position(|a| a == needle)
            .unwrap_or_else(|| panic!("{needle} missing from {args:?}"))
    };
    // The runner reads -progress on stdout: without it the job has no bar.
    assert_eq!(
        &args[..7],
        [
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-nostats",
            "-progress",
            "pipe:1"
        ]
    );
    // File inputs in plan order, THEN the lavfi canvas (input 2).
    assert!(at("a.mp4") < at("b cam.mp4"));
    assert!(at("b cam.mp4") < at("lavfi"));
    assert_eq!(args[at("-map") + 1], "[vout]");
    assert_eq!(args[at("-c:v") + 1], "libx264");
    assert_eq!(args[at("-pix_fmt") + 1], "yuv420p");
    assert_eq!(args[at("-f") + 1], "lavfi");
    assert!(args.ends_with(&[
        "-f".to_string(),
        "mp4".into(),
        "-y".into(),
        "out.part".into()
    ]));
}

// An image has no frame clock: it is looped for as long as its longest use
// reads it, at the canvas rate.
#[test]
fn an_image_input_loops_for_its_longest_use() {
    let mut short = layer(0, 1, 0, 2_000, FULL);
    short.source_out = 3_000;
    let mut long = layer(0, 0, 2_000, 5_000, PIP);
    long.source_out = 5_500;
    let mut p = plan(5_000, vec![short, long]);
    p.inputs = vec![input(0, true)];
    let args = render_args(
        &p,
        &[PathBuf::from("slide.png")],
        Path::new("o"),
        None,
        &settings(),
    );
    let i = args
        .iter()
        .position(|a| a == "slide.png")
        .expect("the image input");
    assert_eq!(
        args[i - 7..=i],
        [
            "-loop",
            "1",
            "-framerate",
            "30",
            "-t",
            "5.500",
            "-i",
            "slide.png"
        ],
        "{args:?}"
    );
}

// The ASS document (Task 43) is burned at the PRE-zoom hook, and its path
// crosses two escaping levels on the way into the filtergraph: a Windows
// drive colon and backslashes that reached ffmpeg raw would split the
// option or eat the separators.
#[test]
fn an_ass_document_is_burned_at_the_pre_zoom_hook() {
    let p = plan(5_000, vec![layer(0, 0, 0, 5_000, FULL)]);
    let ass = Path::new(r"C:\x\cues.ass");
    let args = render_args(
        &p,
        &[PathBuf::from("a.mp4")],
        Path::new("o"),
        Some(ass),
        &settings(),
    );
    let graph = &args[args
        .iter()
        .position(|a| a == "-filter_complex")
        .expect("graph")
        + 1];
    assert!(
        graph.contains(r"[vcomp]ass=filename=C\\:\\\\x\\\\cues.ass[vcued]"),
        "{graph}"
    );
}
