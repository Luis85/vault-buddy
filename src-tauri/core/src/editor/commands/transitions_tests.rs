//! Tests for `transitions.rs` (Task 30; F-19) and for the F12 changes it
//! makes to `clips.rs` (`deleteClips`' transition-aware ripple, the
//! narrowly scoped overlap allowance `trimClip`/`moveClips` share, and the
//! geometry guard every timing command now runs). Sibling file, the
//! `clips.rs`/`clips_tests.rs` precedent.
//!
//! One asymmetric fixture throughout (the "fixture flaw" rule): v1 holds
//! five clips of five different lengths, c2 at a NON-UNIT speed (1.5x, so
//! its output duration 2000ms is not its 3000ms source span), with a gap
//! between c2 and c3; v2 and the audio track a1 hold clips that sit at
//! times a wrongly-scoped shift would visibly move.

use super::*;
use crate::editor::commands::clips::{
    delete_clips, move_clips, reorder_clip, split_clip, trim_clip,
};
use crate::editor::commands::payloads::{
    DeleteClipsPayload, MoveClipsPayload, ReorderClipPayload, ReorderDirection, SplitClipPayload,
    TrimClipPayload,
};
use crate::editor::error::EditorErrorCode;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::{validate_project, Num};

/// v1: c0 [0,500) c1 [500,2500) c2 [2500,4500) (1.5x) c3 [6000,7500)
/// c4 [8000,9000); v2: x1 [2500,3700); a1 (audio): y1 [3000,4100)
/// y2 [4100,5000).
fn project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("av", AssetKind::Video, 60_000));
    p.assets.push(asset("aa", AssetKind::Audio, 60_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("v2", TrackKind::Video, false));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.clips.push(clip("c0", "v1", "av", 0, 0, 500));
    p.clips.push(clip("c1", "v1", "av", 500, 100, 2_100));
    let mut c2 = clip("c2", "v1", "av", 2_500, 3_000, 6_000);
    c2.speed = Some(Num::from_f64(1.5).unwrap());
    p.clips.push(c2);
    p.clips.push(clip("c3", "v1", "av", 6_000, 500, 2_000));
    p.clips.push(clip("c4", "v1", "av", 8_000, 1_000, 2_000));
    p.clips.push(clip("x1", "v2", "av", 2_500, 0, 1_200));
    p.clips.push(clip("y1", "a1", "aa", 3_000, 0, 1_100));
    p.clips.push(clip("y2", "a1", "aa", 4_100, 200, 1_100));
    validate_project(&p).expect("the transition fixture itself is valid");
    p
}

fn add(
    p: &Project,
    from: &str,
    to: &str,
    duration_ms: u64,
    kind: TransitionKind,
) -> Result<(Project, String), EditorError> {
    add_transition(
        p,
        &AddTransitionPayload {
            from_clip_id: from.into(),
            to_clip_id: to.into(),
            duration_ms,
            kind,
        },
    )
}

/// `project()` with c1 -> c2 dissolved over 700ms, validated.
fn with_dissolve() -> Project {
    let (p, _) = add(&project(), "c1", "c2", 700, TransitionKind::Dissolve).unwrap();
    validate_project(&p).expect("an added transition is valid");
    p
}

fn start(p: &Project, id: &str) -> u64 {
    p.clips.iter().find(|c| c.id == id).unwrap().start_ms
}

fn starts(p: &Project, ids: &[&str]) -> Vec<u64> {
    ids.iter().map(|id| start(p, id)).collect()
}

const ALL: [&str; 8] = ["c0", "c1", "c2", "c3", "c4", "x1", "y1", "y2"];

// ---- the brief's named tests ------------------------------------------------

#[test]
fn transition_shortens_only_its_track() {
    let (p, label) = add(&project(), "c1", "c2", 700, TransitionKind::Dissolve).unwrap();
    validate_project(&p).unwrap();
    assert_eq!(label, "Add transition");
    // c2 and everything after it on v1 moved 700ms earlier; c0/c1 stayed,
    // and NOTHING on v2 or a1 moved (a shift applied to every track would
    // have moved x1/y1/y2, all of which start at or after c2's 2500ms).
    assert_eq!(
        starts(&p, &ALL),
        vec![0, 500, 1_800, 5_300, 7_300, 2_500, 3_000, 4_100]
    );
    assert_eq!(p.transitions.len(), 1);
    let t = &p.transitions[0];
    assert_eq!((t.from.as_str(), t.to.as_str()), ("c1", "c2"));
    assert_eq!(t.duration_ms, 700);
    assert_eq!(t.kind, TransitionKind::Dissolve);
    assert!(t.id.starts_with("transition-"), "{}", t.id);
}

#[test]
fn remove_restores_the_original_spacing() {
    let p = with_dissolve();
    let id = p.transitions[0].id.clone();
    let (restored, label) =
        remove_transition(&p, &RemoveTransitionPayload { transition_id: id }).unwrap();
    validate_project(&restored).unwrap();
    assert_eq!(label, "Remove transition");
    assert!(restored.transitions.is_empty());
    assert_eq!(
        starts(&restored, &ALL),
        vec![0, 500, 2_500, 6_000, 8_000, 2_500, 3_000, 4_100]
    );
}

#[test]
fn second_transition_on_the_same_side_is_refused() {
    let p = with_dissolve();

    // c1's OUT side is taken: refused on that basis (the side check runs
    // before adjacency, so this message -- not "does not start where ...
    // ends" -- is what proves the side rule fired).
    let err = add(&p, "c1", "c3", 100, TransitionKind::Dissolve).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message.contains("c1") && err.message.contains("out side"),
        "{}",
        err.message
    );

    // c2's IN side is taken.
    let err = add(&p, "c0", "c2", 100, TransitionKind::Dissolve).unwrap_err();
    assert!(
        err.message.contains("c2") && err.message.contains("in side"),
        "{}",
        err.message
    );

    // One per SIDE, not one per clip: c1's IN side is still free, and c0
    // still ends exactly where c1 starts.
    let (p, _) = add(&p, "c0", "c1", 200, TransitionKind::Dissolve).unwrap();
    validate_project(&p).unwrap();
    assert_eq!(p.transitions.len(), 2);
}

#[test]
fn cross_kind_transition_is_refused() {
    let p = project();
    let err = add(&p, "c1", "c2", 300, TransitionKind::EqualPower).unwrap_err();
    assert!(err.message.contains("dissolve"), "{}", err.message);

    let err = add(&p, "y1", "y2", 300, TransitionKind::Dissolve).unwrap_err();
    assert!(err.message.contains("equal-power"), "{}", err.message);

    // The matching kind on the same audio pair is accepted.
    let (ok, _) = add(&p, "y1", "y2", 300, TransitionKind::EqualPower).unwrap();
    validate_project(&ok).unwrap();
    assert_eq!(starts(&ok, &["y1", "y2", "c2"]), vec![3_000, 3_800, 2_500]);

    // Two clips of different media on one track (a hand-edited graph --
    // `validate_project` rejects it, but the command must not rely on
    // that) are refused by the command itself.
    let mut mixed = project();
    mixed.clips.push(clip("z", "v1", "aa", 7_500, 0, 400));
    let err = add(&mixed, "c3", "z", 100, TransitionKind::Dissolve).unwrap_err();
    assert!(err.message.contains("media"), "{}", err.message);
}

#[test]
fn transition_through_a_group_is_refused_atomically() {
    // c3 (shifted by the transition) is grouped with x1 on v2 (not
    // shifted): the shift would desync the group.
    let mut p = project();
    for c in p.clips.iter_mut().filter(|c| c.id == "c3" || c.id == "x1") {
        c.group_id = Some("g1".into());
    }
    let before = p.clone();
    let err = add(&p, "c1", "c2", 700, TransitionKind::Dissolve).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("g1"), "{}", err.message);
    assert_eq!(
        p, before,
        "a refused transition leaves the project untouched"
    );

    // Removing a transition shifts the same clips back, so it is guarded
    // the same way.
    let mut grouped = with_dissolve();
    for c in grouped
        .clips
        .iter_mut()
        .filter(|c| c.id == "c4" || c.id == "y1")
    {
        c.group_id = Some("g2".into());
    }
    let id = grouped.transitions[0].id.clone();
    let err =
        remove_transition(&grouped, &RemoveTransitionPayload { transition_id: id }).unwrap_err();
    assert!(err.message.contains("g2"), "{}", err.message);

    // A group that lies wholly on the shifted part of the track moves
    // together and is fine.
    let mut same_track = project();
    for c in same_track
        .clips
        .iter_mut()
        .filter(|c| c.id == "c3" || c.id == "c4")
    {
        c.group_id = Some("g3".into());
    }
    add(&same_track, "c1", "c2", 700, TransitionKind::Dissolve).unwrap();
}

#[test]
fn move_of_a_clip_beside_a_transition_is_unaffected_by_the_overlap_allowance() {
    let p = with_dissolve();
    // The c1->c2 window is [1800, 2500). Moving c3 (5300) so it starts
    // at 1900 lands it inside that window: it overlaps BOTH transitioned
    // clips, and the allowance belongs to their pair alone.
    let err = move_clips(
        &p,
        &MoveClipsPayload {
            clip_ids: vec!["c3".into()],
            delta_ms: 1_900 - 5_300,
            track_id: None,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("would overlap"), "{}", err.message);

    // Moving one transitioned clip on its own breaks the pair: refused.
    let err = move_clips(
        &p,
        &MoveClipsPayload {
            clip_ids: vec!["c2".into()],
            delta_ms: 1_000,
            track_id: None,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("transition"), "{}", err.message);

    // Moving the pair together keeps its geometry.
    let (moved, _) = move_clips(
        &p,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            delta_ms: 300,
            track_id: None,
        },
    )
    .unwrap();
    validate_project(&moved).unwrap();
    assert_eq!(starts(&moved, &["c1", "c2"]), vec![800, 2_100]);

    // A move whose shared delta clamps to 0 leaves the pair where it is:
    // the scan compares c2 against its UNMOVED partner c1, and their
    // overlap is exactly the transition's own window -- allowed.
    let (unmoved, _) = move_clips(
        &p,
        &MoveClipsPayload {
            clip_ids: vec!["c2".into(), "c0".into()],
            delta_ms: -400,
            track_id: None,
        },
    )
    .unwrap();
    assert_eq!(starts(&unmoved, &["c0", "c2"]), vec![0, 1_800]);
}

#[test]
fn close_gap_delete_beside_a_transition_ripples_correctly() {
    let p = with_dissolve();

    // A clip BEFORE the pair: both transitioned clips shift by c0's 500ms
    // together, the transition survives intact.
    let (before, _) = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c0".into()],
            close_gap: true,
        },
    )
    .unwrap();
    validate_project(&before).unwrap();
    assert_eq!(before.transitions.len(), 1);
    assert_eq!(
        starts(&before, &["c1", "c2", "c3", "c4", "x1"]),
        vec![0, 1_300, 4_800, 6_800, 2_500]
    );

    // A clip AFTER the pair: only c4 closes up by c3's 1500ms.
    let (after, label) = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c3".into()],
            close_gap: true,
        },
    )
    .unwrap();
    validate_project(&after).unwrap();
    assert_eq!(label, "Delete clips and close the gap");
    assert_eq!(after.transitions.len(), 1);
    assert_eq!(starts(&after, &["c1", "c2", "c4"]), vec![500, 1_800, 5_800]);
}

// ---- deleting a transitioned clip (controller ruling, Task 7 -> 30) --------

#[test]
fn deleting_a_transitioned_clip_removes_the_transition_and_restores_spacing() {
    let p = with_dissolve();

    // Leave the gap: the transition goes, and the overlap it had taken out
    // of v1 comes back (c3/c4 return to their pre-transition starts).
    let (leave, label) = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c2".into()],
            close_gap: false,
        },
    )
    .unwrap();
    validate_project(&leave).unwrap();
    assert!(leave.transitions.is_empty());
    assert_eq!(label, "Delete clips (transition removed)");
    assert_eq!(
        starts(&leave, &["c1", "c3", "c4", "x1"]),
        vec![500, 6_000, 8_000, 2_500]
    );

    // Close the gap: spacing restored FIRST, then c2's whole 2000ms rippled
    // out -- c3 lands exactly 4000ms, the same place deleting c2 from the
    // never-transitioned project would put it.
    let (close, _) = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c2".into()],
            close_gap: true,
        },
    )
    .unwrap();
    validate_project(&close).unwrap();
    assert_eq!(starts(&close, &["c1", "c3", "c4"]), vec![500, 4_000, 6_000]);

    // The FROM side, and both clips at once.
    let (from_side, _) = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: true,
        },
    )
    .unwrap();
    validate_project(&from_side).unwrap();
    assert_eq!(starts(&from_side, &["c2", "c3"]), vec![500, 4_000]);
    let (both, _) = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            close_gap: true,
        },
    )
    .unwrap();
    validate_project(&both).unwrap();
    assert_eq!(starts(&both, &["c0", "c3", "c4"]), vec![0, 2_000, 4_000]);
}

#[test]
fn deleting_a_transitioned_clip_refuses_when_restoring_would_break_a_group() {
    let mut p = with_dissolve();
    for c in p.clips.iter_mut().filter(|c| c.id == "c4" || c.id == "y2") {
        c.group_id = Some("g1".into());
    }
    let err = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["c2".into()],
            close_gap: false,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("g1"), "{}", err.message);
}

#[test]
fn a_ripple_that_would_split_a_transition_is_still_refused() {
    // Reachable only from a hand-edited graph (d overlaps c1 with no
    // transition -- `validate_project` rejects this project): after the
    // transitioned-clip detach, no VALID graph can produce a split. The
    // refusal stays as the backstop the controller ruling keeps.
    let mut p = minimal_project();
    p.assets.push(asset("av", AssetKind::Video, 60_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.clips.push(clip("c1", "v1", "av", 0, 0, 1_000));
    p.clips.push(clip("c2", "v1", "av", 900, 0, 1_000));
    p.clips.push(clip("d", "v1", "av", 500, 0, 300));
    p.transitions.push(Transition {
        id: "tr1".into(),
        from: "c1".into(),
        to: "c2".into(),
        duration_ms: 100,
        kind: TransitionKind::Dissolve,
        extra: Map::new(),
    });
    let err = delete_clips(
        &p,
        &DeleteClipsPayload {
            clip_ids: vec!["d".into()],
            close_gap: true,
        },
    )
    .unwrap_err();
    assert!(
        err.message.contains("split transition tr1"),
        "{}",
        err.message
    );
}

// ---- trim / split keep the geometry -----------------------------------------

#[test]
fn trimming_the_far_edge_of_a_transitioned_clip_is_allowed_but_the_joined_edge_is_not() {
    let p = with_dissolve();
    // c1's HEAD (its far edge from the transition): c1 keeps ending at
    // 2500, so its overlap with c2 is still exactly 700ms -- allowed.
    let (head, _) = trim_clip(
        &p,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 600,
            in_ms: 200,
            out_ms: 2_100,
        },
    )
    .unwrap();
    validate_project(&head).unwrap();

    // c1's TAIL is the joined edge: refused, naming the transition.
    let err = trim_clip(
        &p,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 500,
            in_ms: 100,
            out_ms: 1_900,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("transition"), "{}", err.message);

    // Shortening c2's tail below twice the transition breaks the duration
    // bound even though the joined edge did not move.
    let err = trim_clip(
        &p,
        &TrimClipPayload {
            clip_id: "c2".into(),
            start_ms: 1_800,
            in_ms: 3_000,
            out_ms: 4_500,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("transition"), "{}", err.message);
}

#[test]
fn splitting_a_transitioned_clip_keeps_the_pair_valid_or_refuses() {
    let p = with_dissolve();
    // Splitting c1 early leaves its right half (still c1's tail) carrying
    // the transition out -- `cue_follow::repoint_transitions`.
    let (split, _) = split_clip(
        &p,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 700,
        },
    )
    .unwrap();
    validate_project(&split).unwrap();
    assert_ne!(split.transitions[0].from, "c1");

    // Splitting c1 at 2000 leaves a 500ms right half: too short to carry a
    // 700ms transition.
    let err = split_clip(
        &p,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 2_000,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("transition"), "{}", err.message);
}

#[test]
fn reordering_a_transitioned_pair_is_refused() {
    // Swapping c2 ahead of c1 would leave c1 ending long after c2 starts:
    // the pair's overlap no longer matches the transition.
    let err = reorder_clip(
        &with_dissolve(),
        &ReorderClipPayload {
            clip_id: "c2".into(),
            direction: ReorderDirection::Earlier,
        },
    )
    .unwrap_err();
    assert!(
        err.message.contains("Reorder would break transition"),
        "{}",
        err.message
    );
}

// ---- addTransition's own refusals -------------------------------------------

#[test]
fn add_transition_refuses_non_adjacent_foreign_and_out_of_bounds_requests() {
    let p = project();
    let adjacency = add(&p, "c2", "c3", 300, TransitionKind::Dissolve).unwrap_err();
    assert!(
        adjacency.message.contains("does not start where"),
        "{}",
        adjacency.message
    );

    let tracks = add(&p, "c1", "x1", 300, TransitionKind::Dissolve).unwrap_err();
    assert!(tracks.message.contains("same track"), "{}", tracks.message);

    let same = add(&p, "c1", "c1", 300, TransitionKind::Dissolve).unwrap_err();
    assert!(same.message.contains("differ"), "{}", same.message);

    let zero = add(&p, "c1", "c2", 0, TransitionKind::Dissolve).unwrap_err();
    assert!(zero.message.contains("positive"), "{}", zero.message);

    // Half of the SHORTER clip: c1 and c2 are both 2000ms of output (c2
    // only because of its 1.5x speed -- its source span is 3000ms, which
    // would allow 1500ms if speed were ignored).
    let long = add(&p, "c1", "c2", 1_001, TransitionKind::Dissolve).unwrap_err();
    assert!(long.message.contains("1000 ms"), "{}", long.message);
    let (edge, _) = add(&p, "c1", "c2", 1_000, TransitionKind::Dissolve).unwrap();
    validate_project(&edge).unwrap();

    // c0 (500ms) bounds c0->c1 at 250ms, not c1's 1000ms.
    let short = add(&p, "c0", "c1", 251, TransitionKind::Dissolve).unwrap_err();
    assert!(short.message.contains("250 ms"), "{}", short.message);

    let mut locked = p.clone();
    locked.tracks[0].locked = true;
    let err = add(&locked, "c1", "c2", 300, TransitionKind::Dissolve).unwrap_err();
    assert!(err.message.contains("locked"), "{}", err.message);

    let err = add(&p, "c1", "nope", 300, TransitionKind::Dissolve).unwrap_err();
    assert!(err.message.contains("nope"), "{}", err.message);
}

#[test]
fn add_transition_clears_the_fades_the_crossfade_replaces() {
    // The reference editor's own `makeCrossfade`: the outgoing clip's
    // fade-out and the incoming clip's fade-in would otherwise dip to
    // black/silence in the MIDDLE of the blend.
    let mut p = project();
    for c in p.clips.iter_mut() {
        if c.id == "c1" {
            c.fade_in_ms = 300;
            c.fade_out_ms = 400;
        }
        if c.id == "c2" {
            c.fade_in_ms = 200;
            c.fade_out_ms = 600;
        }
    }
    let (p, _) = add(&p, "c1", "c2", 700, TransitionKind::Dissolve).unwrap();
    let fades = |id: &str| {
        let c = p.clips.iter().find(|c| c.id == id).unwrap();
        (c.fade_in_ms, c.fade_out_ms)
    };
    assert_eq!(fades("c1"), (300, 0));
    assert_eq!(fades("c2"), (0, 600));
}

// ---- setTransitionDuration ---------------------------------------------------

#[test]
fn set_transition_duration_moves_only_its_track_by_the_difference() {
    let p = with_dissolve();
    let id = p.transitions[0].id.clone();
    let set = |d: u64| {
        set_transition_duration(
            &p,
            &SetTransitionDurationPayload {
                transition_id: id.clone(),
                duration_ms: d,
            },
        )
    };

    let (shorter, label) = set(400).unwrap();
    validate_project(&shorter).unwrap();
    assert_eq!(label, "Transition duration");
    assert_eq!(shorter.transitions[0].duration_ms, 400);
    assert_eq!(
        starts(&shorter, &ALL),
        vec![0, 500, 2_100, 5_600, 7_600, 2_500, 3_000, 4_100]
    );

    let (longer, _) = set(900).unwrap();
    validate_project(&longer).unwrap();
    assert_eq!(
        starts(&longer, &["c2", "c3", "x1"]),
        vec![1_600, 5_100, 2_500]
    );

    assert!(set(1_001).unwrap_err().message.contains("1000 ms"));
    assert!(set(0).unwrap_err().message.contains("positive"));
    assert!(set(700).unwrap_err().message.contains("unchanged"));

    let err = set_transition_duration(
        &p,
        &SetTransitionDurationPayload {
            transition_id: "nope".into(),
            duration_ms: 300,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("nope"), "{}", err.message);
}

#[test]
fn remove_transition_refuses_an_unknown_id_and_a_locked_track() {
    let p = with_dissolve();
    let err = remove_transition(
        &p,
        &RemoveTransitionPayload {
            transition_id: "nope".into(),
        },
    )
    .unwrap_err();
    assert!(err.message.contains("nope"), "{}", err.message);

    let mut locked = p.clone();
    locked.tracks[0].locked = true;
    let id = p.transitions[0].id.clone();
    let err =
        remove_transition(&locked, &RemoveTransitionPayload { transition_id: id }).unwrap_err();
    assert!(err.message.contains("locked"), "{}", err.message);
}
