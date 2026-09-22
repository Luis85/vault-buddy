//! Tests for `groups.rs`'s five commands (not inline, the `clips.rs`/
//! `clips_tests.rs` precedent).

use std::collections::HashSet;

use super::*;
use crate::editor::commands::payloads::ClipboardFragment;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::model_cues::{CaptionCue, Effect, EffectKind, Marker};
use crate::editor::test_support::{asset, clip, minimal_project, track};

fn base_project() -> Project {
    minimal_project()
}

fn num(v: i64) -> Num {
    Num::from(v)
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

fn marker(id: &str, clip_id: &str, source_ms: u64) -> Marker {
    Marker {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        source_ms,
        title: "M".to_string(),
        extra: Map::new(),
    }
}

/// Every entity id across every collection this task's commands touch --
/// `duplicate_mints_fresh_ids_everywhere`'s own vacuity/mutation guard.
fn collect_all_ids(project: &Project) -> HashSet<String> {
    let mut ids = HashSet::new();
    for c in &project.clips {
        ids.insert(c.id.clone());
    }
    for e in &project.effects {
        ids.insert(e.id.clone());
    }
    for m in &project.markers {
        ids.insert(m.id.clone());
    }
    if let Some(captions) = &project.captions {
        for cue in &captions.cues {
            ids.insert(cue.id.clone());
        }
    }
    ids
}

// ---- groupClips / ungroupClips -------------------------------------------

#[test]
fn group_clips_requires_at_least_two_clips_and_mints_a_fresh_group_id() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 200));
    project.clips.push(clip("c2", "v1", "a1", 300, 0, 200));

    let err = group_clips(
        &project,
        &GroupClipsPayload {
            clip_ids: vec!["c1".into()],
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    let (candidate, label) = group_clips(
        &project,
        &GroupClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
        },
    )
    .unwrap();
    assert_eq!(label, "Group clips");
    let c1 = candidate.clips.iter().find(|c| c.id == "c1").unwrap();
    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert!(c1.group_id.is_some());
    assert_eq!(c1.group_id, c2.group_id);
}

#[test]
fn group_clips_refuses_when_a_named_clip_is_on_a_locked_track() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, true));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 200));
    project.clips.push(clip("c2", "v1", "a1", 300, 0, 200));

    let err = group_clips(
        &project,
        &GroupClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("locked"), "{}", err.message);
}

#[test]
fn ungroup_clips_clears_the_group_id_from_every_member() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c1 = clip("c1", "v1", "a1", 0, 0, 200);
    c1.group_id = Some("g1".into());
    project.clips.push(c1);
    let mut c2 = clip("c2", "v1", "a1", 300, 0, 200);
    c2.group_id = Some("g1".into());
    project.clips.push(c2);

    let (candidate, label) = ungroup_clips(
        &project,
        &UngroupClipsPayload {
            group_id: "g1".into(),
        },
    )
    .unwrap();
    assert_eq!(label, "Ungroup clips");
    assert!(candidate.clips.iter().all(|c| c.group_id.is_none()));
}

#[test]
fn ungroup_clips_refuses_an_unknown_group_id() {
    let project = base_project();
    let err = ungroup_clips(
        &project,
        &UngroupClipsPayload {
            group_id: "nope".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- duplicateClips ---------------------------------------------------------

#[test]
fn duplicate_mints_fresh_ids_everywhere() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c1 = clip("c1", "v1", "a1", 0, 0, 500);
    c1.group_id = Some("g1".into());
    project.clips.push(c1);
    let mut c2 = clip("c2", "v1", "a1", 600, 0, 200);
    c2.group_id = Some("g1".into());
    project.clips.push(c2);
    project.effects.push(effect("e1", "c1", 0, 100));
    project.markers.push(marker("m1", "c2", 50));
    project.captions = Some(crate::editor::model_cues::CaptionSettings {
        enabled: true,
        burn_in: false,
        font_size: num(16),
        position: CaptionPosition::Bottom,
        background: false,
        cues: vec![caption("cap1", "c1", 0, 100)],
        extra: Map::new(),
    });

    let existing_ids = collect_all_ids(&project);

    let (candidate, label) = duplicate_clips(
        &project,
        &DuplicateClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            offset_ms: 2_000,
        },
    )
    .unwrap();
    assert_eq!(label, "Duplicate clips");

    let new_ids = collect_all_ids(&candidate);
    for id in &existing_ids {
        assert!(new_ids.contains(id), "original id {id} vanished");
    }
    let added = new_ids.len() - existing_ids.len();
    assert_eq!(
        added,
        new_ids.difference(&existing_ids).count(),
        "duplication must add only BRAND NEW ids -- a reused source id \
         (the mutation this test guards against) would shrink this count \
         below the number of entities actually duplicated"
    );
    assert_eq!(candidate.clips.len(), 4, "2 originals + 2 duplicates");
    assert_eq!(candidate.effects.len(), 2, "1 original + 1 duplicate");
    assert_eq!(candidate.markers.len(), 2, "1 original + 1 duplicate");
    assert_eq!(
        candidate.captions.as_ref().unwrap().cues.len(),
        2,
        "1 original + 1 duplicate"
    );

    let dup1 = candidate
        .clips
        .iter()
        .find(|c| c.start_ms == 2_000)
        .expect("c1's duplicate");
    let dup2 = candidate
        .clips
        .iter()
        .find(|c| c.start_ms == 2_600)
        .expect("c2's duplicate");
    assert_eq!(
        dup1.group_id, dup2.group_id,
        "the two duplicates must share a FRESH common group id"
    );
    assert_ne!(
        dup1.group_id.as_deref(),
        Some("g1"),
        "the duplicates' group id must be freshly minted, never the original g1"
    );

    let dup_effect = candidate
        .effects
        .iter()
        .find(|e| e.id != "e1")
        .expect("a duplicated effect");
    assert_eq!(dup_effect.clip_id, dup1.id);
    let dup_marker = candidate
        .markers
        .iter()
        .find(|m| m.id != "m1")
        .expect("a duplicated marker");
    assert_eq!(dup_marker.clip_id, dup2.id);
}

#[test]
fn duplicate_refuses_when_the_source_track_is_locked() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, true));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 200));

    let err = duplicate_clips(
        &project,
        &DuplicateClipsPayload {
            clip_ids: vec!["c1".into()],
            offset_ms: 500,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("locked"), "{}", err.message);
}

#[test]
fn duplicate_refuses_an_overlap_at_the_new_position() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 200));
    project.clips.push(clip("blocker", "v1", "a1", 300, 0, 200));

    let err = duplicate_clips(
        &project,
        &DuplicateClipsPayload {
            clip_ids: vec!["c1".into()],
            offset_ms: 300,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert_eq!(
        project.clips.len(),
        2,
        "a refused duplicate must leave the caller's project untouched"
    );
}

// ---- pasteFragment ----------------------------------------------------------

#[test]
fn paste_repoints_cues_to_new_clips() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.tracks.push(track("v2", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 100, 0, 200));
    project.effects.push(effect("e1", "c1", 0, 100));
    project.markers.push(marker("m1", "c1", 50));
    project.captions = Some(crate::editor::model_cues::CaptionSettings {
        enabled: true,
        burn_in: false,
        font_size: num(16),
        position: CaptionPosition::Bottom,
        background: false,
        cues: vec![caption("cap1", "c1", 0, 100)],
        extra: Map::new(),
    });

    let fragment = ClipboardFragment {
        clips: vec![clip("c1", "v1", "a1", 100, 0, 200)],
        effects: vec![effect("e1", "c1", 0, 100)],
        captions: vec![caption("cap1", "c1", 0, 100)],
        markers: vec![marker("m1", "c1", 50)],
        origin_ms: 100,
    };

    let (candidate, label) = paste_fragment(
        &project,
        &PasteFragmentPayload {
            fragment,
            track_id: "v2".into(),
            at_ms: 1_000,
        },
    )
    .unwrap();
    assert_eq!(label, "Paste");

    let pasted = candidate.clips.iter().find(|c| c.id != "c1").expect(
        "a pasted clip must exist with a FRESH id -- the mutation \
                 (reusing the source clip's own id) would leave every clip \
                 sharing id \"c1\" and this find() would return None",
    );
    assert_eq!(pasted.track_id, "v2");
    assert_eq!(pasted.start_ms, 1_000);

    let pasted_effect = candidate
        .effects
        .iter()
        .find(|e| e.id != "e1")
        .expect("a pasted effect");
    assert_eq!(
        pasted_effect.clip_id, pasted.id,
        "the pasted effect must be re-pointed onto the FRESH clip id, not the original"
    );

    let pasted_marker = candidate
        .markers
        .iter()
        .find(|m| m.id != "m1")
        .expect("a pasted marker");
    assert_eq!(pasted_marker.clip_id, pasted.id);

    let pasted_caption = candidate
        .captions
        .as_ref()
        .unwrap()
        .cues
        .iter()
        .find(|c| c.id != "cap1")
        .expect("a pasted caption");
    assert_eq!(pasted_caption.clip_id, pasted.id);
}

#[test]
fn paste_of_unknown_asset_is_source_missing() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    // Deliberately NO asset "a1" registered in this project.
    let fragment = ClipboardFragment {
        clips: vec![clip("c1", "v1", "a1", 0, 0, 200)],
        effects: Vec::new(),
        captions: Vec::new(),
        markers: Vec::new(),
        origin_ms: 0,
    };

    let err = paste_fragment(
        &project,
        &PasteFragmentPayload {
            fragment,
            track_id: "v1".into(),
            at_ms: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::SourceMissing);
}

#[test]
fn paste_refuses_onto_a_locked_track() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, true));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let fragment = ClipboardFragment {
        clips: vec![clip("c1", "v1", "a1", 0, 0, 200)],
        effects: Vec::new(),
        captions: Vec::new(),
        markers: Vec::new(),
        origin_ms: 0,
    };

    let err = paste_fragment(
        &project,
        &PasteFragmentPayload {
            fragment,
            track_id: "v1".into(),
            at_ms: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("locked"), "{}", err.message);
}

#[test]
fn paste_refuses_an_overlap_on_the_target_track() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project
        .clips
        .push(clip("blocker", "v1", "a1", 1_000, 0, 200));
    let fragment = ClipboardFragment {
        clips: vec![clip("c1", "v1", "a1", 0, 0, 300)],
        effects: Vec::new(),
        captions: Vec::new(),
        markers: Vec::new(),
        origin_ms: 0,
    };

    let err = paste_fragment(
        &project,
        &PasteFragmentPayload {
            fragment,
            track_id: "v1".into(),
            at_ms: 1_000,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn paste_refuses_a_kind_mismatched_track() {
    let mut project = base_project();
    project.tracks.push(track("a1", TrackKind::Audio, false));
    project.assets.push(asset("va1", AssetKind::Video, 5_000));
    let fragment = ClipboardFragment {
        clips: vec![clip("c1", "v1", "va1", 0, 0, 200)],
        effects: Vec::new(),
        captions: Vec::new(),
        markers: Vec::new(),
        origin_ms: 0,
    };

    let err = paste_fragment(
        &project,
        &PasteFragmentPayload {
            fragment,
            track_id: "a1".into(),
            at_ms: 0,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
}

// ---- cutClips ---------------------------------------------------------------

#[test]
fn cut_clips_deletes_like_delete_clips_with_the_label_cut() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 500));
    project.clips.push(clip("c2", "v1", "a1", 1_000, 0, 200));

    let (candidate, label) = cut_clips(
        &project,
        &CutClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: true,
        },
    )
    .unwrap();
    assert_eq!(label, "Cut", "cutClips must relabel the undo entry \"Cut\"");
    assert_eq!(candidate.clips.len(), 1);
    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(
        c2.start_ms, 500,
        "cutClips must share deleteClips' own closeGap ripple semantics"
    );
}
