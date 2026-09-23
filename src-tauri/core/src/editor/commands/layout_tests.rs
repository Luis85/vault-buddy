//! Tests for `layout.rs` (Task 31; F-16, F-21, F-23). Sibling file, the
//! `clips.rs`/`clips_tests.rs` precedent.
//!
//! Fixtures are asymmetric on purpose (the "fixture flaw" rule): the speed
//! clip starts at a non-zero output time with a non-zero source in-point,
//! cues sit off-centre, and the layout values are four DIFFERENT numbers
//! (0.61/0.23/0.27/0.19, A10) so a swapped axis or a dropped field shows.

use super::*;
use crate::editor::commands::clips::clip_end;
use crate::editor::commands::payloads::AddTransitionPayload;
use crate::editor::commands::{apply, EditorCommand};
use crate::editor::error::EditorErrorCode;
use crate::editor::model::{AssetKind, Fit, FrameShape, Rotation, TrackKind};
use crate::editor::model_cues::{Marker, TransitionKind};
use crate::editor::session::{EditorSession, ExecuteRequest};
use crate::editor::test_support::{asset, clip, effect, minimal_project, no_context, track};
use crate::editor::time::{cue_output_span, ClipSpan};
use crate::editor::{limits, validate_project, Map, Num};

fn n(v: f64) -> Num {
    Num::from_f64(v).expect("finite test fixture value")
}

fn f(v: &Num) -> f64 {
    v.as_f64().unwrap()
}

/// v1: c1 [700, 4700) plays source [1000, 5000) at 1x, then c2 at 5200;
/// v2 (locked): l1; a1 (audio): s1. c1 carries a highlight over source
/// [1500, 2500) and a marker at source 3000.
fn project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("av", AssetKind::Video, 60_000));
    p.assets.push(asset("aa", AssetKind::Audio, 60_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("v2", TrackKind::Video, true));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.clips.push(clip("c1", "v1", "av", 700, 1_000, 5_000));
    p.clips.push(clip("c2", "v1", "av", 5_200, 0, 1_500));
    p.clips.push(clip("l1", "v2", "av", 0, 0, 2_000));
    p.clips.push(clip("s1", "a1", "aa", 0, 0, 2_000));
    p.effects.push(effect("e1", "c1", 1_500, 2_500));
    p.markers.push(Marker {
        id: "m1".into(),
        clip_id: "c1".into(),
        source_ms: 3_000,
        title: "M".into(),
        extra: Map::new(),
    });
    validate_project(&p).expect("the layout fixture itself is valid");
    p
}

fn speed(p: &Project, clip_id: &str, value: f64) -> Result<(Project, String), EditorError> {
    set_speed(
        p,
        &SetSpeedPayload {
            clip_id: clip_id.into(),
            speed: n(value),
            preserve_pitch: true,
        },
    )
}

fn by_id<'a>(p: &'a Project, id: &str) -> &'a Clip {
    p.clips.iter().find(|c| c.id == id).unwrap()
}

fn span(c: &Clip) -> ClipSpan {
    ClipSpan {
        start_ms: c.start_ms,
        in_ms: c.in_ms,
        out_ms: c.out_ms,
        speed: c.speed.as_ref().map_or(1.0, f),
    }
}

fn empty_layout(clip_ids: &[&str]) -> SetLayoutPayload {
    SetLayoutPayload {
        clip_ids: clip_ids.iter().map(|s| s.to_string()).collect(),
        x: None,
        y: None,
        w: None,
        h: None,
        opacity: None,
        fit: None,
        frame_shape: None,
        rotation: None,
        mirror: None,
        flip_y: None,
        crop_zoom: None,
        crop_x: None,
        crop_y: None,
    }
}

/// A10's asymmetric box plus every transform field set to a non-default.
fn full_layout(clip_ids: &[&str]) -> SetLayoutPayload {
    SetLayoutPayload {
        x: Some(n(0.61)),
        y: Some(n(0.23)),
        w: Some(n(0.27)),
        h: Some(n(0.19)),
        opacity: Some(n(0.45)),
        fit: Some(Fit::Cover),
        frame_shape: Some(FrameShape::Rounded),
        rotation: Some(Rotation::Deg90),
        mirror: Some(true),
        flip_y: Some(true),
        crop_zoom: Some(n(1.7)),
        crop_x: Some(n(0.35)),
        crop_y: Some(n(0.8)),
        ..empty_layout(clip_ids)
    }
}

fn assert_full_layout(c: &Clip) {
    assert_eq!(
        [f(&c.x), f(&c.y), f(&c.w), f(&c.h)],
        [0.61, 0.23, 0.27, 0.19],
        "clip {}: x/y/w/h",
        c.id
    );
    assert_eq!(f(&c.opacity), 0.45);
    assert_eq!(c.fit, Some(Fit::Cover));
    assert_eq!(c.frame_shape, Some(FrameShape::Rounded));
    assert_eq!(c.rotation, Some(Rotation::Deg90));
    assert_eq!((c.mirror, c.flip_y), (Some(true), Some(true)));
    assert_eq!(
        [
            c.crop_zoom.as_ref().map(f),
            c.crop_x.as_ref().map(f),
            c.crop_y.as_ref().map(f)
        ],
        [Some(1.7), Some(0.35), Some(0.8)]
    );
}

fn refusal(result: Result<(Project, String), EditorError>) -> EditorError {
    let err = result.expect_err("the command must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
    err
}

// ---- setSpeed ---------------------------------------------------------------

// Named test 1 (brief).
#[test]
fn speed_two_halves_output_duration_and_keeps_cue_source_times() {
    let p = project();
    let (candidate, label) = speed(&p, "c1", 2.0).unwrap();
    validate_project(&candidate).expect("a speed change is a valid project");
    let c1 = by_id(&candidate, "c1");

    assert_eq!(clip_end(c1) - c1.start_ms, 2_000, "4000 ms of source at 2x");
    assert_eq!(
        (c1.start_ms, c1.in_ms, c1.out_ms),
        (700, 1_000, 5_000),
        "speed never moves the clip or its source range"
    );
    assert_eq!(c1.preserve_pitch, Some(true));
    assert_eq!(label, "Change speed");

    // Cues keep their SOURCE times; their output span follows the mapping.
    assert_eq!(candidate.effects, p.effects);
    assert_eq!(candidate.markers, p.markers);
    assert_eq!(
        cue_output_span(&span(by_id(&p, "c1")), 1_500, 2_500),
        Some((1_200, 2_200))
    );
    assert_eq!(
        cue_output_span(&span(c1), 1_500, 2_500),
        Some((950, 1_450)),
        "at 2x the cue's output span halves and moves toward the clip start"
    );
    // The next clip on the track stays exactly where it was.
    assert_eq!(by_id(&candidate, "c2").start_ms, 5_200);
}

// Named test 2 (brief).
#[test]
fn speed_change_that_overlaps_the_next_clip_is_refused() {
    let p = project();
    // 4000 ms of source at 0.5x lasts 8000 ms: c1 would end at 8700, past
    // c2's start at 5200. Nothing is rippled to make room.
    let err = refusal(speed(&p, "c1", 0.5));
    assert!(
        err.message.contains("c2") && err.message.contains("make room"),
        "the refusal names the clip in the way and what to do: {}",
        err.message
    );
    // A slowdown that still ends before c2 is fine: 4000/0.9 = 4444 ms,
    // ending at 5144 < 5200.
    let (ok, _) = speed(&p, "c1", 0.9).unwrap();
    assert_eq!(clip_end(by_id(&ok, "c1")), 5_144);
    validate_project(&ok).unwrap();
}

#[test]
fn speed_outside_the_quarter_to_four_range_is_refused() {
    let p = project();
    for bad in [0.2, 4.5] {
        let err = refusal(speed(&p, "c1", bad));
        assert!(err.message.contains("0.25"), "{}", err.message);
    }
    // 0.25x is in range: its refusal here is the overlap with c2, not the
    // range; 4x is in range and fits.
    let err = refusal(speed(&p, "c1", 0.25));
    assert!(err.message.contains("c2"), "{}", err.message);
    speed(&p, "c2", 4.0).expect("4x is in range");
}

#[test]
fn speed_that_would_leave_less_than_the_minimum_is_refused() {
    let mut p = project();
    p.clips.push(clip("short", "v1", "av", 10_000, 0, 300));
    // 300 ms of source at 4x = 75 ms, below the 100 ms minimum.
    let err = refusal(speed(&p, "short", 4.0));
    assert!(err.message.contains("100 ms"), "{}", err.message);
}

#[test]
fn speed_on_a_locked_track_is_refused() {
    let err = refusal(speed(&project(), "l1", 2.0));
    assert!(err.message.contains("locked"), "{}", err.message);
}

#[test]
fn speed_that_changes_nothing_is_refused_but_a_pitch_change_is_not() {
    let mut p = project();
    p.clips[0].speed = Some(n(1.5));
    p.clips[0].preserve_pitch = Some(true);
    refusal(speed(&p, "c1", 1.5));
    let (pitch, label) = set_speed(
        &p,
        &SetSpeedPayload {
            clip_id: "c1".into(),
            speed: n(1.5),
            preserve_pitch: false,
        },
    )
    .unwrap();
    assert_eq!(by_id(&pitch, "c1").preserve_pitch, Some(false));
    assert_eq!(label, "Preserve pitch off");
}

// Carried from Task 29: setSpeed clamps fades exactly like trimClip does.
#[test]
fn speed_that_shortens_a_clip_clamps_its_fades_and_labels_the_step() {
    let mut p = project();
    p.clips[0].fade_in_ms = 1_500;
    p.clips[0].fade_out_ms = 900;
    // 4000 ms at 2x -> 2000 ms output, so each fade may be at most 1000.
    let (candidate, label) = speed(&p, "c1", 2.0).unwrap();
    let c1 = by_id(&candidate, "c1");
    assert_eq!((c1.fade_in_ms, c1.fade_out_ms), (1_000, 900));
    assert_eq!(label, "Change speed (fades adjusted)");
    validate_project(&candidate).expect("clamped fades are valid");

    // Slowing down never needs a clamp: the plain label.
    let (_, label) = speed(&p, "c1", 0.9).unwrap();
    assert_eq!(label, "Change speed");
}

// Carried from Task 30: setSpeed respects an existing transition.
#[test]
fn speed_on_a_transitioned_clip_is_refused_up_front_when_it_breaks_the_geometry() {
    let mut p = project();
    // c1 [700,4700) -> c3 at 4700 on the same track, dissolved over 900ms.
    p.clips.retain(|c| c.id != "c2");
    p.clips.push(clip("c3", "v1", "av", 4_700, 0, 2_000));
    let (p, _) = apply(
        &p,
        &EditorCommand::AddTransition(AddTransitionPayload {
            from_clip_id: "c1".into(),
            to_clip_id: "c3".into(),
            duration_ms: 900,
            kind: TransitionKind::Dissolve,
        }),
        &no_context(),
    )
    .unwrap();
    validate_project(&p).unwrap();

    // The FROM clip changing speed moves its end: the overlap is no longer
    // the transition's window.
    let err = refusal(speed(&p, "c1", 1.25));
    assert!(err.message.contains("transition"), "{}", err.message);

    // The TO clip at 2x lasts 1000 ms, so a 900 ms dissolve exceeds half of
    // it: refused as invalidRequest, never left for validate_project's
    // invalidProject.
    let err = refusal(speed(&p, "c3", 2.0));
    assert!(err.message.contains("transition"), "{}", err.message);

    // A speed that keeps the geometry is accepted and valid.
    let (ok, _) = speed(&p, "c3", 1.1).unwrap();
    validate_project(&ok).expect("a geometry-preserving speed change stays valid");
}

// ---- setLayout --------------------------------------------------------------

// Named test 3 (brief, A10): set -> save -> load -> equal values.
#[test]
fn layout_round_trips_through_serialization() {
    let mut session = EditorSession::new("s1", project());
    session
        .execute(
            &ExecuteRequest {
                session_id: "s1".into(),
                expected_revision: 1,
                command_id: "cmd-layout".into(),
                command: EditorCommand::SetLayout(full_layout(&["c1"])),
            },
            &no_context(),
        )
        .expect("the layout command is accepted and validates");

    let saved = serde_json::to_string(session.project()).unwrap();
    let loaded: Project = serde_json::from_str(&saved).unwrap();
    validate_project(&loaded).expect("the reloaded project is valid");
    assert_full_layout(by_id(&loaded, "c1"));
    assert_eq!(&loaded, session.project(), "nothing else drifts on reload");
    // The untargeted clip keeps its defaults.
    assert_eq!(f(&by_id(&loaded, "c2").x), 0.0);
}

// Named test 4 (brief).
#[test]
fn multi_clip_layout_is_atomic() {
    let p = project();
    let (candidate, label) = set_layout(&p, &full_layout(&["c1", "c2"])).unwrap();
    assert_full_layout(by_id(&candidate, "c1"));
    assert_full_layout(by_id(&candidate, "c2"));
    assert_eq!(label, "Change layout (2 clips)");

    // One unusable target refuses the WHOLE command: through a session,
    // nothing is installed and the revision does not move.
    for (bad, why) in [
        ("l1", "locked"),
        ("s1", "audio"),
        ("nope", "does not resolve"),
    ] {
        let mut session = EditorSession::new("s1", p.clone());
        let err = session
            .execute(
                &ExecuteRequest {
                    session_id: "s1".into(),
                    expected_revision: 1,
                    command_id: format!("cmd-{bad}"),
                    command: EditorCommand::SetLayout(full_layout(&["c1", bad])),
                },
                &no_context(),
            )
            .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(err.message.contains(why), "{bad}: {}", err.message);
        assert_eq!(session.project(), &p, "{bad}: c1 must not change either");
        assert_eq!(session.snapshot().revision, 1);
    }
}

#[test]
fn layout_values_outside_the_schema_ranges_are_refused() {
    let p = project();
    let cases: Vec<(&str, SetLayoutPayload)> = vec![
        (
            "x",
            SetLayoutPayload {
                x: Some(n(1.2)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "y",
            SetLayoutPayload {
                y: Some(n(-0.1)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "w",
            SetLayoutPayload {
                w: Some(n(0.05)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "h",
            SetLayoutPayload {
                h: Some(n(1.5)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "opacity",
            SetLayoutPayload {
                opacity: Some(n(1.01)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "cropZoom",
            SetLayoutPayload {
                crop_zoom: Some(n(3.5)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "cropX",
            SetLayoutPayload {
                crop_x: Some(n(-0.2)),
                ..empty_layout(&["c1"])
            },
        ),
        (
            "cropY",
            SetLayoutPayload {
                crop_y: Some(n(1.3)),
                ..empty_layout(&["c1"])
            },
        ),
    ];
    for (field, payload) in cases {
        let err = refusal(set_layout(&p, &payload));
        assert!(err.message.contains(field), "{field}: {}", err.message);
    }
    // The inclusive bounds themselves are accepted.
    let edges = SetLayoutPayload {
        x: Some(n(0.0)),
        y: Some(n(1.0)),
        w: Some(n(0.1)),
        h: Some(n(1.0)),
        opacity: Some(n(0.0)),
        crop_zoom: Some(n(3.0)),
        crop_x: Some(n(1.0)),
        crop_y: Some(n(0.0)),
        ..empty_layout(&["c1"])
    };
    let (candidate, _) = set_layout(&p, &edges).unwrap();
    validate_project(&candidate).unwrap();
}

#[test]
fn layout_with_no_field_or_no_clip_is_refused_and_a_partial_one_keeps_the_rest() {
    let p = project();
    refusal(set_layout(&p, &empty_layout(&["c1"])));
    refusal(set_layout(
        &p,
        &SetLayoutPayload {
            x: Some(n(0.5)),
            ..empty_layout(&[])
        },
    ));

    let mut before = p.clone();
    before.clips[0].rotation = Some(Rotation::Deg270);
    let (candidate, label) = set_layout(
        &before,
        &SetLayoutPayload {
            y: Some(n(0.23)),
            ..empty_layout(&["c1"])
        },
    )
    .unwrap();
    let c1 = by_id(&candidate, "c1");
    assert_eq!(f(&c1.y), 0.23);
    assert_eq!(f(&c1.x), 0.0, "an absent field is left alone");
    assert_eq!(c1.rotation, Some(Rotation::Deg270));
    assert_eq!(label, "Change layout");
}

#[test]
fn rotation_accepts_only_quarter_turns_on_the_wire() {
    let ok: EditorCommand = serde_json::from_value(serde_json::json!({
        "kind": "setLayout", "clipIds": ["c1"], "rotation": 270, "frameShape": "circle",
        "fit": "cover", "flipY": true, "cropZoom": 2, "cropX": 0.25, "cropY": 0.75
    }))
    .unwrap();
    let EditorCommand::SetLayout(payload) = ok else {
        panic!("setLayout decodes as setLayout");
    };
    assert_eq!(payload.rotation, Some(Rotation::Deg270));
    assert_eq!(payload.frame_shape, Some(FrameShape::Circle));
    assert_eq!(payload.flip_y, Some(true));
    assert!(serde_json::from_value::<EditorCommand>(serde_json::json!({
        "kind": "setLayout", "clipIds": ["c1"], "rotation": 45
    }))
    .is_err());
}

#[test]
fn validation_backstops_a_crop_anchor_outside_the_frame() {
    // A document from elsewhere (never through setLayout) carrying an
    // out-of-range anchor is invalidProject, like crop_zoom already was.
    for (x, y) in [(Some(1.2), None), (None, Some(-0.4))] {
        let mut p = project();
        p.clips[0].crop_x = x.map(n);
        p.clips[0].crop_y = y.map(n);
        let err = validate_project(&p).unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidProject);
        assert!(err.message.contains("crop_"), "{}", err.message);
    }
}

// ---- setCanvas (Task 32; F-38) ---------------------------------------------

// Named test 5 (brief).
#[test]
fn only_the_four_canvases_are_accepted() {
    let p = project(); // starts at 1280x720 (minimal_project's default)
    for (w, h) in [(1920, 1080), (100, 100), (720, 721), (0, 0)] {
        let err = refusal(set_canvas(
            &p,
            &SetCanvasPayload {
                width: w,
                height: h,
            },
        ));
        assert!(
            err.message.contains(&format!("{w}x{h}")),
            "{w}x{h}: {}",
            err.message
        );
    }
    // Every one of the four presets OTHER than the project's own starting
    // canvas is accepted, keeps fps untouched, and is itself schema-valid.
    for &(w, h) in limits::CANVASES
        .iter()
        .filter(|&&(w, h)| (w, h) != (p.canvas.width, p.canvas.height))
    {
        let (candidate, label) = set_canvas(
            &p,
            &SetCanvasPayload {
                width: w,
                height: h,
            },
        )
        .unwrap();
        assert_eq!((candidate.canvas.width, candidate.canvas.height), (w, h));
        assert_eq!(
            candidate.canvas.fps, p.canvas.fps,
            "fps is not a setCanvas field"
        );
        assert_eq!(label, "Change canvas");
        validate_project(&candidate).expect("every preset is a valid canvas");
    }
}

#[test]
fn set_canvas_to_the_current_pair_is_refused_as_a_no_op() {
    let p = project(); // 1280x720
    let err = refusal(set_canvas(
        &p,
        &SetCanvasPayload {
            width: 1280,
            height: 720,
        },
    ));
    assert!(err.message.contains("already"), "{}", err.message);
}

#[test]
fn set_canvas_preserves_forward_compat_extra() {
    let mut p = project();
    p.canvas
        .extra
        .insert("future".to_string(), serde_json::json!(true));
    let (candidate, _) = set_canvas(
        &p,
        &SetCanvasPayload {
            width: 720,
            height: 1280,
        },
    )
    .unwrap();
    assert_eq!(
        candidate.canvas.extra.get("future"),
        Some(&serde_json::json!(true)),
        "an edit here must not drop an unrelated forward-compat field (R3)"
    );
}

// ---- setAdjustments (Task 32; F-39) ----------------------------------------

fn adjustments(
    brightness: f64,
    contrast: f64,
    saturation: f64,
    sepia: f64,
    grayscale: f64,
) -> Adjustments {
    Adjustments {
        brightness: n(brightness),
        contrast: n(contrast),
        saturation: n(saturation),
        sepia: n(sepia),
        grayscale: n(grayscale),
        extra: Map::new(),
    }
}

/// A fixture dedicated to `setAdjustments`, independent of the shared
/// `project()` above (the fixture-flaw rule: piling a card asset and a
/// second locked track onto the speed/layout fixture risks tripping one of
/// ITS guards instead of this one's). `cam`/`v1`: ordinary footage. `mic`/
/// `a1`: audio, no colour. `v2` (locked): `locked1`. `cardAsset`/`card1`: a
/// title card -- `builtin: Card` is what makes it one, never `Clip.card`
/// (unset here on purpose, so the test cannot pass by accident from the
/// wrong field).
fn color_project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("cam", AssetKind::Video, 10_000));
    p.assets.push(asset("mic", AssetKind::Audio, 10_000));
    let mut card_asset = asset("cardAsset", AssetKind::Video, 1_000);
    card_asset.builtin = Some(Builtin::Card);
    p.assets.push(card_asset);
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("v2", TrackKind::Video, true));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.clips.push(clip("c1", "v1", "cam", 0, 0, 2_000));
    p.clips.push(clip("c2", "v1", "cam", 3_000, 0, 1_000));
    p.clips.push(clip("locked1", "v2", "cam", 0, 0, 1_000));
    p.clips.push(clip("aud1", "a1", "mic", 0, 0, 1_000));
    p.clips
        .push(clip("card1", "v1", "cardAsset", 5_000, 0, 500));
    validate_project(&p).expect("the color fixture itself is valid");
    p
}

// Named test 6 (brief).
#[test]
fn adjustments_ranges_are_enforced() {
    let p = color_project();
    let cases: Vec<(&str, Adjustments)> = vec![
        ("brightness", adjustments(0.2, 1.0, 1.0, 0.0, 0.0)),
        ("brightness", adjustments(2.1, 1.0, 1.0, 0.0, 0.0)),
        ("contrast", adjustments(1.0, 0.24, 1.0, 0.0, 0.0)),
        ("contrast", adjustments(1.0, 2.01, 1.0, 0.0, 0.0)),
        ("saturation", adjustments(1.0, 1.0, -0.1, 0.0, 0.0)),
        ("saturation", adjustments(1.0, 1.0, 2.01, 0.0, 0.0)),
        ("sepia", adjustments(1.0, 1.0, 1.0, -0.1, 0.0)),
        ("sepia", adjustments(1.0, 1.0, 1.0, 1.01, 0.0)),
        ("grayscale", adjustments(1.0, 1.0, 1.0, 0.0, -0.1)),
        ("grayscale", adjustments(1.0, 1.0, 1.0, 0.0, 1.01)),
    ];
    for (field, adj) in cases {
        let err = refusal(set_adjustments(
            &p,
            &SetAdjustmentsPayload {
                clip_ids: vec!["c1".into()],
                adjustments: Some(adj),
            },
        ));
        assert!(err.message.contains(field), "{field}: {}", err.message);
    }
    // The inclusive bounds are accepted, and land on the clip untouched.
    let edges = adjustments(0.25, 2.0, 0.0, 1.0, 1.0);
    let (candidate, label) = set_adjustments(
        &p,
        &SetAdjustmentsPayload {
            clip_ids: vec!["c1".into()],
            adjustments: Some(edges.clone()),
        },
    )
    .unwrap();
    assert_eq!(by_id(&candidate, "c1").adjustments.as_ref(), Some(&edges));
    assert_eq!(label, "Change colour");
    validate_project(&candidate).unwrap();
}

// Named test 7 (brief).
#[test]
fn cards_refuse_adjustments() {
    let p = color_project();
    let attempt = adjustments(1.2, 1.0, 1.0, 0.0, 0.0);

    for payload in [
        SetAdjustmentsPayload {
            clip_ids: vec!["card1".into()],
            adjustments: Some(attempt.clone()),
        },
        // Clearing is refused the same way -- a card can never carry
        // adjustments, so there is nothing to clear either.
        SetAdjustmentsPayload {
            clip_ids: vec!["card1".into()],
            adjustments: None,
        },
        // One card among several targets refuses the WHOLE command --
        // `check_color_targets`' own atomic-multi-target discipline.
        SetAdjustmentsPayload {
            clip_ids: vec!["c1".into(), "card1".into()],
            adjustments: Some(attempt.clone()),
        },
    ] {
        let err = refusal(set_adjustments(&p, &payload));
        assert_eq!(err.message, "Colour applies to footage, not title cards");
    }
}

#[test]
fn adjustments_refuse_an_audio_clip_and_a_locked_track() {
    let p = color_project();
    let attempt = Some(adjustments(1.2, 1.0, 1.0, 0.0, 0.0));

    let err = refusal(set_adjustments(
        &p,
        &SetAdjustmentsPayload {
            clip_ids: vec!["aud1".into()],
            adjustments: attempt.clone(),
        },
    ));
    assert!(err.message.contains("audio"), "{}", err.message);

    let err = refusal(set_adjustments(
        &p,
        &SetAdjustmentsPayload {
            clip_ids: vec!["locked1".into()],
            adjustments: attempt,
        },
    ));
    assert!(err.message.contains("locked"), "{}", err.message);
}

#[test]
fn adjustments_apply_to_every_target_and_none_clears_only_the_targeted_clip() {
    let p = color_project();
    let (candidate, label) = set_adjustments(
        &p,
        &SetAdjustmentsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            adjustments: Some(adjustments(1.3, 1.1, 1.3, 0.0, 0.0)),
        },
    )
    .unwrap();
    assert!(by_id(&candidate, "c1").adjustments.is_some());
    assert!(by_id(&candidate, "c2").adjustments.is_some());
    assert_eq!(label, "Change colour (2 clips)");

    let (cleared, label) = set_adjustments(
        &candidate,
        &SetAdjustmentsPayload {
            clip_ids: vec!["c1".into()],
            adjustments: None,
        },
    )
    .unwrap();
    assert!(by_id(&cleared, "c1").adjustments.is_none());
    assert!(
        by_id(&cleared, "c2").adjustments.is_some(),
        "an untargeted clip is left alone"
    );
    assert_eq!(label, "Reset colour");
}

#[test]
fn adjustments_with_no_clips_or_an_unresolved_clip_is_refused() {
    let p = color_project();
    let err = refusal(set_adjustments(
        &p,
        &SetAdjustmentsPayload {
            clip_ids: vec![],
            adjustments: None,
        },
    ));
    assert!(err.message.contains("at least one clip"), "{}", err.message);

    let err = refusal(set_adjustments(
        &p,
        &SetAdjustmentsPayload {
            clip_ids: vec!["nope".into()],
            adjustments: None,
        },
    ));
    assert!(err.message.contains("does not resolve"), "{}", err.message);
}
