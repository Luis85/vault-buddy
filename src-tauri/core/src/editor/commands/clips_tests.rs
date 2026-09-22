//! Tests for `clips.rs`'s seven clip commands (not inline, the
//! `validate.rs`/`validate_tests.rs` precedent). At least one fixture
//! carries a non-unit speed and cues are deliberately asymmetric
//! (straddling/boundary cases, never a centered cue that could pass by
//! accident).

use super::*;
use crate::editor::model::AssetKind;
use crate::editor::model_cues::{
    CaptionCue, CaptionPosition, CaptionSettings, Effect, EffectKind, Marker, Transition,
    TransitionKind,
};
use crate::editor::test_support::minimal_project;

fn num_f(v: f64) -> Num {
    Num::from_f64(v).expect("finite test fixture value")
}

fn base_project() -> Project {
    minimal_project()
}

fn track(id: &str, kind: TrackKind, locked: bool) -> Track {
    Track {
        id: id.to_string(),
        kind,
        name: id.to_string(),
        visible: true,
        locked,
        muted: false,
        solo: false,
        volume: num(1),
        extra: Map::new(),
    }
}

fn asset(id: &str, kind: AssetKind, duration_ms: u64) -> Asset {
    Asset {
        id: id.to_string(),
        kind,
        name: format!("Asset {id}"),
        duration_ms,
        width: None,
        height: None,
        size: None,
        builtin: None,
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    }
}

fn clip(id: &str, track_id: &str, asset_id: &str, start_ms: u64, in_ms: u64, out_ms: u64) -> Clip {
    Clip {
        id: id.to_string(),
        asset_id: asset_id.to_string(),
        track_id: track_id.to_string(),
        name: id.to_string(),
        start_ms,
        in_ms,
        out_ms,
        fade_in_ms: 0,
        fade_out_ms: 0,
        fade_curve: FadeCurve::Linear,
        opacity: num(1),
        volume: num(1),
        muted: false,
        x: num(0),
        y: num(0),
        w: num(1),
        h: num(1),
        speed: None,
        rotation: None,
        frame_shape: None,
        fit: None,
        mirror: None,
        flip_y: None,
        preserve_pitch: None,
        group_id: None,
        crop_zoom: None,
        crop_x: None,
        crop_y: None,
        adjustments: None,
        card: None,
        extra: Map::new(),
    }
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
    project.assets.push(asset("a1", AssetKind::Video, 200));
    let mut c1 = clip("c1", "v1", "a1", 0, 0, 100);
    c1.speed = Some(num_f(2.0));
    project.clips.push(c1);

    let before = time::project_duration(&project);
    assert_eq!(before, 50);

    let (candidate, _) = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 13,
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
