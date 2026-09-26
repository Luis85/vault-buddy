//! `duplicateClips`/`pasteFragment` carrying a crossfade (GAP-178, ADR §9
//! (p)): a copied pair keeps its transition, re-pointed at the fresh clip
//! ids; the overlap that transition explains is the ONLY in-batch overlap
//! accepted. A sibling of `groups_tests.rs` so neither file nears the cap.
//!
//! Asymmetric fixture (the "fixture flaw" rule): p1 is 1200 ms, p2 1000 ms
//! from a non-zero in point, dissolved over 400 ms, so a copy that kept the
//! original ids, dropped the shift or swapped `from`/`to` cannot pass.

use super::*;
use crate::editor::commands::payloads::{AddTransitionPayload, ClipboardFragment};
use crate::editor::commands::transitions::add_transition;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::model_cues::{Transition, TransitionKind};
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::validate_project;

/// v1: p1 [0,1200) dissolved into p2 over 400 ms (p2 then starts at 800),
/// plus x1, which the transition's shift moves from 3000 to [2600,3100).
fn crossfaded() -> Project {
    let mut p = minimal_project();
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.assets.push(asset("av", AssetKind::Video, 60_000));
    p.assets.push(asset("aa", AssetKind::Audio, 60_000));
    p.clips.push(clip("p1", "v1", "av", 0, 0, 1_200));
    p.clips.push(clip("p2", "v1", "av", 1_200, 300, 1_300));
    p.clips.push(clip("x1", "v1", "av", 3_000, 0, 500));
    let (p, _) = add_transition(
        &p,
        &AddTransitionPayload {
            from_clip_id: "p1".into(),
            to_clip_id: "p2".into(),
            duration_ms: 400,
            kind: TransitionKind::Dissolve,
        },
    )
    .unwrap();
    validate_project(&p).expect("the crossfaded fixture is valid");
    p
}

fn clip_of<'a>(p: &'a Project, id: &str) -> &'a Clip {
    p.clips.iter().find(|c| c.id == id).unwrap()
}

/// The one transition in `candidate` that `before` did not have.
fn new_transition(before: &Project, candidate: &Project) -> Transition {
    let old: HashSet<&str> = before.transitions.iter().map(|t| t.id.as_str()).collect();
    let added: Vec<&Transition> = candidate
        .transitions
        .iter()
        .filter(|t| !old.contains(t.id.as_str()))
        .collect();
    assert_eq!(added.len(), 1, "exactly one carried transition");
    added[0].clone()
}

fn fragment_of(p: &Project, ids: &[&str], transitions: Vec<Transition>) -> ClipboardFragment {
    ClipboardFragment {
        clips: p
            .clips
            .iter()
            .filter(|c| ids.contains(&c.id.as_str()))
            .cloned()
            .collect(),
        effects: Vec::new(),
        captions: Vec::new(),
        markers: Vec::new(),
        transitions,
        origin_ms: 0,
    }
}

fn paste(p: &Project, fragment: ClipboardFragment, at_ms: u64) -> Result<Project, EditorError> {
    paste_fragment(
        p,
        &PasteFragmentPayload {
            fragment,
            track_id: "v1".into(),
            at_ms,
        },
    )
    .map(|(candidate, _)| candidate)
}

/// The copies of p1/p2 in `candidate`: the clips at the two shifted starts.
fn copies(candidate: &Project, from_start: u64) -> (String, String) {
    let at = |start: u64| {
        candidate
            .clips
            .iter()
            .find(|c| c.start_ms == start && !["p1", "p2"].contains(&c.id.as_str()))
            .unwrap_or_else(|| panic!("a copy starting at {start}"))
            .id
            .clone()
    };
    (at(from_start), at(from_start + 800))
}

#[test]
fn duplicating_a_crossfaded_pair_carries_its_transition() {
    let before = crossfaded();
    let (candidate, label) = duplicate_clips(
        &before,
        &DuplicateClipsPayload {
            clip_ids: vec!["p1".into(), "p2".into()],
            offset_ms: 5_000,
        },
    )
    .expect("the copies overlap exactly as their transition says");
    assert_eq!(label, "Duplicate clips");

    let (from, to) = copies(&candidate, 5_000);
    let t = new_transition(&before, &candidate);
    assert_eq!(
        (t.from.as_str(), t.to.as_str()),
        (from.as_str(), to.as_str())
    );
    assert_eq!(t.duration_ms, 400);
    assert_eq!(t.kind, TransitionKind::Dissolve);
    assert_ne!(t.id, before.transitions[0].id, "a fresh transition id");
    assert_eq!(candidate.transitions.len(), 2, "the original stays");
    validate_project(&candidate).expect("the duplicate is a valid project");
}

#[test]
fn duplicating_one_clip_of_a_pair_carries_no_transition() {
    let before = crossfaded();
    let (candidate, _) = duplicate_clips(
        &before,
        &DuplicateClipsPayload {
            clip_ids: vec!["p1".into()],
            offset_ms: 5_000,
        },
    )
    .unwrap();
    assert_eq!(candidate.transitions, before.transitions);
}

#[test]
fn pasting_a_copied_crossfaded_pair_carries_its_transition() {
    let before = crossfaded();
    let fragment = fragment_of(&before, &["p1", "p2"], before.transitions.clone());
    let candidate = paste(&before, fragment, 6_000).expect("the pasted pair is crossfaded");

    let (from, to) = copies(&candidate, 6_000);
    let t = new_transition(&before, &candidate);
    assert_eq!(
        (t.from.as_str(), t.to.as_str()),
        (from.as_str(), to.as_str())
    );
    assert_eq!(t.duration_ms, 400);
    assert_eq!(clip_of(&candidate, &to).start_ms, 6_800);
    validate_project(&candidate).expect("the paste is a valid project");
}

#[test]
fn a_fragment_transition_naming_a_clip_outside_the_fragment_is_refused() {
    let before = crossfaded();
    let fragment = fragment_of(&before, &["p1"], before.transitions.clone());
    let err = paste(&before, fragment, 6_000).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert_eq!(
        err.message,
        format!(
            "transition {} joins clip p2, which is not in the fragment",
            before.transitions[0].id
        )
    );
}

#[test]
fn an_overlap_no_carried_transition_explains_is_still_refused() {
    let before = crossfaded();
    // The same crossfaded pair, but its transition left behind.
    let fragment = fragment_of(&before, &["p1", "p2"], Vec::new());
    let err = paste(&before, fragment, 6_000).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message.contains("would overlap clip"),
        "{}",
        err.message
    );

    // A carried transition explains ITS pair only: pasting it over x1
    // still collides with the existing clip.
    let fragment = fragment_of(&before, &["p1", "p2"], before.transitions.clone());
    let err = paste(&before, fragment, 2_500).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("would overlap x1"), "{}", err.message);
}

#[test]
fn a_carried_transition_whose_geometry_does_not_hold_is_refused() {
    let before = crossfaded();
    let mut t = before.transitions[0].clone();
    t.duration_ms = 300; // the clips overlap by 400
    let fragment = fragment_of(&before, &["p1", "p2"], vec![t]);
    let err = paste(&before, fragment, 6_000).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message.contains("must end exactly 300 ms after clip"),
        "{}",
        err.message
    );
}

#[test]
fn a_carried_transition_of_the_wrong_kind_or_reusing_a_side_is_refused() {
    let before = crossfaded();
    let mut wrong_kind = before.transitions[0].clone();
    wrong_kind.kind = TransitionKind::EqualPower;
    let err = paste(
        &before,
        fragment_of(&before, &["p1", "p2"], vec![wrong_kind]),
        6_000,
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message
            .contains("needs a Dissolve transition, not EqualPower"),
        "{}",
        err.message
    );

    let twice = vec![before.transitions[0].clone(), before.transitions[0].clone()];
    let err = paste(&before, fragment_of(&before, &["p1", "p2"], twice), 6_000).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message
            .contains("already has a transition on this side"),
        "{}",
        err.message
    );
}
