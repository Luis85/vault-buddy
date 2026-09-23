//! Tests for `clips.rs`'s seven clip commands (not inline, the
//! `validate.rs`/`validate_tests.rs` precedent). At least one fixture
//! carries a non-unit speed and cues are deliberately asymmetric
//! (straddling/boundary cases, never a centered cue that could pass by
//! accident).

use super::*;
use crate::editor::model::{AssetKind, MediaType};
use crate::editor::model_cues::{
    CaptionCue, CaptionPosition, CaptionSettings, Effect, EffectKind, Marker, Transition,
    TransitionKind,
};
use crate::editor::test_support::{asset, clip, minimal_project, track};

fn num_f(v: f64) -> Num {
    Num::from_f64(v).expect("finite test fixture value")
}

fn base_project() -> Project {
    minimal_project()
}

fn effect(id: &str, clip_id: &str, start_ms: u64, end_ms: u64) -> Effect {
    Effect {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        kind: EffectKind::Highlight,
        start_ms,
        end_ms,
        x: num(0),
        y: num(0),
        color: "#ffffff".to_string(),
        text: None,
        w: None,
        h: None,
        x2: None,
        y2: None,
        factor: None,
        font_size: None,
        stroke: None,
        dim: None,
        easing: None,
        number: None,
        background: None,
        extra: Map::new(),
    }
}

fn caption(id: &str, clip_id: &str, start_ms: u64, end_ms: u64) -> CaptionCue {
    CaptionCue {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        start_ms,
        end_ms,
        text: "Hi".to_string(),
        extra: Map::new(),
    }
}

fn caption_settings(cues: Vec<CaptionCue>) -> CaptionSettings {
    CaptionSettings {
        enabled: true,
        burn_in: false,
        font_size: num(16),
        position: CaptionPosition::Bottom,
        background: false,
        cues,
        extra: Map::new(),
    }
}

fn marker(id: &str, clip_id: &str, source_ms: u64) -> Marker {
    Marker {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        source_ms,
        title: "M".to_string(),
        extra: Map::new(),
    }
}

// ---- insertClip / updateClip (Behavior bullets, not individually named
// in the brief's own Tests-first list) -------------------------------------

#[test]
fn insert_clip_mints_an_id_and_applies_defaults() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));

    let (candidate, label) = insert_clip(
        &project,
        &InsertClipPayload {
            asset_id: "a1".into(),
            track_id: "v1".into(),
            start_ms: 0,
            in_ms: 0,
            out_ms: 1_000,
        },
    )
    .unwrap();

    assert_eq!(label, "Insert clip");
    assert_eq!(candidate.clips.len(), 1);
    let inserted = &candidate.clips[0];
    assert!(inserted.id.starts_with("clip-"), "id: {}", inserted.id);
    assert_eq!(
        (inserted.fade_in_ms, inserted.fade_out_ms, inserted.muted),
        (0, 0, false)
    );
    assert_eq!(inserted.fade_curve, FadeCurve::Linear);
    assert_eq!((inserted.x.clone(), inserted.y.clone()), (num(0), num(0)));
    assert_eq!((inserted.w.clone(), inserted.h.clone()), (num(1), num(1)));
    assert_eq!(
        (inserted.opacity.clone(), inserted.volume.clone()),
        (num(1), num(1))
    );
}

#[test]
fn insert_clip_refuses_an_overlap_on_the_same_track() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let err = insert_clip(
        &project,
        &InsertClipPayload {
            asset_id: "a1".into(),
            track_id: "v1".into(),
            start_ms: 500,
            in_ms: 0,
            out_ms: 1_000,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn insert_clip_refuses_a_start_ms_near_u64_max_instead_of_overflowing() {
    // Regression: `startMs` here is unvalidated IPC input -- before the
    // fix, `time::clip_output_end`'s unchecked `start_ms + duration`
    // overflowed u64 (panics in debug, wraps in release) inside the
    // overlap check, before `validate_project` ever got a chance to
    // reject it.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));

    let err = insert_clip(
        &project,
        &InsertClipPayload {
            asset_id: "a1".into(),
            track_id: "v1".into(),
            start_ms: u64::MAX - 10,
            in_ms: 0,
            out_ms: 1_000,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- insertClip -- image out_ms exemption (Task 26, F-10) -----------------
// Multi-track placement and stills: an image asset's `duration_ms` is only
// ever the import-time default (`probe::IMAGE_DEFAULT_DURATION_MS`,
// 5000 ms), never a real recorded length, so `insertClip` (like `trimClip`
// already did before this task) must let an image clip's `outMs` extend
// past it -- up to `limits::MAX_DURATION_MS` -- while a real video/audio
// asset's own `duration_ms` stays a hard ceiling. Before this task
// `insert_clip` had NO `outMs` bound check of its own at all (only
// `trim_clip` and `validate_project`'s backstop did), so a video asset's
// nominal duration was silently unenforced by this function in isolation.

#[test]
fn image_clips_may_extend_past_their_nominal_duration() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    let mut still = asset("a1", AssetKind::Video, 5_000);
    still.media_type = Some(MediaType::Image);
    project.assets.push(still);

    let (candidate, _) = insert_clip(
        &project,
        &InsertClipPayload {
            asset_id: "a1".into(),
            track_id: "v1".into(),
            start_ms: 0,
            in_ms: 0,
            out_ms: 6_000, // past the still's own 5_000 ms nominal duration
        },
    )
    .unwrap();
    assert_eq!(candidate.clips[0].out_ms, 6_000);
}

#[test]
fn video_clips_may_not() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));

    let err = insert_clip(
        &project,
        &InsertClipPayload {
            asset_id: "a1".into(),
            track_id: "v1".into(),
            start_ms: 0,
            in_ms: 0,
            out_ms: 6_000, // past the asset's real 5_000 ms duration
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn update_clip_trims_and_renames() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let (candidate, label) = update_clip(
        &project,
        &UpdateClipPayload {
            clip_id: "c1".into(),
            name: "  New Name  ".into(),
        },
    )
    .unwrap();
    assert_eq!(label, "Rename clip");
    assert_eq!(candidate.clips[0].name, "New Name");
}

#[test]
fn update_clip_rejects_an_empty_name() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let err = update_clip(
        &project,
        &UpdateClipPayload {
            clip_id: "c1".into(),
            name: "   ".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- splitClip --------------------------------------------------------------

#[test]
fn split_conserves_total_duration() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 1_200));
    // Task 21 (F14): the source span/split point are widened from the
    // original 0..100 @2x / at_ms 13 (50ms total, 13ms/37ms halves) so
    // BOTH halves clear the new MIN_CLIP_MS (100ms) minimum -- this test is
    // about duration conservation, not the minimum, and a fixture that
    // trips a DIFFERENT guard than the one under test proves nothing (the
    // global constraints' own "fixture flaw" rule).
    let mut c1 = clip("c1", "v1", "a1", 0, 0, 600);
    c1.speed = Some(num_f(2.0));
    project.clips.push(c1);

    let before = time::project_duration(&project);
    assert_eq!(before, 300);

    let (candidate, _) = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 130,
        },
    )
    .unwrap();

    assert_eq!(candidate.clips.len(), 2);
    assert_eq!(
        time::project_duration(&candidate),
        before,
        "splitting must not change the project's total output duration"
    );
}

#[test]
fn split_that_would_leave_a_sub_minimum_half_is_refused() {
    // Task 21 (F14): a clip 0..1_000 split at 950 would leave a 50ms right
    // half, and at 50 a 50ms left half -- either falls below
    // limits::MIN_CLIP_MS (100). Refused before anything is installed.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    // Both halves are checked: 950 leaves a 50ms RIGHT half, 50 a 50ms LEFT
    // half -- a check of only one side would pass one of these.
    for at_ms in [950, 50] {
        let err = split_clip(
            &project,
            &SplitClipPayload {
                clip_id: "c1".into(),
                at_ms,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(
            err.message.contains("100 ms minimum"),
            "the refusal must name the 100ms minimum: {}",
            err.message
        );
    }
}

#[test]
fn split_at_boundary_is_refused() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    for at_ms in [0, 1_000] {
        let err = split_clip(
            &project,
            &SplitClipPayload {
                clip_id: "c1".into(),
                at_ms,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(
            err.message.contains("boundary"),
            "at_ms {at_ms}: {}",
            err.message
        );
    }
}

#[test]
fn split_moves_whole_cues_and_splits_straddling_cues() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));
    project.effects.push(effect("e-left", "c1", 0, 300));
    project.effects.push(effect("e-right", "c1", 600, 900));
    project.effects.push(effect("e-straddle", "c1", 400, 700));
    project.captions = Some(caption_settings(vec![
        caption("cap-left", "c1", 0, 300),
        caption("cap-straddle", "c1", 400, 700),
    ]));

    let (candidate, _) = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 500,
        },
    )
    .unwrap();
    let right_id = candidate
        .clips
        .iter()
        .find(|c| c.id != "c1")
        .expect("split must produce a right half")
        .id
        .clone();

    let e_left = candidate.effects.iter().find(|e| e.id == "e-left").unwrap();
    assert_eq!(e_left.clip_id, "c1");
    assert_eq!((e_left.start_ms, e_left.end_ms), (0, 300));

    let e_right = candidate
        .effects
        .iter()
        .find(|e| e.id == "e-right")
        .unwrap();
    assert_eq!(e_right.clip_id, right_id);
    assert_eq!((e_right.start_ms, e_right.end_ms), (600, 900));

    let straddle_left = candidate
        .effects
        .iter()
        .find(|e| e.id == "e-straddle")
        .unwrap();
    assert_eq!(straddle_left.clip_id, "c1");
    assert_eq!((straddle_left.start_ms, straddle_left.end_ms), (400, 500));

    let straddle_right = candidate
        .effects
        .iter()
        .filter(|e| e.clip_id == right_id && e.start_ms == 500 && e.end_ms == 700)
        .count();
    assert_eq!(
        straddle_right, 1,
        "the straddling effect's right half must get a fresh id on the right half"
    );
    assert_eq!(
        candidate.effects.len(),
        4,
        "one straddling effect must split into two, the other two stay whole"
    );

    let cues = &candidate.captions.as_ref().unwrap().cues;
    let cap_left = cues.iter().find(|c| c.id == "cap-left").unwrap();
    assert_eq!(cap_left.clip_id, "c1");
    let cap_straddle_left = cues.iter().find(|c| c.id == "cap-straddle").unwrap();
    assert_eq!(cap_straddle_left.clip_id, "c1");
    assert_eq!(
        (cap_straddle_left.start_ms, cap_straddle_left.end_ms),
        (400, 500)
    );
    let cap_straddle_right = cues
        .iter()
        .filter(|c| c.clip_id == right_id && c.start_ms == 500 && c.end_ms == 700)
        .count();
    assert_eq!(cap_straddle_right, 1);
    assert_eq!(cues.len(), 3);
}

#[test]
fn split_assigns_a_marker_on_the_cut_to_exactly_one_half() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));
    project.markers.push(marker("m-before", "c1", 300));
    project.markers.push(marker("m-at-cut", "c1", 500));
    project.markers.push(marker("m-after", "c1", 700));

    let (candidate, _) = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 500,
        },
    )
    .unwrap();
    let right_id = candidate
        .clips
        .iter()
        .find(|c| c.id != "c1")
        .unwrap()
        .id
        .clone();

    let m_before = candidate
        .markers
        .iter()
        .find(|m| m.id == "m-before")
        .unwrap();
    assert_eq!(m_before.clip_id, "c1");

    let m_at_cut = candidate
        .markers
        .iter()
        .find(|m| m.id == "m-at-cut")
        .unwrap();
    assert_eq!(
        m_at_cut.clip_id, right_id,
        "a marker exactly at the cut belongs to the right half only"
    );

    let m_after = candidate
        .markers
        .iter()
        .find(|m| m.id == "m-after")
        .unwrap();
    assert_eq!(m_after.clip_id, right_id);
}

#[test]
fn split_at_non_unit_speed_maps_the_source_point() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c1 = clip("c1", "v1", "a1", 0, 0, 4_000);
    c1.speed = Some(num_f(2.0));
    project.clips.push(c1);

    let (candidate, _) = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 500,
        },
    )
    .unwrap();

    let left = candidate.clips.iter().find(|c| c.id == "c1").unwrap();
    assert_eq!(
        left.out_ms, 1_000,
        "source split point = in_ms + (atMs-start_ms)*speed = 0 + 500*2"
    );
    let right = candidate.clips.iter().find(|c| c.id != "c1").unwrap();
    assert_eq!(right.in_ms, 1_000);
    assert_eq!(right.start_ms, 500);
}

#[test]
fn split_save_reopen_round_trip_keeps_cue_intervals() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));
    project.effects.push(effect("e-straddle", "c1", 400, 700));
    project.markers.push(marker("m1", "c1", 500));

    let (candidate, _) = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 500,
        },
    )
    .unwrap();

    let value = serde_json::to_value(&candidate).unwrap();
    let round_tripped: Project = serde_json::from_value(value).unwrap();
    assert_eq!(
        round_tripped, candidate,
        "a split project's clips and cue intervals must round-trip through JSON byte-for-byte"
    );
}

// ---- trimClip -----------------------------------------------------------

#[test]
fn trim_keeps_source_cue_times() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));
    project.effects.push(effect("e1", "c1", 100, 200));
    project.markers.push(marker("m1", "c1", 150));
    project.captions = Some(caption_settings(vec![caption("cap1", "c1", 100, 200)]));

    let (candidate, _) = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 50,
            in_ms: 300,
            out_ms: 900,
        },
    )
    .unwrap();

    let clip = candidate.clips.iter().find(|c| c.id == "c1").unwrap();
    assert_eq!((clip.start_ms, clip.in_ms, clip.out_ms), (50, 300, 900));

    let e1 = candidate.effects.iter().find(|e| e.id == "e1").unwrap();
    assert_eq!(
        (e1.start_ms, e1.end_ms),
        (100, 200),
        "trim must not touch a source-linked cue's own timestamps"
    );
    let m1 = candidate.markers.iter().find(|m| m.id == "m1").unwrap();
    assert_eq!(m1.source_ms, 150);
    let cap1 = &candidate.captions.as_ref().unwrap().cues[0];
    assert_eq!((cap1.start_ms, cap1.end_ms), (100, 200));
}

#[test]
fn trim_to_empty_is_refused() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 2_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let err = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 0,
            in_ms: 500,
            out_ms: 500,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn trim_below_the_minimum_is_refused_naming_100ms() {
    // Task 21 (F14): 500..599 is a 99ms output duration -- one below
    // limits::MIN_CLIP_MS.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let err = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 0,
            in_ms: 500,
            out_ms: 599,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message.contains("100 ms minimum"),
        "the refusal must name the 100ms minimum: {}",
        err.message
    );
}

#[test]
fn trim_to_exactly_the_minimum_is_accepted() {
    // The boundary: 500..600 is exactly limits::MIN_CLIP_MS -- the minimum
    // is a floor the clip may sit ON, not one it must clear.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let (candidate, _) = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 0,
            in_ms: 500,
            out_ms: 600,
        },
    )
    .unwrap();
    assert_eq!(candidate.clips[0].out_ms - candidate.clips[0].in_ms, 100);
}

// F11 (Task 29): trimClip clamps a clip's own fades when they no longer fit
// half the SHRUNK output duration, rather than refusing a trim the user
// never asked to touch a fade to perform.
#[test]
fn trim_that_shrinks_a_clip_clamps_its_fades_and_labels_the_step() {
    // 1000ms clip, 400/400ms fades -- both already within the ORIGINAL
    // 500ms half-duration limit, so nothing here is invalid before the trim.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c = clip("c1", "v1", "a1", 0, 0, 1_000);
    c.fade_in_ms = 400;
    c.fade_out_ms = 400;
    project.clips.push(c);

    // Trimmed to a 600ms output duration -- half is now 300ms, below both
    // fades, which must clamp rather than make the trim itself fail.
    let (candidate, label) = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 0,
            in_ms: 0,
            out_ms: 600,
        },
    )
    .unwrap();
    let trimmed = &candidate.clips[0];
    assert_eq!((trimmed.fade_in_ms, trimmed.fade_out_ms), (300, 300));
    assert_eq!(label, "Trim (fades adjusted)");
}

#[test]
fn trim_that_does_not_shrink_fades_below_their_limit_keeps_the_plain_label() {
    // The other arm of the same branch: a fade that ALREADY fits the new
    // half-duration is left untouched and the label stays the plain one --
    // "adjusted" must never be claimed when nothing was.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c = clip("c1", "v1", "a1", 0, 0, 1_000);
    c.fade_in_ms = 100;
    c.fade_out_ms = 100;
    project.clips.push(c);

    let (candidate, label) = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 0,
            in_ms: 0,
            out_ms: 600,
        },
    )
    .unwrap();
    let trimmed = &candidate.clips[0];
    assert_eq!((trimmed.fade_in_ms, trimmed.fade_out_ms), (100, 100));
    assert_eq!(label, "Trim clip");
}

#[test]
fn trim_clip_refuses_a_start_ms_near_u64_max_instead_of_overflowing() {
    // Regression: same overflow class as insertClip's -- trimClip's own
    // `startMs` builds a `ClipSpan` straight from unvalidated IPC input
    // for the overlap check.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let err = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: u64::MAX - 10,
            in_ms: 0,
            out_ms: 1_000,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- deleteClips ------------------------------------------------------------

#[test]
fn delete_leave_gap_moves_nothing() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 500));
    project.clips.push(clip("c2", "v1", "a1", 1_000, 0, 500));

    let (candidate, _) = delete_clips(
        &project,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: false,
        },
    )
    .unwrap();

    assert_eq!(candidate.clips.len(), 1);
    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(
        c2.start_ms, 1_000,
        "leave-gap deletion must not move any other clip"
    );
}

#[test]
fn delete_close_gap_ripples_only_its_track() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.tracks.push(track("a1", TrackKind::Audio, false));
    project.assets.push(asset("va1", AssetKind::Video, 5_000));
    project.assets.push(asset("aa1", AssetKind::Audio, 5_000));
    project.clips.push(clip("c1", "v1", "va1", 0, 0, 500));
    project.clips.push(clip("c2", "v1", "va1", 1_000, 0, 500));
    project.clips.push(clip("ca1", "a1", "aa1", 1_000, 0, 500));

    let (candidate, _) = delete_clips(
        &project,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: true,
        },
    )
    .unwrap();

    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(
        c2.start_ms, 500,
        "c2 must shift earlier by c1's 500ms output duration"
    );
    let ca1 = candidate.clips.iter().find(|c| c.id == "ca1").unwrap();
    assert_eq!(
        ca1.start_ms, 1_000,
        "a clip on a1 must not ripple -- deletion happened on v1 only"
    );
}

#[test]
fn ripple_through_a_group_is_refused_atomically() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.tracks.push(track("a1", TrackKind::Audio, false));
    project.assets.push(asset("va1", AssetKind::Video, 5_000));
    project.assets.push(asset("aa1", AssetKind::Audio, 5_000));
    project.clips.push(clip("c1", "v1", "va1", 0, 0, 500));
    let mut c2 = clip("c2", "v1", "va1", 1_000, 0, 500);
    c2.group_id = Some("g1".into());
    project.clips.push(c2);
    let mut ca1 = clip("ca1", "a1", "aa1", 1_000, 0, 500);
    ca1.group_id = Some("g1".into());
    project.clips.push(ca1);

    let err = delete_clips(
        &project,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: true,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("g1"), "{}", err.message);
    assert_eq!(
        project.clips.len(),
        3,
        "a refused ripple must leave the caller's project untouched"
    );
}

#[test]
fn delete_close_gap_refuses_when_a_transition_spans_the_gap() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 500));
    project.clips.push(clip("c2", "v1", "a1", 500, 0, 500));
    project.transitions.push(Transition {
        id: "t1".into(),
        from: "c1".into(),
        to: "c2".into(),
        duration_ms: 100,
        kind: TransitionKind::Dissolve,
        extra: Map::new(),
    });

    let err = delete_clips(
        &project,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: true,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("transition"), "{}", err.message);
}

// moveClips and reorderClip tests (plus the locked-track table test that
// spans all four track-locking commands) live in the sibling
// `clips_move_reorder_tests.rs` -- this file itself was pushing past the
// 800-nonblank-line Rust cap once the overflow regression tests landed,
// the same `#[path]` split precedent `clips.rs` itself already uses.
#[path = "clips_move_reorder_tests.rs"]
mod move_reorder_tests;
