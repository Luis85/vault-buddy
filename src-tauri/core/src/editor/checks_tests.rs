//! Task 54's named tests: one per check code (F27: fourteen), each a
//! minimal project that raises EXACTLY that finding next to a control that
//! raises none, plus the wire pin and "no quality score, ever".
//!
//! Every fixture starts from `base()`, which raises nothing at all
//! (`the_base_fixture_raises_nothing`): a clean 12 s capture on a 16:9
//! canvas with a destination. Asymmetric on purpose -- a 1920x1080 source
//! on a 1280x720 canvas (the aspect matches, the pixels do not, so a check
//! comparing raw sizes fails), distinct x/y everywhere, a non-unit speed
//! where a time mapping is involved.

use std::collections::BTreeSet;

use super::*;
use crate::editor::model::{AssetKind, Project, TrackKind};
use crate::editor::model_cues::{CaptionCue, CaptionPosition, CaptionSettings, EffectKind};
use crate::editor::test_support::{asset, clip, effect, minimal_project, track};
use crate::editor::{Map, Num};

fn num(v: f64) -> Num {
    Num::from_f64(v).unwrap()
}

fn int(v: i64) -> Num {
    Num::from(v)
}

fn base() -> Project {
    let mut p = minimal_project();
    p.destination.vault = "vault-1".to_string();
    p.tracks = vec![
        track("v1", TrackKind::Video, false),
        track("v2", TrackKind::Video, false),
        track("a1", TrackKind::Audio, false),
        track("a2", TrackKind::Audio, false),
    ];
    let mut cap = asset("cap", AssetKind::Video, 20_000);
    cap.width = Some(int(1920));
    cap.height = Some(int(1080));
    p.assets = vec![cap, asset("voice", AssetKind::Audio, 20_000)];
    p.clips = vec![clip("c1", "v1", "cap", 0, 0, 12_000)];
    p
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn run(p: &Project) -> Vec<CheckFinding> {
    run_checks(p, &none(), 0, &none())
}

fn codes(findings: &[CheckFinding]) -> Vec<CheckCode> {
    findings.iter().map(|f| f.code).collect()
}

fn only(findings: Vec<CheckFinding>, code: CheckCode) -> CheckFinding {
    assert_eq!(
        codes(&findings),
        vec![code],
        "expected exactly one {code:?}: {findings:#?}"
    );
    findings.into_iter().next().unwrap()
}

fn captions(enabled: bool, cues: Vec<CaptionCue>) -> CaptionSettings {
    CaptionSettings {
        enabled,
        burn_in: true,
        font_size: int(30),
        position: CaptionPosition::Bottom,
        background: true,
        cues,
        extra: Map::new(),
    }
}

fn cue(id: &str, clip_id: &str, start_ms: u64, end_ms: u64, text: &str) -> CaptionCue {
    CaptionCue {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        start_ms,
        end_ms,
        text: text.to_string(),
        extra: Map::new(),
    }
}

/// A text cue with an explicit box (x/y distinct, inside the safe area
/// unless a test moves it).
fn text_cue(id: &str, x: f64, y: f64, start_ms: u64, end_ms: u64) -> crate::editor::Effect {
    let mut e = effect(id, "c1", start_ms, end_ms);
    e.kind = EffectKind::Text;
    e.text = Some(format!("Cue {id}"));
    e.x = num(x);
    e.y = num(y);
    e.w = Some(num(0.3));
    e.h = Some(num(0.1));
    e
}

fn target(kind: TargetKind, id: &str) -> CheckTarget {
    CheckTarget {
        kind,
        id: Some(id.to_string()),
    }
}

fn project_target() -> CheckTarget {
    CheckTarget {
        kind: TargetKind::Project,
        id: None,
    }
}

#[test]
fn the_base_fixture_raises_nothing() {
    // Every test below is "base + one change": if the base raised anything
    // itself, "exactly that finding" would prove nothing.
    assert_eq!(run(&base()), Vec::new());
}

// ---- missingMedia --------------------------------------------------------

#[test]
fn missing_media_blocks_when_a_visible_clip_uses_the_file() {
    let f = only(
        run_checks(&base(), &set(&["cap"]), 0, &none()),
        CheckCode::MissingMedia,
    );
    assert_eq!(f.severity, Severity::Blocking);
    assert_eq!(f.action, Some(CheckAction::Reconnect));
    assert_eq!(f.target, target(TargetKind::Asset, "cap"));
    assert_eq!(f.id, "chk-missingMedia-cap");
    assert_eq!(
        f.message,
        "\"Asset cap\" is missing, and the render needs it. Reconnect the original file."
    );
    // Control: a missing file no clip uses is the library's business, not
    // a reason to refuse a render.
    assert_eq!(
        run_checks(&base(), &set(&["voice"]), 0, &none()),
        Vec::new()
    );
}

#[test]
fn missing_media_reaches_a_detached_audio_clip_through_its_linked_video() {
    let mut p = base();
    let mut detached = asset("cap-audio", AssetKind::Audio, 20_000);
    detached.linked_asset = Some("cap".to_string());
    p.assets.push(detached);
    // The capture's own clip is gone; only its detached sound plays.
    p.clips = vec![clip("s1", "a1", "cap-audio", 0, 0, 12_000)];
    let f = only(
        run_checks(&p, &set(&["cap"]), 0, &none()),
        CheckCode::MissingMedia,
    );
    assert_eq!(f.severity, Severity::Blocking);
    assert_eq!(f.target, target(TargetKind::Asset, "cap"));
}

#[test]
fn missing_media_on_hidden_tracks_only_warns() {
    let mut p = base();
    p.tracks[0].visible = false;
    let findings = run_checks(&p, &set(&["cap"]), 0, &none());
    let f = findings
        .iter()
        .find(|f| f.code == CheckCode::MissingMedia)
        .expect("a missing file on a hidden track is still reported");
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(
        f.message,
        "\"Asset cap\" is missing. Only hidden tracks use it, so the render skips it."
    );
}

// ---- emptyProject --------------------------------------------------------

#[test]
fn an_empty_timeline_blocks() {
    let mut p = base();
    p.clips.clear();
    let f = only(run(&p), CheckCode::EmptyProject);
    assert_eq!(f.severity, Severity::Blocking);
    assert_eq!(f.target, project_target());
    assert_eq!(f.action, None);
    assert_eq!(f.id, "chk-emptyProject-project");
    assert_eq!(
        f.message,
        "The timeline is empty. Place a clip on it before rendering."
    );
}

// ---- noDestination -------------------------------------------------------

#[test]
fn no_destination_vault_warns_and_offers_to_set_one() {
    let mut p = base();
    p.destination.vault = "  ".to_string();
    let f = only(run(&p), CheckCode::NoDestination);
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::SetDestination));
    assert_eq!(f.target, project_target());
    assert_eq!(
        f.message,
        "No vault is set to publish into. Choose where this tutorial goes."
    );
}

// ---- gap -----------------------------------------------------------------

#[test]
fn a_gap_over_one_second_on_the_top_video_track_is_noted() {
    let mut p = base();
    p.clips.push(clip("c2", "v1", "cap", 13_500, 0, 4_000));
    let f = only(run(&p), CheckCode::Gap);
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.action, Some(CheckAction::Select));
    assert_eq!(f.target, target(TargetKind::Clip, "c2"));
    assert_eq!(f.message, "No clip on v1 from 0:12.0 to 0:13.5.");
    // Control: 900 ms is under the line.
    let mut p = base();
    p.clips.push(clip("c2", "v1", "cap", 12_900, 0, 4_000));
    assert_eq!(run(&p), Vec::new());
}

#[test]
fn a_gap_uses_output_time_and_only_the_top_populated_track() {
    // c1 at speed 2 ends at 6 s of OUTPUT (12 s of source): a gap to 7.5 s.
    let mut p = base();
    p.clips[0].speed = Some(int(2));
    p.clips.push(clip("c2", "v1", "cap", 7_500, 0, 4_000));
    assert_eq!(
        only(run(&p), CheckCode::Gap).message,
        "No clip on v1 from 0:06.0 to 0:07.5."
    );
    // The same gap on a LOWER track, under a populated top track, is not it.
    let mut p = base();
    p.clips.push(clip("d1", "v2", "cap", 0, 0, 2_000));
    p.clips.push(clip("d2", "v2", "cap", 5_000, 0, 2_000));
    assert_eq!(run(&p), Vec::new());
}

#[test]
fn a_leading_gap_counts() {
    let mut p = base();
    p.clips[0].start_ms = 2_000;
    let f = only(run(&p), CheckCode::Gap);
    assert_eq!(f.target, target(TargetKind::Clip, "c1"));
    assert_eq!(f.message, "No clip on v1 from 0:00.0 to 0:02.0.");
}

// ---- allMuted ------------------------------------------------------------

#[test]
fn every_sound_silenced_warns() {
    let mut p = base();
    let mut voice = clip("s1", "a1", "voice", 0, 0, 12_000);
    voice.muted = true;
    p.clips.push(voice);
    let f = only(
        run_checks(&p, &none(), 0, &set(&["voice"])),
        CheckCode::AllMuted,
    );
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::OpenAudio));
    assert_eq!(f.target, project_target());
    assert_eq!(
        f.message,
        "Every clip with sound is muted or silenced, so the render has no audio."
    );
    // Control: the same clip audible.
    p.clips[1].muted = false;
    assert_eq!(run_checks(&p, &none(), 0, &set(&["voice"])), Vec::new());
}

#[test]
fn a_silent_master_or_a_solo_elsewhere_silences_too() {
    let mut p = base();
    p.clips.push(clip("s1", "a1", "voice", 0, 0, 12_000));
    p.master_gain = 0.0;
    assert_eq!(
        only(
            run_checks(&p, &none(), 0, &set(&["voice"])),
            CheckCode::AllMuted
        )
        .message,
        "The master level is at zero, so the render has no audio."
    );
    let mut p = base();
    p.clips.push(clip("s1", "a1", "voice", 0, 0, 12_000));
    p.tracks[3].solo = true; // a2 soloed, empty: a1 is silenced
    only(
        run_checks(&p, &none(), 0, &set(&["voice"])),
        CheckCode::AllMuted,
    );
    // A project with no sound at all is not "muted".
    assert_eq!(run(&base()), Vec::new());
}

// ---- clipping ------------------------------------------------------------

#[test]
fn overlapping_sound_summing_over_unity_may_clip() {
    let mut p = base();
    let mut a = clip("s1", "a1", "voice", 0, 0, 8_000);
    a.volume = num(0.6);
    let mut b = clip("s2", "a2", "voice", 3_000, 0, 8_000);
    b.volume = num(0.5);
    p.clips.push(a);
    p.clips.push(b);
    let f = only(
        run_checks(&p, &none(), 0, &set(&["voice"])),
        CheckCode::Clipping,
    );
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::OpenAudio));
    assert_eq!(f.target, target(TargetKind::Clip, "s2"));
    assert_eq!(
        f.message,
        "\"s2\" overlaps other sound at a combined level of 110%, which may clip."
    );
    // Control: exactly unity (0.5 + 0.5) is not over it.
    p.clips[1].volume = num(0.5);
    assert_eq!(run_checks(&p, &none(), 0, &set(&["voice"])), Vec::new());
}

// ---- excludedCaptions ----------------------------------------------------

#[test]
fn captions_that_are_turned_off_are_excluded() {
    let mut p = base();
    p.captions = Some(captions(false, vec![cue("q1", "c1", 0, 2_000, "Hello")]));
    let f = only(run(&p), CheckCode::ExcludedCaptions);
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::OpenCaptions));
    assert_eq!(
        f.message,
        "Captions are turned off, so 1 caption will not appear in the render."
    );
    // Control: the same cue with captions on.
    p.captions = Some(captions(true, vec![cue("q1", "c1", 0, 2_000, "Hello")]));
    assert_eq!(run(&p), Vec::new());
}

// ---- captionOverlap ------------------------------------------------------

#[test]
fn overlapping_captions_are_reported_on_the_later_cue() {
    let mut p = base();
    p.captions = Some(captions(
        true,
        vec![
            cue("q1", "c1", 0, 3_000, "Hello there"),
            cue("q2", "c1", 2_000, 5_000, "Second line"),
        ],
    ));
    let f = only(run(&p), CheckCode::CaptionOverlap);
    assert_eq!(f.target, target(TargetKind::Caption, "q2"));
    assert_eq!(f.action, Some(CheckAction::OpenCaptions));
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.message, "Captions 1 and 2 overlap.");
    // Control: back to back is not an overlap (half-open spans).
    p.captions = Some(captions(
        true,
        vec![
            cue("q1", "c1", 0, 3_000, "Hello there"),
            cue("q2", "c1", 3_000, 6_000, "Second line"),
        ],
    ));
    assert_eq!(run(&p), Vec::new());
}

// ---- captionDensity ------------------------------------------------------

#[test]
fn a_caption_read_faster_than_twenty_characters_a_second_is_dense() {
    // 35 characters over 3 s of SOURCE on a 2x clip: 1.5 s of output,
    // 23.3 characters a second.
    let text = "Open the settings and pick a format";
    assert_eq!(text.chars().count(), 35);
    let mut p = base();
    p.clips[0].speed = Some(int(2));
    p.captions = Some(captions(true, vec![cue("q1", "c1", 0, 3_000, text)]));
    let f = only(run(&p), CheckCode::CaptionDensity);
    assert_eq!(f.target, target(TargetKind::Caption, "q1"));
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::OpenCaptions));
    assert_eq!(f.message, "Caption 1 reads at 23.3 characters per second.");
    // Control: the same cue at 1x reads at 11.7.
    p.clips[0].speed = None;
    assert_eq!(run(&p), Vec::new());
}

// ---- pendingTake ---------------------------------------------------------

#[test]
fn an_open_webcam_take_warns() {
    let f = only(
        run_checks(&base(), &none(), 2, &none()),
        CheckCode::PendingTake,
    );
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::OpenWebcam));
    assert_eq!(f.target, project_target());
    assert_eq!(
        f.message,
        "2 webcam takes are not finished. Finish or discard them before sharing."
    );
    assert_eq!(
        only(
            run_checks(&base(), &none(), 1, &none()),
            CheckCode::PendingTake
        )
        .message,
        "1 webcam take is not finished. Finish or discard it before sharing."
    );
    assert_eq!(run_checks(&base(), &none(), 0, &none()), Vec::new());
}

// ---- transparentClip -----------------------------------------------------

#[test]
fn a_nearly_invisible_clip_warns() {
    let mut p = base();
    p.clips[0].opacity = num(0.04);
    let f = only(run(&p), CheckCode::TransparentClip);
    assert_eq!(f.target, target(TargetKind::Clip, "c1"));
    assert_eq!(f.action, Some(CheckAction::OpenLayout));
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(
        f.message,
        "\"c1\" is almost fully transparent (4% opacity)."
    );
    // Control: 0.05 is the line, and it is not under it.
    p.clips[0].opacity = num(0.05);
    assert_eq!(run(&p), Vec::new());
}

#[test]
fn a_hidden_video_track_with_clips_warns() {
    let mut p = base();
    p.clips.push(clip("d1", "v2", "cap", 0, 0, 3_000));
    p.clips.push(clip("d2", "v2", "cap", 4_000, 0, 3_000));
    p.tracks[1].visible = false;
    let f = only(run(&p), CheckCode::TransparentClip);
    assert_eq!(f.target, target(TargetKind::Track, "v2"));
    assert_eq!(f.action, Some(CheckAction::OpenLayout));
    assert_eq!(
        f.message,
        "Track v2 is hidden, so its 2 clips will not appear in the render."
    );
    // Control: a hidden track with nothing on it is nothing to review.
    let mut p = base();
    p.tracks[1].visible = false;
    assert_eq!(run(&p), Vec::new());
}

// ---- textCollision -------------------------------------------------------

#[test]
fn two_text_cues_sharing_time_and_space_collide() {
    let mut p = base();
    p.effects = vec![
        text_cue("t1", 0.1, 0.12, 0, 4_000),
        text_cue("t2", 0.2, 0.15, 2_000, 6_000),
    ];
    let f = only(run(&p), CheckCode::TextCollision);
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.target, target(TargetKind::Effect, "t2"));
    assert_eq!(f.action, Some(CheckAction::Select));
    assert_eq!(
        f.message,
        "Two text cues overlap on screen: \"Cue t1\" and \"Cue t2\"."
    );
    // Control 1: the same time, apart on screen.
    p.effects[1].x = num(0.6);
    assert_eq!(run(&p), Vec::new());
    // Control 2: the same place, apart in time.
    p.effects[1].x = num(0.2);
    p.effects[1].start_ms = 4_000;
    assert_eq!(run(&p), Vec::new());
}

// ---- privacyCover --------------------------------------------------------

#[test]
fn every_privacy_cover_is_a_warning_to_check_the_render() {
    let mut p = base();
    let mut mask = effect("m1", "c1", 1_000, 5_000);
    mask.kind = EffectKind::Mask;
    mask.x = num(0.3);
    mask.y = num(0.4);
    mask.w = Some(num(0.2));
    mask.h = Some(num(0.1));
    p.effects = vec![mask];
    let f = only(run(&p), CheckCode::PrivacyCover);
    // MUTATION CHECK (the brief): make this info-level and this goes red.
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.target, target(TargetKind::Effect, "m1"));
    assert_eq!(f.action, Some(CheckAction::Select));
    assert_eq!(
        f.message,
        "Stationary cover \u{2014} check the rendered video; originals are uncensored"
    );
    // Control: a highlight is not a cover.
    p.effects[0].kind = EffectKind::Highlight;
    assert_eq!(run(&p), Vec::new());
}

// ---- canvasReview --------------------------------------------------------

#[test]
fn a_full_frame_source_of_another_aspect_needs_a_canvas_review() {
    let mut p = base();
    p.assets[0].height = Some(int(1200)); // 16:10 on a 16:9 canvas
    let f = only(run(&p), CheckCode::CanvasReview);
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.action, Some(CheckAction::ReviewCanvas));
    assert_eq!(f.target, target(TargetKind::Clip, "c1"));
    assert_eq!(
        f.message,
        "\"Asset cap\" is 1920\u{d7}1200, which does not match the 1280\u{d7}720 canvas. Review its crop and framing."
    );
    // Control: a picture-in-picture box of that source is framed on purpose.
    p.clips[0].w = num(0.3);
    p.clips[0].h = num(0.4);
    assert_eq!(run(&p), Vec::new());
}

#[test]
fn text_outside_the_five_percent_safe_area_needs_a_canvas_review() {
    let mut p = base();
    p.effects = vec![text_cue("t1", 0.03, 0.2, 0, 3_000)];
    let f = only(run(&p), CheckCode::CanvasReview);
    assert_eq!(f.target, target(TargetKind::Effect, "t1"));
    assert_eq!(
        f.message,
        "\"Cue t1\" reaches outside the safe area. Keep text 5% inside the frame."
    );
    // Control: 0.05 in from the left, and 0.95 - 0.3 from the right.
    p.effects[0].x = num(0.05);
    assert_eq!(run(&p), Vec::new());
    p.effects[0].x = num(0.65);
    assert_eq!(run(&p), Vec::new());
    // The far edge counts too: 0.66 + 0.3 = 0.96.
    p.effects[0].x = num(0.66);
    only(run(&p), CheckCode::CanvasReview);
}

#[test]
fn a_caption_too_tall_for_the_canvas_needs_a_canvas_review() {
    // 199 characters at font 56. On the 720x720 square the lines hold 17
    // characters: 12 lines, 879 px, taller than the 648 px safe band. On
    // 16:9 the same caption wraps at 33 a line (7 lines, 529 px) and fits:
    // the canvas's WIDTH decides, so a swapped axis fails one side.
    let text = "word ".repeat(40);
    let mut settings = captions(true, vec![cue("q1", "c1", 0, 12_000, text.trim())]);
    settings.font_size = int(56);
    let mut square = base();
    square.canvas.width = 720;
    square.canvas.height = 720;
    square.assets[0].width = Some(int(1080));
    square.assets[0].height = Some(int(1080));
    square.captions = Some(settings.clone());
    let f = only(run(&square), CheckCode::CanvasReview);
    assert_eq!(f.target, target(TargetKind::Caption, "q1"));
    assert_eq!(f.action, Some(CheckAction::ReviewCanvas));
    assert_eq!(
        f.message,
        "Caption 1 is too long to fit inside the safe area at this size."
    );
    // Control: the same caption on the 16:9 canvas fits.
    let mut wide = base();
    wide.captions = Some(settings);
    assert_eq!(run(&wide), Vec::new());
    // And at font 30 it fits the square too.
    square.captions.as_mut().unwrap().font_size = int(30);
    assert_eq!(run(&square), Vec::new());
}

// ---- the wire -------------------------------------------------------------

#[test]
fn a_finding_serializes_to_the_contract_literal() {
    let f = only(
        run_checks(&base(), &set(&["cap"]), 0, &none()),
        CheckCode::MissingMedia,
    );
    let expected: serde_json::Value = serde_json::from_str(
        r#"{
            "id": "chk-missingMedia-cap",
            "severity": "blocking",
            "code": "missingMedia",
            "message": "\"Asset cap\" is missing, and the render needs it. Reconnect the original file.",
            "target": {"kind": "asset", "id": "cap"},
            "action": "reconnect"
        }"#,
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&f).unwrap(), expected);

    let mut p = base();
    p.clips.clear();
    let empty = only(run(&p), CheckCode::EmptyProject);
    let expected: serde_json::Value = serde_json::from_str(
        r#"{
            "id": "chk-emptyProject-project",
            "severity": "blocking",
            "code": "emptyProject",
            "message": "The timeline is empty. Place a clip on it before rendering.",
            "target": {"kind": "project", "id": null},
            "action": null
        }"#,
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&empty).unwrap(), expected);
}

#[test]
fn every_code_and_action_uses_the_contract_spelling() {
    let codes = [
        (CheckCode::MissingMedia, "missingMedia"),
        (CheckCode::EmptyProject, "emptyProject"),
        (CheckCode::NoDestination, "noDestination"),
        (CheckCode::Gap, "gap"),
        (CheckCode::AllMuted, "allMuted"),
        (CheckCode::Clipping, "clipping"),
        (CheckCode::ExcludedCaptions, "excludedCaptions"),
        (CheckCode::CaptionOverlap, "captionOverlap"),
        (CheckCode::CaptionDensity, "captionDensity"),
        (CheckCode::PendingTake, "pendingTake"),
        (CheckCode::TransparentClip, "transparentClip"),
        (CheckCode::TextCollision, "textCollision"),
        (CheckCode::PrivacyCover, "privacyCover"),
        (CheckCode::CanvasReview, "canvasReview"),
    ];
    for (code, name) in codes {
        assert_eq!(serde_json::to_value(code).unwrap(), serde_json::json!(name));
        assert_eq!(
            code.as_str(),
            name,
            "the id segment must be the wire spelling"
        );
    }
    let actions = [
        (CheckAction::Reconnect, "reconnect"),
        (CheckAction::Select, "select"),
        (CheckAction::OpenCaptions, "openCaptions"),
        (CheckAction::OpenLayout, "openLayout"),
        (CheckAction::OpenAudio, "openAudio"),
        (CheckAction::OpenWebcam, "openWebcam"),
        (CheckAction::ReviewCanvas, "reviewCanvas"),
        (CheckAction::SetDestination, "setDestination"),
    ];
    for (action, name) in actions {
        assert_eq!(
            serde_json::to_value(action).unwrap(),
            serde_json::json!(name)
        );
    }
}

/// A project that raises as many findings at once as the rules allow.
fn everything() -> (Project, BTreeSet<String>) {
    let mut p = base();
    p.destination.vault.clear();
    p.assets[0].height = Some(int(1200));
    p.clips[0].opacity = num(0.01);
    p.clips.push(clip("c2", "v1", "cap", 14_000, 0, 2_000));
    let mut mask = effect("m1", "c1", 0, 2_000);
    mask.kind = EffectKind::Mask;
    p.effects = vec![
        mask,
        text_cue("t1", 0.01, 0.1, 0, 3_000),
        text_cue("t2", 0.02, 0.12, 0, 3_000),
    ];
    p.captions = Some(captions(
        false,
        vec![
            cue(
                "q1",
                "c1",
                0,
                1_000,
                "A caption that is far too long to read",
            ),
            cue("q2", "c1", 500, 2_000, "Another"),
        ],
    ));
    (p, set(&["cap"]))
}

#[test]
fn no_quality_score_is_ever_emitted() {
    // SCREENS 07: "actionable findings rather than an invented quality
    // score". Nothing in the reply may be a score, at any depth.
    fn has_score(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(map) => map
                .iter()
                .any(|(k, v)| k.to_lowercase().contains("score") || has_score(v)),
            serde_json::Value::Array(items) => items.iter().any(has_score),
            _ => false,
        }
    }
    let (p, missing) = everything();
    let findings = run_checks(&p, &missing, 1, &none());
    assert!(
        findings.len() >= 10,
        "the fixture must exercise many rules: {findings:#?}"
    );
    let value = serde_json::to_value(&findings).unwrap();
    assert!(!has_score(&value), "a score leaked into the reply: {value}");
}

#[test]
fn ids_are_unique_and_stable_across_runs() {
    let (p, missing) = everything();
    let a = run_checks(&p, &missing, 1, &none());
    let b = run_checks(&p, &missing, 1, &none());
    assert_eq!(a, b, "the same project must read the same way twice");
    let ids: BTreeSet<&str> = a.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids.len(), a.len(), "duplicate ids: {a:#?}");
    for f in &a {
        let target = f.target.id.as_deref().unwrap_or("project");
        assert_eq!(f.id, format!("chk-{}-{}", f.code.as_str(), target));
    }
}
