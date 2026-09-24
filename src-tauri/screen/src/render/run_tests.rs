//! The render runner's pure half (Task 45): the capability probe's parser,
//! the filters a plan needs, the refusal, the ASS documents in the job dir
//! and the output verification. The REAL round trips are
//! `tests/render_roundtrip.rs`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::json;
use vault_buddy_core::editor::model::{Adjustments, FrameShape, Rotation};
use vault_buddy_core::editor::model_cues::{CaptionPosition, Effect, EffectKind, TransitionKind};
use vault_buddy_core::editor::render_plan::{
    Crop, Cut, PlannedCaption, PlannedCaptions, PlannedCue, RenderPlan,
};
use vault_buddy_core::screen_capture_config::ScreenQuality;

use super::run::{
    parse_output_probe, render, render_refusal, required_filters, verify_output,
    write_ass_documents, OutputProbe, RenderRequestNative,
};
use super::test_support::{audio, card, layer, plan, zoom, FULL, PIP};
use super::{parse_filters_output, render_args, AssHooks, FfmpegCapabilities};
use crate::ffmpeg_args::EncodeSettings;
use crate::ScreenError;

/// `ffmpeg -hide_banner -filters` as 9.0.1 prints it: a legend whose lines
/// hold `=`, a `------` separator, then `flags name io description`.
const FILTERS_9: &str = "Filters:
  T.. = Timeline support
  .S. = Slice threading
  A = Audio input/output
  V = Video input/output
  N = Dynamic number and/or type of input/output
  | = Source or sink filter
  ------
 .. acrossfade        N->A       Cross fade two input audio streams.
 .. ass               V->V       Render ASS subtitles onto input video using the libass library.
 TS overlay           VV->V      Overlay a video source on top of the input.
 .S xfade             VV->V      Cross fade one video with another video.
 .. color             |->V       Provide an uniformly colored input.
";

/// ffmpeg 4.x's shape: three flag columns, and NO separator line.
const FILTERS_4: &str = "Filters:
  T.. = Timeline support
  .S. = Slice threading
  ..C = Command support
  A = Audio input/output
 ... abench            A->A       Benchmark part of a filtergraph.
 TSC overlay           VV->V      Overlay a video source on top of the input.
 ... anullsrc          |->A       Null audio source, return empty audio frames.
";

const ENCODERS: &str = "Encoders:
 V..... = Video
 A..... = Audio
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D libx264rgb           libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 RGB (codec h264)
 A....D aac                  AAC (Advanced Audio Coding)
";

// F21: the capability probe's parser reads NAMES -- never a legend flag, a
// header, the separator or an `=` -- from both ffmpeg generations, and the
// encoder list through the same rule. A parser that kept the legend would
// report a `T..` filter; one that needed the separator would read nothing
// at all from a 4.x build and refuse every render.
#[test]
fn parse_filters_output_reads_names() {
    let caps = parse_filters_output(FILTERS_9);
    for name in ["acrossfade", "ass", "overlay", "xfade", "color"] {
        assert!(caps.has_filter(name), "{name} was not read");
    }
    for junk in ["T..", "=", "Filters:", "------", "A", "|", "Timeline"] {
        assert!(!caps.has_filter(junk), "{junk} is not a filter");
    }
    assert!(!caps.has_encoder("libx264"), "-filters lists no encoders");

    let old = parse_filters_output(FILTERS_4);
    assert!(old.has_filter("overlay") && old.has_filter("anullsrc") && old.has_filter("abench"));
    assert!(!old.has_filter("..C"));

    let with = parse_filters_output(FILTERS_9).with_encoders_output(ENCODERS);
    assert!(with.has_encoder("libx264") && with.has_encoder("aac"));
    assert!(with.has_encoder("libx264rgb"), "exact names, both kept");
    assert!(!with.has_encoder("V....."), "the legend is not an encoder");
    assert!(
        with.has_filter("xfade"),
        "adding encoders keeps the filters"
    );
}

fn text_cue(start: u64, end: u64) -> PlannedCue {
    let effect: Effect = serde_json::from_value(json!({
        "id": "t", "clip_id": "clip", "kind": "text", "start_ms": start, "end_ms": end,
        "x": 0.1, "y": 0.2, "color": "#ffffff", "text": "Hello",
    }))
    .expect("a text effect");
    PlannedCue {
        effect_id: "t".into(),
        clip_id: "clip".into(),
        kind: EffectKind::Text,
        output_start: start,
        output_end: end,
        effect,
        cut: Cut::default(),
    }
}

fn captions() -> PlannedCaptions {
    PlannedCaptions {
        font_size: 28.0,
        position: CaptionPosition::Bottom,
        background: true,
        cues: vec![PlannedCaption {
            id: "q".into(),
            output_start: 200,
            output_end: 1_400,
            text: "a caption".into(),
        }],
    }
}

/// A non-identity plan: one picture-in-picture layer.
fn boxed_plan() -> RenderPlan {
    plan(3_000, vec![layer(0, 0, 0, 3_000, PIP)])
}

// F-27: libass is an optional ffmpeg dependency, so `ass` is required ONLY
// when a document will really be burned -- a cue that draws, a card with
// text, or burned captions. A zoom is not drawn by ASS (it is the
// `perspective` warp), so a zoom-only plan must not demand libass either.
#[test]
fn required_filters_lists_ass_only_when_cues_exist() {
    let bare = boxed_plan();
    assert!(!required_filters(&bare).contains("ass"), "no cue, no ass");

    let mut zoomed = boxed_plan();
    zoomed.cues = vec![zoom("z", 0, 1_000, 2.0, 0.3, 0.6, None)];
    let needs = required_filters(&zoomed);
    assert!(!needs.contains("ass"), "a zoom draws nothing through ASS");
    assert!(
        needs.contains("perspective"),
        "but it is a perspective warp"
    );

    let mut cued = boxed_plan();
    cued.cues = vec![text_cue(100, 900)];
    assert!(required_filters(&cued).contains("ass"), "a text cue burns");

    let mut captioned = boxed_plan();
    captioned.captions = Some(captions());
    assert!(
        required_filters(&captioned).contains("ass"),
        "captions burn"
    );
}

// R1: the identity plan is a stream copy -- no graph, so no filter at all.
// Requiring any would refuse the untouched fast path on a minimal build
// that can do it.
#[test]
fn an_identity_plan_requires_no_filter() {
    let mut whole = layer(0, 0, 0, 60_000, FULL);
    whole.source_in = 0;
    whole.source_out = 60_000;
    let p = plan(60_000, vec![whole]);
    assert!(p.is_identity(), "fixture must be R1's identity");
    assert!(required_filters(&p).is_empty());
    let nothing = parse_filters_output("");
    assert_eq!(render_refusal(&p, &nothing, ""), None);
}

/// Every filter name a `filter_complex` graph (or a lavfi input spec)
/// uses: the token before `=` of every filter, labels stripped, quoted
/// and escaped text skipped. Test-only on purpose: production derives the
/// set from the plan's FEATURES (`required_filters`), and this is the
/// independent reading that keeps the two honest.
fn filter_names(graph: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut chars = graph.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                token.push(c);
                token.extend(chars.next());
            }
            '\'' => {
                quoted = !quoted;
                token.push(c);
            }
            ';' | ',' if !quoted => {
                names.extend(name_of(&token));
                token.clear();
            }
            _ => token.push(c),
        }
    }
    names.extend(name_of(&token));
    names
}

fn name_of(token: &str) -> Option<String> {
    let mut t = token.trim();
    while let Some(rest) = t.strip_prefix('[') {
        t = rest.split_once(']').map_or("", |(_, r)| r);
    }
    let name: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn adjustments() -> Adjustments {
    serde_json::from_value(json!({
        "brightness": 1.2, "contrast": 1.1, "saturation": 0.8, "sepia": 0.3, "grayscale": 0.2,
    }))
    .expect("adjustments")
}

/// Every feature the graph has a filter for, at once.
fn feature_rich_plan() -> RenderPlan {
    let mut turned = layer(0, 3, 0, 4_000, FULL);
    turned.rotation = Rotation::Deg90;
    turned.flip_y = true;
    turned.mirror = true;
    turned.frame_shape = FrameShape::Circle;
    turned.adjustments = Some(adjustments());
    turned.opacity = 0.8;
    turned.fade_in = 300;
    turned.fade_out = 400;
    let mut from = layer(1, 2, 0, 2_000, PIP);
    from.rotation = Rotation::Deg180;
    from.fit = vault_buddy_core::editor::model::Fit::Cover;
    from.crop = Some(Crop {
        zoom: 1.5,
        x: 0.3,
        y: 0.6,
    });
    from.transition_out = Some((TransitionKind::Dissolve, 500));
    let mut to = layer(2, 2, 1_500, 4_000, PIP);
    to.rotation = Rotation::Deg270;
    to.fit = vault_buddy_core::editor::model::Fit::Cover;
    to.frame_shape = FrameShape::Rounded;
    to.transition_in = Some((TransitionKind::Dissolve, 500));
    let mut p = plan(4_000, vec![turned, from, to]);
    let mut card = card(1, 500, 2_500, "#223344");
    card.fade_in = 200;
    card.opacity = 0.9;
    p.cards = vec![card];
    p.cues = vec![
        zoom("z", 1_000, 3_000, 2.0, 0.3, 0.6, Some(400.0)),
        text_cue(200, 1_800),
    ];
    p.captions = Some(captions());
    let mut fast = audio(0, 3, 0, 2_000);
    fast.speed = 2.0;
    fast.fade_in = 300;
    fast.crossfade_out = Some((TransitionKind::EqualPower, 500));
    let mut slow = audio(1, 3, 1_500, 4_000);
    slow.speed = 0.5;
    slow.preserve_pitch = false;
    slow.fade_out = 600;
    slow.crossfade_in = Some((TransitionKind::EqualPower, 500));
    let other = audio(2, 1, 0, 4_000);
    p.audio = vec![fast, slow, other];
    p.master_gain = 0.7;
    p
}

fn settings() -> EncodeSettings {
    EncodeSettings {
        width: 1280,
        height: 720,
        fps: 30,
        quality: ScreenQuality::Balanced,
        h264_encoder: "libx264".into(),
        has_audio: true,
    }
}

// Controller ruling (Task 45): the refusal is only as good as the list it
// checks, and the list is derived from the plan's features -- so every
// filter the REAL argv of a plan using every feature names must be on it.
// A graph filter missing from `required_filters` is one an LGPL or old
// build lacks without the render being refused: it dies inside ffmpeg
// with text that names none of this (`perspective`, `eq` and `geq` are
// GPL-only; `xfade` needs 4.3).
#[test]
fn required_filters_cover_every_filter_the_graph_uses() {
    let p = feature_rich_plan();
    let inputs: Vec<PathBuf> = (0..3)
        .map(|i| PathBuf::from(format!("in{i}.mp4")))
        .collect();
    let (cues, subs) = (Path::new("cues.ass"), Path::new("captions.ass"));
    let hooks = AssHooks {
        cues: Some(cues),
        captions: Some(subs),
        fontsdir: None,
    };
    let args = render_args(&p, &inputs, Path::new("out.mp4"), hooks, &settings());
    let graph_at = args
        .iter()
        .position(|a| a == "-filter_complex")
        .expect("a graph render");
    let mut used = filter_names(&args[graph_at + 1]);
    for window in args.windows(4) {
        if window[0] == "-f" && window[1] == "lavfi" && window[2] == "-i" {
            used.extend(filter_names(&window[3]));
        }
    }
    let required: BTreeSet<String> = required_filters(&p)
        .into_iter()
        .map(str::to_string)
        .collect();
    let missing: Vec<&String> = used.difference(&required).collect();
    assert!(
        missing.is_empty(),
        "the graph uses {missing:?}, which required_filters does not list"
    );
    // The fixture really exercises the optional ones (the fixture flaw: a
    // plan that used none of them would pass vacuously).
    for name in [
        "xfade",
        "acrossfade",
        "atempo",
        "asetrate",
        "afade",
        "fade",
        "geq",
        "eq",
        "colorchannelmixer",
        "perspective",
        "ass",
        "transpose",
        "hflip",
        "vflip",
    ] {
        assert!(used.contains(name), "the fixture never uses {name}");
    }
}

/// Every filter and encoder `p` needs, and nothing else.
fn caps_for(p: &RenderPlan) -> FfmpegCapabilities {
    let listing: String = required_filters(p)
        .iter()
        .map(|f| format!(" .. {f}  V->V  x\n"))
        .collect();
    parse_filters_output(&listing).with_encoders_output(" V....D libx264  x\n A....D aac  x\n")
}

fn without_filter(p: &RenderPlan, gone: &str) -> FfmpegCapabilities {
    let listing: String = required_filters(p)
        .iter()
        .filter(|f| **f != gone)
        .map(|f| format!(" .. {f}  V->V  x\n"))
        .collect();
    parse_filters_output(&listing).with_encoders_output(" V....D libx264  x\n A....D aac  x\n")
}

// F-19: a dissolve on an ffmpeg without `xfade` (older than 4.3, the
// render's floor) is refused BEFORE a child exists, naming the filter and
// the version -- never ffmpeg's own "No such filter" after a wait.
#[test]
fn refusal_names_the_missing_filter() {
    let p = feature_rich_plan();
    assert_eq!(render_refusal(&p, &caps_for(&p), "libx264"), None);

    let message = render_refusal(&p, &without_filter(&p, "xfade"), "libx264")
        .expect("a missing xfade is refused");
    assert!(message.contains("xfade"), "{message}");
    assert!(message.contains("4.3"), "the floor is named: {message}");

    let message = render_refusal(&p, &without_filter(&p, "ass"), "libx264").expect("no libass");
    assert!(
        message.contains("ass") && !message.contains("xfade"),
        "{message}"
    );

    let no_aac = parse_filters_output(
        &required_filters(&p)
            .iter()
            .map(|f| format!(" .. {f}  V->V  x\n"))
            .collect::<String>(),
    )
    .with_encoders_output(" V....D libx264  x\n");
    let message = render_refusal(&p, &no_aac, "libx264").expect("no aac");
    assert!(message.contains("aac"), "{message}");

    let message = render_refusal(&p, &caps_for(&p), "").expect("no H.264 encoder");
    assert!(message.contains("H.264"), "{message}");
    let message = render_refusal(&p, &caps_for(&p), "h264_nvenc").expect("an absent encoder");
    assert!(message.contains("h264_nvenc"), "{message}");
}

fn request<'a>(
    plan: &'a RenderPlan,
    inputs: &'a [PathBuf],
    caps: &'a FfmpegCapabilities,
    dir: &'a Path,
) -> RenderRequestNative<'a> {
    RenderRequestNative {
        ffmpeg: Path::new("no-such-ffmpeg-for-this-test"),
        ffprobe: Path::new("no-such-ffprobe-for-this-test"),
        plan,
        inputs,
        job_dir: dir,
        dest: Path::new("never-written.mp4"),
        fontsdir: None,
        caps,
        settings: settings(),
    }
}

// Task 42 carry: `render_args` asserts one path per planned input. A job
// handing it too few must get an ERROR, never that panic -- a panic on
// the render thread would take the job with it and leave no terminal
// record. Extra paths beyond the plan's last input are the job's whole
// source list and are simply not read.
#[test]
fn too_few_inputs_is_an_error_not_a_panic() {
    let p = feature_rich_plan();
    let caps = caps_for(&p);
    let dir = tempfile::tempdir().expect("tempdir");
    let two = [PathBuf::from("a.mp4"), PathBuf::from("b.mp4")];
    match render(
        request(&p, &two, &caps, dir.path()),
        &AtomicBool::new(false),
        &mut |_| {},
    ) {
        Err(ScreenError::Io(message)) => assert!(message.contains('3'), "{message}"),
        other => panic!("want an Io error naming the count, got {other:?}"),
    }
}

// A refused render never reaches a child or the job dir.
#[test]
fn a_refused_render_writes_nothing() {
    let p = feature_rich_plan();
    let caps = without_filter(&p, "xfade");
    let dir = tempfile::tempdir().expect("tempdir");
    let inputs: Vec<PathBuf> = (0..3)
        .map(|i| PathBuf::from(format!("in{i}.mp4")))
        .collect();
    let result = render(
        request(&p, &inputs, &caps, dir.path()),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(matches!(result, Err(ScreenError::Refused(ref m)) if m.contains("xfade")));
    assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 0);
}

// Controller ruling (Task 45): BOTH documents land in the job dir when each
// has content -- the pre-zoom cues and the post-zoom captions are two
// files for two hooks -- and neither is written when it has none.
#[test]
fn both_ass_documents_are_written_to_the_job_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut p = boxed_plan();
    p.cues = vec![text_cue(100, 900)];
    p.captions = Some(captions());
    let (cues, subs) = write_ass_documents(&p, dir.path()).expect("written");
    let cues = cues.expect("a cue document");
    let subs = subs.expect("a caption document");
    assert_eq!(cues.parent(), Some(dir.path()));
    assert_ne!(cues, subs);
    let cue_text = std::fs::read_to_string(&cues).expect("cues");
    let sub_text = std::fs::read_to_string(&subs).expect("captions");
    assert_eq!(Some(cue_text), super::ass::build_cue_ass(&p));
    assert_eq!(Some(sub_text), super::ass::build_caption_ass(&p));

    let empty = tempfile::tempdir().expect("tempdir");
    let (none_a, none_b) = write_ass_documents(&boxed_plan(), empty.path()).expect("nothing");
    assert!(none_a.is_none() && none_b.is_none());
    assert_eq!(std::fs::read_dir(empty.path()).expect("dir").count(), 0);
}

#[test]
fn the_output_probe_reads_streams_and_duration() {
    let probe = parse_output_probe(
        "codec_type=video\ncodec_type=audio\ncodec_type=data\nduration=2.034000\n",
    );
    assert_eq!(
        probe,
        OutputProbe {
            duration_ms: Some(2_034),
            video_streams: 1,
            audio_streams: 1,
        }
    );
    assert_eq!(parse_output_probe("duration=N/A\n").duration_ms, None);
}

// The brief's check: within one frame + 40 ms of the plan, at least one
// video stream, and audio -- except a remux of a SILENT source, which
// copies the no-audio stream set it was given (R1's silent identity).
#[test]
fn the_output_is_verified_against_the_plan() {
    let ok = OutputProbe {
        duration_ms: Some(2_070),
        video_streams: 1,
        audio_streams: 1,
    };
    // 1 frame at 30 fps (34 ms, rounded up) + 40 ms = 74 ms.
    assert_eq!(verify_output(&ok, 2_000, 30, true), Ok(2_070));
    let long = OutputProbe {
        duration_ms: Some(2_075),
        ..ok
    };
    assert!(verify_output(&long, 2_000, 30, true).is_err());
    let short = OutputProbe {
        duration_ms: Some(1_925),
        ..ok
    };
    assert!(verify_output(&short, 2_000, 30, true).is_err());
    let silent = OutputProbe {
        audio_streams: 0,
        ..ok
    };
    assert!(verify_output(&silent, 2_000, 30, true).is_err());
    assert_eq!(verify_output(&silent, 2_000, 30, false), Ok(2_070));
    let blind = OutputProbe {
        video_streams: 0,
        ..ok
    };
    assert!(verify_output(&blind, 2_000, 30, false).is_err());
    let unknown = OutputProbe {
        duration_ms: None,
        ..ok
    };
    assert!(verify_output(&unknown, 2_000, 30, true).is_err());
}
