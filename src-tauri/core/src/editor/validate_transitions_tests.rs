//! Task 30: transition geometry, kind and same-track overlap. Split out of
//! `validate_tests.rs` at its own `#[path]` boundary (Task 34 fix round 1,
//! LOC guard: `validate_tests.rs` alone had grown to 832 nonblank lines
//! against the 800 Rust cap once the new `effect_numeric_fields_are_
//! bounded_to_the_schema` table test landed). Nested a level deeper than a
//! sibling of `validate.rs` -- `mod transitions` inside `validate_tests.rs`
//! itself, not a second `#[path]` mod on `validate.rs` -- so `use super::*`
//! here reaches `validate_tests.rs`'s own `base_project()`/`Product`/
//! `Record` re-exports for free, the same way `validate_tests.rs`'s own
//! `use super::*` already reaches `validate.rs`'s.

use super::*;

// ---- Task 30: transition geometry, kind and same-track overlap ------------

/// `base_project`'s c1 (0..1000ms on t1) plus a c2 on the same track, and
/// an audio track/asset pair for the kind rule. c2 is 1400ms long -- NOT
/// c1's 1000ms -- so a duration bound computed from the wrong clip fails.
fn project_with_second_clip(c2_start: u64) -> Project {
    let mut project = base_project();
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c2", "asset_id": "a1", "track_id": "t1", "name": "Clip Two",
            "start_ms": c2_start, "in_ms": 200, "out_ms": 1600,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    project
}

fn dissolve(from: &str, to: &str, duration_ms: u64) -> crate::editor::model_cues::Transition {
    serde_json::from_value(serde_json::json!({
        "id": "tr1", "from": from, "to": to, "duration_ms": duration_ms, "kind": "dissolve"
    }))
    .unwrap()
}

#[test]
fn a_transition_is_an_overlap_of_exactly_its_duration_not_mere_adjacency() {
    // Regression (Task 30): before, validation demanded from's end ==
    // to's start, so the very overlap `addTransition` creates would have
    // been rejected as invalid on the next command.
    let mut overlapping = project_with_second_clip(700);
    overlapping.transitions.push(dissolve("c1", "c2", 300));
    validate_project(&overlapping).expect("a 300ms overlap carrying a 300ms transition is valid");

    let mut adjacent = project_with_second_clip(1000);
    adjacent.transitions.push(dissolve("c1", "c2", 300));
    let err = validate_project(&adjacent).unwrap_err();
    assert!(
        err.message.contains("tr1") && err.message.contains("300 ms after"),
        "message: {}",
        err.message
    );

    // A transition longer than half the SHORTER clip (c1, 1000ms) is out.
    let mut too_long = project_with_second_clip(400);
    too_long.transitions.push(dissolve("c1", "c2", 600));
    let err = validate_project(&too_long).unwrap_err();
    assert!(err.message.contains("1000 ms"), "message: {}", err.message);
}

#[test]
fn a_transition_kind_must_match_its_clips_media() {
    let mut project = project_with_second_clip(700);
    let mut t = dissolve("c1", "c2", 300);
    t.kind = crate::editor::model_cues::TransitionKind::EqualPower;
    project.transitions.push(t);
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("tr1") && err.message.contains("Dissolve"),
        "message: {}",
        err.message
    );
}

#[test]
fn same_track_clips_may_overlap_only_by_their_transition() {
    // The same 300ms overlap as the valid case above, but with no
    // transition to explain it.
    let project = project_with_second_clip(700);
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c2") && err.message.contains("without a transition"),
        "message: {}",
        err.message
    );

    // The allowance is the transitioned PAIR's alone: a third clip placed
    // inside the c1->c2 window is still an unexplained overlap.
    let mut project = project_with_second_clip(700);
    project.transitions.push(dissolve("c1", "c2", 300));
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c3", "asset_id": "a1", "track_id": "t1", "name": "Clip Three",
            "start_ms": 800, "in_ms": 0, "out_ms": 200,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("c3"), "message: {}", err.message);
}
