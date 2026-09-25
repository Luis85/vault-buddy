//! Tests for `tracks.rs`'s five commands (not inline, the `clips.rs`/
//! `clips_tests.rs` precedent — this file mirrors `groups_tests.rs`'s own
//! fixture-building style).

use super::*;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::model_cues::{
    CaptionCue, CaptionPosition, CaptionSettings, Effect, EffectKind, Marker, Transition,
    TransitionKind,
};
use crate::editor::test_support::{asset, clip, minimal_project, track};

fn base_project() -> Project {
    minimal_project()
}

fn effect(id: &str, clip_id: &str) -> Effect {
    Effect {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        kind: EffectKind::Highlight,
        start_ms: 0,
        end_ms: 100,
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

fn caption_cue(id: &str, clip_id: &str) -> CaptionCue {
    CaptionCue {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        start_ms: 0,
        end_ms: 100,
        text: "Hi".to_string(),
        extra: Map::new(),
    }
}

fn marker(id: &str, clip_id: &str) -> Marker {
    Marker {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        source_ms: 0,
        title: "M".to_string(),
        extra: Map::new(),
    }
}

fn transition(id: &str, from: &str, to: &str) -> Transition {
    Transition {
        id: id.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        duration_ms: 500,
        kind: TransitionKind::Dissolve,
        extra: Map::new(),
    }
}

// ---- locked track: rename/move/delete/flags -------------------------------

#[test]
fn locked_track_refuses_rename_move_delete_and_flag_changes_but_allows_unlock() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, true));
    project.tracks.push(track("v2", TrackKind::Video, false));

    let err = rename_track(
        &project,
        &RenameTrackPayload {
            track_id: "v1".into(),
            name: "New name".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    let err = move_track(
        &project,
        &MoveTrackPayload {
            track_id: "v1".into(),
            to_index: 1,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    let err = delete_track(
        &project,
        &DeleteTrackPayload {
            track_id: "v1".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    // Any OTHER flag change is refused while locked.
    let err = set_track_flags(
        &project,
        &SetTrackFlagsPayload {
            track_id: "v1".into(),
            visible: Some(false),
            locked: None,
            muted: None,
            solo: None,
            volume: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    // Unlocking, alone, is always allowed.
    let (candidate, _label) = set_track_flags(
        &project,
        &SetTrackFlagsPayload {
            track_id: "v1".into(),
            visible: None,
            locked: Some(false),
            muted: None,
            solo: None,
            volume: None,
        },
    )
    .unwrap();
    let v1 = candidate.tracks.iter().find(|t| t.id == "v1").unwrap();
    assert!(!v1.locked);
}

#[test]
fn set_track_flags_refuses_a_combined_unlock_and_other_change_while_locked() {
    // Unlocking in the SAME call that also changes another flag is still
    // refused -- the "other field" is what a locked track blocks, not the
    // `locked` field's own presence.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, true));

    let err = set_track_flags(
        &project,
        &SetTrackFlagsPayload {
            track_id: "v1".into(),
            visible: None,
            locked: Some(false),
            muted: Some(true),
            solo: None,
            volume: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- deleteTrack -----------------------------------------------------------

#[test]
fn delete_track_removes_its_clips_and_cues_in_one_undo() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.tracks.push(track("v2", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 200));
    project.clips.push(clip("c2", "v1", "a1", 300, 0, 200));
    // An untouched clip on the OTHER track must survive.
    project.clips.push(clip("c3", "v2", "a1", 0, 0, 200));
    project.effects.push(effect("e1", "c1"));
    project.effects.push(effect("e2", "c3"));
    project.markers.push(marker("m1", "c2"));
    project.transitions.push(transition("tr1", "c1", "c2"));
    project.captions = Some(CaptionSettings {
        enabled: true,
        burn_in: false,
        font_size: num(24),
        position: CaptionPosition::Bottom,
        background: true,
        cues: vec![caption_cue("cap1", "c1"), caption_cue("cap2", "c3")],
        extra: Map::new(),
    });

    let (candidate, label) = delete_track(
        &project,
        &DeleteTrackPayload {
            track_id: "v1".into(),
        },
    )
    .unwrap();

    assert_eq!(label, "Delete track v1");
    assert!(!candidate.tracks.iter().any(|t| t.id == "v1"));
    assert!(candidate.tracks.iter().any(|t| t.id == "v2"));
    assert!(!candidate.clips.iter().any(|c| c.id == "c1" || c.id == "c2"));
    assert!(
        candidate.clips.iter().any(|c| c.id == "c3"),
        "untouched track's clip must survive"
    );
    assert!(!candidate.effects.iter().any(|e| e.id == "e1"));
    assert!(candidate.effects.iter().any(|e| e.id == "e2"));
    assert!(!candidate.markers.iter().any(|m| m.id == "m1"));
    assert!(!candidate.transitions.iter().any(|t| t.id == "tr1"));
    let cues = &candidate.captions.as_ref().unwrap().cues;
    assert!(!cues.iter().any(|c| c.id == "cap1"));
    assert!(cues.iter().any(|c| c.id == "cap2"));
}

#[test]
fn delete_track_rejects_an_unknown_track() {
    let project = base_project();
    let err = delete_track(
        &project,
        &DeleteTrackPayload {
            track_id: "nope".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- moveTrack --------------------------------------------------------------

#[test]
fn move_track_reorders_compositing_order() {
    // Asymmetric fixture (distinct kinds/names/positions) -- a swapped
    // index arithmetic must not accidentally look correct on a uniform set.
    let mut project = base_project();
    project
        .tracks
        .push(track("v-front", TrackKind::Video, false));
    project.tracks.push(track("a-mid", TrackKind::Audio, false));
    project
        .tracks
        .push(track("v-back", TrackKind::Video, false));

    let (candidate, _label) = move_track(
        &project,
        &MoveTrackPayload {
            track_id: "v-back".into(),
            to_index: 0,
        },
    )
    .unwrap();

    let order: Vec<&str> = candidate.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(order, vec!["v-back", "v-front", "a-mid"]);
}

#[test]
fn move_track_clamps_an_out_of_range_index_to_the_end() {
    let mut project = base_project();
    project.tracks.push(track("t1", TrackKind::Video, false));
    project.tracks.push(track("t2", TrackKind::Video, false));
    project.tracks.push(track("t3", TrackKind::Video, false));

    let (candidate, _label) = move_track(
        &project,
        &MoveTrackPayload {
            track_id: "t1".into(),
            to_index: 999,
        },
    )
    .unwrap();

    let order: Vec<&str> = candidate.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(order, vec!["t2", "t3", "t1"]);
}

#[test]
fn move_track_rejects_an_unknown_track() {
    let project = base_project();
    let err = move_track(
        &project,
        &MoveTrackPayload {
            track_id: "nope".into(),
            to_index: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- renameTrack ------------------------------------------------------------

#[test]
fn rename_track_trims_and_updates_name() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));

    let (candidate, label) = rename_track(
        &project,
        &RenameTrackPayload {
            track_id: "v1".into(),
            name: "  Webcam  ".into(),
        },
    )
    .unwrap();
    assert_eq!(candidate.tracks[0].name, "Webcam");
    assert_eq!(label, "Rename track");
}

#[test]
fn rename_track_rejects_empty_after_trim() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));

    let err = rename_track(
        &project,
        &RenameTrackPayload {
            track_id: "v1".into(),
            name: "   ".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn rename_track_does_not_itself_reject_an_overlong_name() {
    // The 200-char maximum is validate_project's job (`validate::
    // TRACK_NAME_MAX`), deferred for the same atomicity reason
    // `meta::rename`'s own doc comment gives for the title's 160-char max.
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    let long = "x".repeat(250);

    let (candidate, _label) = rename_track(
        &project,
        &RenameTrackPayload {
            track_id: "v1".into(),
            name: long.clone(),
        },
    )
    .unwrap();
    assert_eq!(candidate.tracks[0].name, long);
}

#[test]
fn rename_track_rejects_an_unknown_track() {
    let project = base_project();
    let err = rename_track(
        &project,
        &RenameTrackPayload {
            track_id: "nope".into(),
            name: "New".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- setTrackFlags (unlocked) ------------------------------------------------

#[test]
fn set_track_flags_updates_every_field_when_unlocked() {
    let mut project = base_project();
    project.tracks.push(track("a1", TrackKind::Audio, false));

    let (candidate, label) = set_track_flags(
        &project,
        &SetTrackFlagsPayload {
            track_id: "a1".into(),
            visible: Some(false),
            locked: None,
            muted: Some(true),
            solo: Some(true),
            volume: Some(Num::from_f64(0.5).unwrap()),
        },
    )
    .unwrap();
    let t = &candidate.tracks[0];
    assert!(!t.visible);
    assert!(t.muted);
    assert!(t.solo);
    assert_eq!(t.volume.as_f64(), Some(0.5));
    assert_eq!(label, "Update track");
}

#[test]
fn set_track_flags_rejects_an_unknown_track() {
    let project = base_project();
    let err = set_track_flags(
        &project,
        &SetTrackFlagsPayload {
            track_id: "nope".into(),
            visible: Some(true),
            locked: None,
            muted: None,
            solo: None,
            volume: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- addTrack ----------------------------------------------------------------

#[test]
fn add_track_mints_a_prefixed_id_and_inserts_at_the_frontmost_index() {
    let mut project = base_project();
    project
        .tracks
        .push(track("existing", TrackKind::Video, false));

    let (candidate, label) = add_track(
        &project,
        &AddTrackPayload {
            kind: TrackKind::Video,
            name: "  Webcam  ".into(),
            index: 0,
        },
    )
    .unwrap();

    assert_eq!(candidate.tracks.len(), 2);
    let new_track = &candidate.tracks[0];
    assert!(
        new_track.id.starts_with("trk-"),
        "id must carry the trk- prefix: {}",
        new_track.id
    );
    assert_eq!(new_track.name, "Webcam");
    assert_eq!(new_track.kind, TrackKind::Video);
    assert!(new_track.visible);
    assert!(!new_track.locked);
    assert!(!new_track.muted);
    assert!(!new_track.solo);
    assert_eq!(new_track.volume.as_f64(), Some(1.0));
    assert_eq!(candidate.tracks[1].id, "existing");
    assert_eq!(label, "Add track");
}

#[test]
fn add_track_clamps_an_out_of_range_index_to_the_end() {
    let mut project = base_project();
    project
        .tracks
        .push(track("existing", TrackKind::Audio, false));

    let (candidate, _label) = add_track(
        &project,
        &AddTrackPayload {
            kind: TrackKind::Audio,
            name: "New".into(),
            index: 999,
        },
    )
    .unwrap();

    assert_eq!(candidate.tracks[0].id, "existing");
    assert_eq!(candidate.tracks[1].name, "New");
}

#[test]
fn add_track_rejects_empty_name_after_trim() {
    let project = base_project();
    let err = add_track(
        &project,
        &AddTrackPayload {
            kind: TrackKind::Video,
            name: "   ".into(),
            index: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn track_limit_is_enforced() {
    let mut project = base_project();
    for i in 0..limits::MAX_TRACKS {
        project
            .tracks
            .push(track(&format!("t{i}"), TrackKind::Video, false));
    }
    assert_eq!(project.tracks.len(), limits::MAX_TRACKS);

    // 33rd -> Err.
    let err = add_track(
        &project,
        &AddTrackPayload {
            kind: TrackKind::Video,
            name: "One too many".into(),
            index: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}
