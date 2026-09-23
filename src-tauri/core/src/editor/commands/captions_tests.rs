//! Tests for `captions.rs` (Task 36; F-34, F-35). Sibling file, the
//! `cues.rs`/`cues_tests.rs` precedent.
//!
//! One fixture throughout, asymmetric on purpose (the "fixture flaw"
//! rule): clip `c1` sits at output `700`, plays source `[1_000, 9_000)`
//! and runs at speed 2 -- so a cue's SOURCE time, its clip-relative OUTPUT
//! time and its timeline OUTPUT time are three different numbers, and
//! confusing any two of them shows.

use super::*;
use crate::editor::captions_io::ParsedCue;
use crate::editor::commands::payloads::{
    AddCaptionPayload, ImportCaptionsPayload, RemoveCaptionsPayload, SetCaptionSettingsPayload,
    SplitCaptionPayload, UpdateCaptionPayload,
};
use crate::editor::error::EditorErrorCode;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::model_cues::CaptionPosition;
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::time::{cue_output_span, ClipSpan};
use crate::editor::{is_valid_id, validate_project, Map, Num};

fn project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("av", AssetKind::Video, 60_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    let mut c1 = clip("c1", "v1", "av", 700, 1_000, 9_000);
    c1.speed = Some(Num::from_f64(2.0).unwrap());
    p.clips.push(c1);
    validate_project(&p).expect("the caption fixture itself is valid");
    p
}

fn span() -> ClipSpan {
    ClipSpan {
        start_ms: 700,
        in_ms: 1_000,
        out_ms: 9_000,
        speed: 2.0,
    }
}

fn with_cue(p: &mut Project, id: &str, start_ms: u64, end_ms: u64, text: &str) {
    p.captions
        .get_or_insert_with(default_settings)
        .cues
        .push(CaptionCue {
            id: id.into(),
            clip_id: "c1".into(),
            start_ms,
            end_ms,
            text: text.into(),
            extra: Map::new(),
        });
}

fn cues(p: &Project) -> &[CaptionCue] {
    &p.captions.as_ref().expect("captions exist").cues
}

fn refusal(result: Result<(Project, String), EditorError>) -> EditorError {
    let err = result.expect_err("the command must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
    err
}

fn parsed(start_ms: u64, end_ms: u64, text: &str) -> ParsedCue {
    ParsedCue {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

// ---- the brief's named tests --------------------------------------------

#[test]
fn split_caption_keeps_both_halves_on_the_clip() {
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 6_000, "one two three four");
    let (after, label) = split_caption(
        &p,
        &SplitCaptionPayload {
            caption_id: "cap1".into(),
            at_ms: 3_000, // SOURCE time, a quarter of the way in
        },
    )
    .unwrap();
    validate_project(&after).unwrap();
    assert_eq!(label, "Split caption");
    let list = cues(&after);
    assert_eq!(list.len(), 2);
    let left = list.iter().find(|c| c.id == "cap1").unwrap();
    let right = list.iter().find(|c| c.id != "cap1").unwrap();
    assert_eq!(left.clip_id, "c1");
    assert_eq!(right.clip_id, "c1", "both halves stay on the SAME clip");
    assert_eq!((left.start_ms, left.end_ms), (2_000, 3_000));
    assert_eq!((right.start_ms, right.end_ms), (3_000, 6_000));
    // The words divide in the same proportion as the time: a quarter of
    // four words is one.
    assert_eq!(left.text, "one");
    assert_eq!(right.text, "two three four");
}

#[test]
fn import_converts_output_to_source_time_at_speed_two() {
    // Imported times are OUTPUT times relative to the clip's own start.
    // At speed 2 the clip shows 4000 ms of output, so:
    // - [500, 1500) plays source [1000 + 1000, 1000 + 3000) = [2000, 4000);
    // - [3500, 4500) starts inside and is cut at the clip's source end;
    // - [4000, 5000) starts at the clip's end: outside, reported skipped.
    let p = project();
    let plan = plan_caption_import(
        &p,
        "c1",
        &[
            parsed(500, 1_500, "A"),
            parsed(3_500, 4_500, "B"),
            parsed(4_000, 5_000, "C"),
        ],
    )
    .unwrap();
    assert_eq!(plan.skipped, 1);
    let spans: Vec<(u64, u64, &str)> = plan
        .cues
        .iter()
        .map(|c| (c.start_ms, c.end_ms, c.text.as_str()))
        .collect();
    assert_eq!(spans, vec![(2_000, 4_000, "A"), (8_000, 9_000, "B")]);

    let (after, _) = import_captions(
        &p,
        &ImportCaptionsPayload {
            clip_id: "c1".into(),
            cues: plan.cues,
            replace: false,
        },
    )
    .unwrap();
    validate_project(&after).unwrap();
    // And back: the first cue appears on the TIMELINE exactly where the
    // file said, shifted by the clip's own start (700).
    let first = &cues(&after)[0];
    assert_eq!(
        cue_output_span(&span(), first.start_ms, first.end_ms),
        Some((1_200, 2_200))
    );
}

// ---- setCaptionSettings ---------------------------------------------------

fn settings(font: Option<i64>, position: Option<CaptionPosition>) -> SetCaptionSettingsPayload {
    SetCaptionSettingsPayload {
        enabled: None,
        burn_in: None,
        font_size: font.map(Num::from),
        position,
        background: None,
    }
}

#[test]
fn set_caption_settings_creates_the_defaults_then_applies() {
    let (after, label) =
        set_caption_settings(&project(), &settings(Some(40), Some(CaptionPosition::Top))).unwrap();
    validate_project(&after).unwrap();
    assert_eq!(label, "Caption settings");
    let s = after.captions.unwrap();
    assert_eq!(s.font_size, Num::from(40));
    assert_eq!(s.position, CaptionPosition::Top);
    // Untouched fields keep the reference editor's defaults.
    assert!(s.enabled && s.burn_in && s.background);
}

#[test]
fn set_caption_settings_bounds_the_font_size() {
    for bad in [17, 57] {
        let err = refusal(set_caption_settings(&project(), &settings(Some(bad), None)));
        assert!(
            err.message.contains("18") && err.message.contains("56"),
            "{}",
            err.message
        );
    }
    for good in [18, 56] {
        assert!(set_caption_settings(&project(), &settings(Some(good), None)).is_ok());
    }
    let fractional = SetCaptionSettingsPayload {
        font_size: Some(Num::from_f64(30.5).unwrap()),
        ..settings(None, None)
    };
    assert!(set_caption_settings(&project(), &fractional).is_ok());
}

#[test]
fn set_caption_settings_needs_a_field() {
    refusal(set_caption_settings(&project(), &settings(None, None)));
}

#[test]
fn set_caption_settings_keeps_existing_cues() {
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 3_000, "Hi");
    let payload = SetCaptionSettingsPayload {
        burn_in: Some(false),
        ..settings(None, None)
    };
    let (after, _) = set_caption_settings(&p, &payload).unwrap();
    let s = after.captions.unwrap();
    assert!(!s.burn_in);
    assert_eq!(s.cues.len(), 1);
}

// ---- addCaption / updateCaption / removeCaptions --------------------------

fn add(start_ms: u64, end_ms: u64, text: &str) -> AddCaptionPayload {
    AddCaptionPayload {
        clip_id: "c1".into(),
        start_ms,
        end_ms,
        text: text.into(),
    }
}

#[test]
fn add_caption_stores_source_time_and_trimmed_text() {
    let (after, label) = add_caption(&project(), &add(2_000, 3_500, "  Hello  ")).unwrap();
    validate_project(&after).unwrap();
    assert_eq!(label, "Add caption");
    let cue = &cues(&after)[0];
    assert_eq!(
        (cue.clip_id.as_str(), cue.start_ms, cue.end_ms),
        ("c1", 2_000, 3_500)
    );
    assert_eq!(cue.text, "Hello");
    assert!(is_valid_id(&cue.id));
    // A project with no caption settings gets the defaults: captions ON.
    assert!(after.captions.as_ref().unwrap().enabled);
}

#[test]
fn add_caption_refuses_a_span_outside_the_clip_source() {
    refusal(add_caption(&project(), &add(500, 2_000, "Early")));
    refusal(add_caption(&project(), &add(8_000, 9_001, "Late")));
    refusal(add_caption(&project(), &add(3_000, 3_000, "Empty span")));
    assert!(add_caption(&project(), &add(1_000, 9_000, "Whole")).is_ok());
}

#[test]
fn add_caption_refuses_empty_and_oversized_text() {
    refusal(add_caption(&project(), &add(2_000, 3_000, "   ")));
    let err = refusal(add_caption(
        &project(),
        &add(2_000, 3_000, &"x".repeat(501)),
    ));
    assert!(err.message.contains("500"), "{}", err.message);
    assert!(add_caption(&project(), &add(2_000, 3_000, &"x".repeat(500))).is_ok());
}

#[test]
fn add_caption_refuses_a_locked_track_and_an_unknown_clip() {
    let mut p = project();
    p.tracks[0].locked = true;
    refusal(add_caption(&p, &add(2_000, 3_000, "Hi")));
    let unknown = AddCaptionPayload {
        clip_id: "nope".into(),
        ..add(2_000, 3_000, "Hi")
    };
    refusal(add_caption(&project(), &unknown));
}

#[test]
fn add_caption_refuses_past_the_caption_limit() {
    let mut p = project();
    for i in 0..limits::MAX_CAPTIONS {
        with_cue(&mut p, &format!("cap{i}"), 2_000, 3_000, "Hi");
    }
    let err = refusal(add_caption(&p, &add(2_000, 3_000, "One more")));
    assert!(err.message.contains("2000"), "{}", err.message);
}

#[test]
fn update_caption_changes_only_what_it_names() {
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 3_000, "Teh typo");
    let payload = UpdateCaptionPayload {
        caption_id: "cap1".into(),
        start_ms: None,
        end_ms: Some(4_000),
        text: Some("The fix".into()),
    };
    let (after, label) = update_caption(&p, &payload).unwrap();
    validate_project(&after).unwrap();
    assert_eq!(label, "Edit caption");
    let cue = &cues(&after)[0];
    assert_eq!(
        (cue.start_ms, cue.end_ms, cue.text.as_str()),
        (2_000, 4_000, "The fix")
    );
}

#[test]
fn update_caption_refuses_a_bad_span_an_empty_patch_and_an_unknown_id() {
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 3_000, "Hi");
    let patch = |start_ms, end_ms, text: Option<&str>| UpdateCaptionPayload {
        caption_id: "cap1".into(),
        start_ms,
        end_ms,
        text: text.map(str::to_string),
    };
    refusal(update_caption(&p, &patch(Some(3_000), None, None)));
    refusal(update_caption(&p, &patch(None, Some(9_500), None)));
    refusal(update_caption(&p, &patch(None, None, None)));
    refusal(update_caption(&p, &patch(None, None, Some(" "))));
    let unknown = UpdateCaptionPayload {
        caption_id: "nope".into(),
        ..patch(None, None, Some("x"))
    };
    refusal(update_caption(&p, &unknown));
}

#[test]
fn remove_captions_removes_every_named_cue() {
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 3_000, "A");
    with_cue(&mut p, "cap2", 3_000, 4_000, "B");
    with_cue(&mut p, "cap3", 4_000, 5_000, "C");
    let (after, label) = remove_captions(
        &p,
        &RemoveCaptionsPayload {
            caption_ids: vec!["cap1".into(), "cap3".into()],
        },
    )
    .unwrap();
    assert_eq!(label, "Delete captions");
    let ids: Vec<&str> = cues(&after).iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["cap2"]);
    // Unknown id, or nothing named: refused, never a silent no-op.
    refusal(remove_captions(
        &p,
        &RemoveCaptionsPayload {
            caption_ids: vec!["cap1".into(), "nope".into()],
        },
    ));
    refusal(remove_captions(
        &p,
        &RemoveCaptionsPayload {
            caption_ids: vec![],
        },
    ));
}

// ---- splitCaption ----------------------------------------------------------

#[test]
fn split_caption_refuses_a_boundary_split() {
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 6_000, "one two");
    for at_ms in [2_000, 6_000, 1_500, 7_000] {
        let err = refusal(split_caption(
            &p,
            &SplitCaptionPayload {
                caption_id: "cap1".into(),
                at_ms,
            },
        ));
        assert!(err.message.contains("inside"), "{at_ms}: {}", err.message);
    }
}

#[test]
fn split_caption_of_one_word_keeps_it_on_both_halves() {
    // One word cannot be divided; dropping it from either half would leave
    // an empty caption -- which the commands refuse (`checked_text`) and,
    // since GAP-179 closed, `validate_project` does too -- so both keep it
    // and the user edits whichever is wrong.
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 6_000, "Hello");
    let (after, _) = split_caption(
        &p,
        &SplitCaptionPayload {
            caption_id: "cap1".into(),
            at_ms: 5_900,
        },
    )
    .unwrap();
    assert!(cues(&after).iter().all(|c| c.text == "Hello"));
}

#[test]
fn split_caption_never_leaves_a_half_without_words() {
    // At 95 % of the time a proportional share would give the right half
    // no word at all; each half keeps at least one.
    let mut p = project();
    with_cue(&mut p, "cap1", 2_000, 6_000, "one two three");
    let (after, _) = split_caption(
        &p,
        &SplitCaptionPayload {
            caption_id: "cap1".into(),
            at_ms: 5_800,
        },
    )
    .unwrap();
    let texts: Vec<&str> = cues(&after).iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["one two", "three"]);
}

// ---- ImportCaptions -----------------------------------------------------------

#[test]
fn import_replace_drops_only_this_clips_cues() {
    let mut p = project();
    p.clips.push(clip("c2", "v1", "av", 20_000, 0, 5_000));
    with_cue(&mut p, "old", 2_000, 3_000, "Old");
    p.captions.as_mut().unwrap().cues.push(CaptionCue {
        id: "other".into(),
        clip_id: "c2".into(),
        start_ms: 100,
        end_ms: 200,
        text: "Other clip".into(),
        extra: Map::new(),
    });
    let plan = plan_caption_import(&p, "c1", &[parsed(0, 500, "New")]).unwrap();
    let (after, label) = import_captions(
        &p,
        &ImportCaptionsPayload {
            clip_id: "c1".into(),
            cues: plan.cues,
            replace: true,
        },
    )
    .unwrap();
    validate_project(&after).unwrap();
    assert_eq!(label, "Replace captions");
    let texts: Vec<&str> = cues(&after).iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["Other clip", "New"]);
}

#[test]
fn import_without_replace_appends_and_is_one_step() {
    let mut p = project();
    with_cue(&mut p, "old", 2_000, 3_000, "Old");
    let plan =
        plan_caption_import(&p, "c1", &[parsed(0, 500, "A"), parsed(600, 900, "B")]).unwrap();
    let (after, label) = import_captions(
        &p,
        &ImportCaptionsPayload {
            clip_id: "c1".into(),
            cues: plan.cues,
            replace: false,
        },
    )
    .unwrap();
    assert_eq!(label, "Import captions");
    assert_eq!(cues(&after).len(), 3);
}

#[test]
fn import_with_nothing_inside_the_clip_is_refused_and_says_why() {
    let err = plan_caption_import(&project(), "c1", &[parsed(4_000, 5_000, "Late")]).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("00:00"), "{}", err.message);
}

#[test]
fn import_refuses_a_locked_track_and_the_caption_limit() {
    let mut locked = project();
    locked.tracks[0].locked = true;
    assert!(plan_caption_import(&locked, "c1", &[parsed(0, 500, "A")]).is_err());

    let mut full = project();
    for i in 0..limits::MAX_CAPTIONS {
        with_cue(&mut full, &format!("cap{i}"), 2_000, 3_000, "Hi");
    }
    let plan = plan_caption_import(&project(), "c1", &[parsed(0, 500, "A")]).unwrap();
    refusal(import_captions(
        &full,
        &ImportCaptionsPayload {
            clip_id: "c1".into(),
            cues: plan.cues,
            replace: false,
        },
    ));
}

#[test]
fn import_skips_a_cue_that_rounds_to_nothing() {
    // At speed 0.25 a 1 ms output cue is a quarter of a source ms: both
    // ends round to the same instant, so it cannot be stored -- skipped
    // and counted, not refused and not silently dropped.
    let mut p = project();
    p.clips[0].speed = Some(Num::from_f64(0.25).unwrap());
    let plan =
        plan_caption_import(&p, "c1", &[parsed(0, 1, "Blink"), parsed(0, 400, "Ok")]).unwrap();
    assert_eq!(plan.skipped, 1);
    assert_eq!(plan.cues.len(), 1);
}
