//! Tests for `cues.rs` (Task 34; F-27–F-33). Sibling file, the
//! `clips.rs`/`clips_tests.rs` precedent.
//!
//! One fixture throughout: a single clip `c1` on its own video track, at a
//! non-zero, non-round output position (`start_ms 700`) playing a
//! non-zero-based source range (`in_ms 1_000, out_ms 9_000`, 8000 ms of
//! source) -- the "fixture flaw" rule, so a cue's SOURCE time is never
//! accidentally equal to its OUTPUT time and a swapped one would show.

use super::*;
use crate::editor::commands::clips::{move_clips, split_clip};
use crate::editor::commands::layout::set_speed;
use crate::editor::commands::payloads::{
    AddEffectPayload, ArrowEffectProps, HighlightEffectProps, MaskEffectProps, MoveClipsPayload,
    RemoveEffectPayload, SetSpeedPayload, SplitClipPayload, SpotlightEffectProps, StepEffectProps,
    TextEffectProps, UpdateEffectPayload, ZoomEffectProps,
};
use crate::editor::error::EditorErrorCode;
use crate::editor::model::{AssetKind, Clip, TrackKind};
use crate::editor::model_cues::EffectKind;
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::time::{cue_output_span, ClipSpan};
use crate::editor::{validate_project, Num};

fn n(v: f64) -> Num {
    Num::from_f64(v).expect("finite test fixture value")
}

fn project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("av", AssetKind::Video, 60_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.clips.push(clip("c1", "v1", "av", 700, 1_000, 9_000));
    validate_project(&p).expect("the cue fixture itself is valid");
    p
}

fn by_id<'a>(p: &'a Project, id: &str) -> &'a Clip {
    p.clips.iter().find(|c| c.id == id).unwrap()
}

fn span(c: &Clip) -> ClipSpan {
    ClipSpan {
        start_ms: c.start_ms,
        in_ms: c.in_ms,
        out_ms: c.out_ms,
        speed: c.speed.as_ref().map_or(1.0, |s| s.as_f64().unwrap()),
    }
}

fn add(
    p: &Project,
    clip_id: &str,
    kind: EffectKind,
    start_ms: u64,
    end_ms: u64,
    props: EffectProps,
) -> Result<(Project, String), EditorError> {
    add_effect(
        p,
        &AddEffectPayload {
            clip_id: clip_id.into(),
            kind,
            start_ms,
            end_ms,
            props,
        },
    )
}

fn refusal(result: Result<(Project, String), EditorError>) -> EditorError {
    let err = result.expect_err("the command must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
    err
}

// ---- the brief's named tests --------------------------------------------

/// Table of 7: every kind's `props` may be sent fully empty and still
/// produce a `validate_project`-clean effect, because `default_shell`
/// supplies a value for every field that kind carries (the brief's own
/// "Defaults per kind when omitted") -- and each kind's OWN defining
/// field(s) are the ones present, not some other kind's.
#[test]
fn each_kind_requires_its_props() {
    let p = project();
    let cases: [(EffectKind, EffectProps); 7] = [
        (
            EffectKind::Text,
            EffectProps::Text(TextEffectProps::default()),
        ),
        (
            EffectKind::Arrow,
            EffectProps::Arrow(ArrowEffectProps::default()),
        ),
        (
            EffectKind::Highlight,
            EffectProps::Highlight(HighlightEffectProps::default()),
        ),
        (
            EffectKind::Spotlight,
            EffectProps::Spotlight(SpotlightEffectProps::default()),
        ),
        (
            EffectKind::Zoom,
            EffectProps::Zoom(ZoomEffectProps::default()),
        ),
        (
            EffectKind::Step,
            EffectProps::Step(StepEffectProps::default()),
        ),
        (
            EffectKind::Mask,
            EffectProps::Mask(MaskEffectProps::default()),
        ),
    ];
    let mut checked = 0;
    for (kind, props) in cases {
        let (candidate, label) = add(&p, "c1", kind, 2_000, 3_000, props)
            .unwrap_or_else(|e| panic!("{kind:?}: addEffect with empty props must succeed: {e:?}"));
        assert_eq!(label, "Add effect");
        validate_project(&candidate).unwrap_or_else(|e| {
            panic!("{kind:?}: the defaulted effect must pass validate_project: {e:?}")
        });
        let effect = candidate.effects.last().expect("the effect was just added");
        assert_eq!(effect.kind, kind);
        assert_eq!(effect.color, "#ffd279", "{kind:?}: default colour");
        match kind {
            EffectKind::Text => {
                assert!(effect.text.is_some() && effect.w.is_some() && effect.h.is_some());
                assert!(effect.font_size.is_some() && effect.background.is_some());
            }
            EffectKind::Arrow => {
                assert!(effect.x2.is_some() && effect.y2.is_some() && effect.stroke.is_some());
            }
            EffectKind::Highlight => {
                assert!(effect.w.is_some() && effect.h.is_some() && effect.stroke.is_some());
            }
            EffectKind::Spotlight => {
                assert!(effect.w.is_some() && effect.h.is_some() && effect.dim.is_some());
            }
            EffectKind::Zoom => {
                assert!(effect.factor.is_some() && effect.easing.is_some());
            }
            EffectKind::Step => {
                assert!(effect.number.is_some() && effect.text.is_some());
            }
            EffectKind::Mask => {
                assert!(effect.w.is_some() && effect.h.is_some());
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 7, "all seven kinds must be exercised");
}

/// A11: a cue's `start_ms`/`end_ms` are the wire's SOURCE times, stored
/// verbatim -- never the clip's OUTPUT position -- and `moveClips` (which
/// only ever touches `Clip.start_ms`) leaves them byte-identical.
#[test]
fn cue_times_are_source_times_and_survive_a_move() {
    let p = project();
    let (p, _) = add(
        &p,
        "c1",
        EffectKind::Highlight,
        1_500,
        2_500,
        EffectProps::Highlight(HighlightEffectProps::default()),
    )
    .unwrap();
    let effect_id = p.effects[0].id.clone();

    // Stored verbatim: NOT `output_at(1_500)` (which would be 1_200 on this
    // fixture's c1), and not clamped/shifted in any way.
    assert_eq!(p.effects[0].start_ms, 1_500);
    assert_eq!(p.effects[0].end_ms, 2_500);
    assert_eq!(
        by_id(&p, "c1").start_ms,
        700,
        "sanity: clip has not moved yet"
    );

    let (moved, _) = move_clips(
        &p,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: 1_000,
            track_id: None,
        },
    )
    .unwrap();
    validate_project(&moved).expect("a plain move is a valid project");
    assert_eq!(by_id(&moved, "c1").start_ms, 1_700, "the clip did move");

    let after = moved.effects.iter().find(|e| e.id == effect_id).unwrap();
    assert_eq!(
        after.start_ms, 1_500,
        "the cue's source start must not move"
    );
    assert_eq!(after.end_ms, 2_500, "the cue's source end must not move");
}

/// `time::cue_output_span` on the SAME source span, before and after
/// `setSpeed{speed: 2}`, must halve -- the brief's own worked example.
#[test]
fn cue_output_span_follows_speed() {
    let p = project();
    let (p, _) = add(
        &p,
        "c1",
        EffectKind::Highlight,
        2_000,
        4_000,
        EffectProps::Highlight(HighlightEffectProps::default()),
    )
    .unwrap();
    let cue = (p.effects[0].start_ms, p.effects[0].end_ms);

    let before = cue_output_span(&span(by_id(&p, "c1")), cue.0, cue.1)
        .expect("the cue sits inside c1's source range");

    let (sped, _) = set_speed(
        &p,
        &SetSpeedPayload {
            clip_id: "c1".into(),
            speed: n(2.0),
            preserve_pitch: false,
        },
    )
    .unwrap();
    validate_project(&sped).expect("a speed change is a valid project");
    // The cue itself is untouched by the speed change (Task 7's own claim).
    assert_eq!(sped.effects[0].start_ms, cue.0);
    assert_eq!(sped.effects[0].end_ms, cue.1);

    let after = cue_output_span(&span(by_id(&sped, "c1")), cue.0, cue.1)
        .expect("the cue still sits inside c1's (unchanged) source range");

    let before_len = before.1 - before.0;
    let after_len = after.1 - after.0;
    assert_eq!(before_len, 2_000);
    assert_eq!(
        after_len,
        before_len / 2,
        "at 2x speed the cue's OUTPUT span must halve"
    );
}

/// The brief's "startMs/endMs ... inside the clip's source range": a cue
/// starting before `in_ms` or ending after `out_ms` is refused before it is
/// ever created.
#[test]
fn cue_outside_its_clip_source_range_is_refused() {
    let p = project();

    let err = refusal(add(
        &p,
        "c1",
        EffectKind::Highlight,
        500, // c1's in_ms is 1_000
        2_000,
        EffectProps::Highlight(HighlightEffectProps::default()),
    ));
    assert!(err.message.contains("source range"), "{}", err.message);

    let err = refusal(add(
        &p,
        "c1",
        EffectKind::Highlight,
        8_000,
        9_500, // c1's out_ms is 9_000
        EffectProps::Highlight(HighlightEffectProps::default()),
    ));
    assert!(err.message.contains("source range"), "{}", err.message);
}

/// Re-asserts `splitClip`'s already-shipped cue reassignment
/// (`cue_follow::split_effects`) with an ARROW effect specifically, to
/// cover PROPS copying: `x2`/`y2`/`stroke` (fields no OTHER kind in this
/// suite's fixtures carries) must survive onto BOTH halves untouched.
#[test]
fn split_through_a_cue_yields_two_cues() {
    let p = project();
    let (p, _) = add(
        &p,
        "c1",
        EffectKind::Arrow,
        2_000,
        3_000,
        EffectProps::Arrow(ArrowEffectProps {
            x: Some(n(0.2)),
            y: Some(n(0.3)),
            x2: Some(n(0.9)),
            y2: Some(n(0.05)),
            color: Some("#112233".into()),
            stroke: Some(Num::from(7)),
        }),
    )
    .unwrap();

    // c1: start_ms 700, in_ms 1_000, out_ms 9_000, speed 1x -> source_at(2_200) = 2_500,
    // which straddles the arrow's source span [2_000, 3_000).
    let (candidate, _) = split_clip(
        &p,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 2_200,
        },
    )
    .unwrap();
    validate_project(&candidate).expect("a split through a cue is a valid project");

    let right_id = candidate
        .clips
        .iter()
        .map(|c| c.id.as_str())
        .find(|id| *id != "c1")
        .expect("split_clip must have minted a second clip")
        .to_string();

    assert_eq!(candidate.effects.len(), 2, "the arrow must split into two");
    let left = candidate
        .effects
        .iter()
        .find(|e| e.clip_id == "c1")
        .unwrap();
    let right = candidate
        .effects
        .iter()
        .find(|e| e.clip_id == right_id)
        .unwrap();

    assert_eq!(
        (left.start_ms, left.end_ms),
        (2_000, 2_500),
        "left half clamps at the cut"
    );
    assert_eq!(
        (right.start_ms, right.end_ms),
        (2_500, 3_000),
        "right half starts at the cut"
    );

    for half in [left, right] {
        assert_eq!(half.kind, EffectKind::Arrow);
        assert_eq!(half.x2, Some(n(0.9)), "x2 must copy onto both halves");
        assert_eq!(half.y2, Some(n(0.05)), "y2 must copy onto both halves");
        assert_eq!(half.color, "#112233", "color must copy onto both halves");
        assert_eq!(
            half.stroke,
            Some(Num::from(7)),
            "stroke must copy onto both halves"
        );
    }
}

// ---- updateEffect / removeEffect (beyond the five named tests, covering
// the two commands the named tests don't otherwise exercise) -------------

#[test]
fn update_effect_patches_only_the_fields_it_carries() {
    let p = project();
    let (p, _) = add(
        &p,
        "c1",
        EffectKind::Text,
        2_000,
        3_000,
        EffectProps::Text(TextEffectProps {
            text: Some("Original".into()),
            ..Default::default()
        }),
    )
    .unwrap();
    let effect_id = p.effects[0].id.clone();
    let original_w = p.effects[0].w.clone();

    let (updated, label) = update_effect(
        &p,
        &UpdateEffectPayload {
            effect_id: effect_id.clone(),
            start_ms: Some(2_200),
            end_ms: None,
            props: Some(serde_json::json!({"text": "Changed"})),
        },
    )
    .unwrap();
    assert_eq!(label, "Update effect");
    let effect = updated.effects.iter().find(|e| e.id == effect_id).unwrap();
    assert_eq!(effect.start_ms, 2_200, "startMs was patched");
    assert_eq!(effect.end_ms, 3_000, "endMs was left alone");
    assert_eq!(effect.text.as_deref(), Some("Changed"));
    assert_eq!(
        effect.w, original_w,
        "an omitted field keeps its CURRENT value"
    );
    validate_project(&updated).unwrap();
}

#[test]
fn update_effect_refuses_a_wrong_kind_props_field() {
    let p = project();
    let (p, _) = add(
        &p,
        "c1",
        EffectKind::Text,
        2_000,
        3_000,
        EffectProps::Text(TextEffectProps::default()),
    )
    .unwrap();
    let effect_id = p.effects[0].id.clone();

    // `x2` belongs to `arrow`, not `text` -- deny_unknown_fields must reject it.
    refusal(update_effect(
        &p,
        &UpdateEffectPayload {
            effect_id,
            start_ms: None,
            end_ms: None,
            props: Some(serde_json::json!({"x2": 0.5})),
        },
    ));
}

#[test]
fn remove_effect_deletes_it() {
    let p = project();
    let (p, _) = add(
        &p,
        "c1",
        EffectKind::Mask,
        2_000,
        3_000,
        EffectProps::Mask(MaskEffectProps::default()),
    )
    .unwrap();
    let effect_id = p.effects[0].id.clone();

    let (candidate, label) = remove_effect(
        &p,
        &RemoveEffectPayload {
            effect_id: effect_id.clone(),
        },
    )
    .unwrap();
    assert_eq!(label, "Remove effect");
    assert!(candidate.effects.iter().all(|e| e.id != effect_id));
    validate_project(&candidate).unwrap();

    refusal(remove_effect(
        &candidate,
        &RemoveEffectPayload { effect_id },
    ));
}
