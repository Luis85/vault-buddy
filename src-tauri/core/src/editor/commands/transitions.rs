//! `addTransition`/`setTransitionDuration`/`removeTransition` (Task 30;
//! F-19, DATA-MODEL.md § Fade and transition rules: "A dissolve/equal-power
//! crossfade has compatible endpoints, positive bounded duration and
//! available source/time geometry"), plus the three hooks `clips.rs` calls
//! so every OTHER timing command respects a transition (F12). Its own
//! module rather than an extension of `fades.rs` (the brief's
//! fallback): the transition commands and their hooks would take that file
//! past the brief's 600-line threshold.
//!
//! **A transition IS an overlap.** Applying one moves `to` and every clip
//! after it on THAT track earlier by `durationMs`, so `from`'s last
//! `durationMs` and `to`'s first `durationMs` play together; removing it
//! moves them back (F-19: "Only the target track is shortened by the
//! overlap; removal restores spacing"). `validate_media::
//! transition_geometry_error` is the one statement of the resulting shape
//! (`from` ends exactly `durationMs` after `to` starts, the duration at
//! most half the shorter clip); `ensure_intact` below turns that same text
//! into an `invalidRequest` for a trim/move/split/ripple that would break
//! it, so a timing command refuses up front instead of reaching
//! `validate_project`'s `invalidProject` backstop.
//!
//! **Only its own track moves.** Every shift here is `shift_track`, which
//! touches exactly one track id; `check_groups` refuses first when a
//! shifted clip is grouped with a clip on another track (the shift would
//! desync the group -- `deleteClips`' own ripple rule, `clips.rs`). A
//! locked track refuses through the shared `ensure_unlocked`; clips are
//! only ever locked by their track.

use std::collections::HashSet;

use super::clips::{clip_end, ensure_unlocked, find_asset, find_clip, invalid_request, overlaps};
use super::fades::fade_limit;
use super::payloads::{
    AddTransitionPayload, RemoveTransitionPayload, SetTransitionDurationPayload,
};
use crate::editor::error::EditorError;
use crate::editor::ids::new_entity_id;
use crate::editor::model::{Clip, Project};
use crate::editor::model_cues::{Transition, TransitionKind};
use crate::editor::validate_media::{transition_geometry_error, transition_kind_for};
use crate::editor::Map;

fn kind_name(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Dissolve => "dissolve",
        TransitionKind::EqualPower => "equal-power",
    }
}

fn find_transition<'a>(project: &'a Project, id: &str) -> Result<&'a Transition, EditorError> {
    project
        .transitions
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| invalid_request(format!("transition {id} does not resolve")))
}

/// The largest duration a transition joining `from` and `to` may have:
/// half the SHORTER clip's output duration (`fades::fade_limit`, the one
/// place "half, floored" lives).
fn duration_bound(from: &Clip, to: &Clip) -> u64 {
    fade_limit(clip_end(from) - from.start_ms).min(fade_limit(clip_end(to) - to.start_ms))
}

fn check_duration(duration_ms: u64, from: &Clip, to: &Clip) -> Result<(), EditorError> {
    let bound = duration_bound(from, to);
    if duration_ms == 0 || duration_ms > bound {
        return Err(invalid_request(format!(
            "durationMs {duration_ms} must be positive and at most {bound} ms (half the shorter clip)"
        )));
    }
    Ok(())
}

/// Refuses, before anything moves, when shifting the clips on `track_id`
/// that start at or after `edge_ms` would move a clip whose group also
/// holds a clip on ANOTHER track. `exempt` clips (ones a delete is about
/// to remove) neither count as moved nor as a group partner left behind.
fn check_groups(
    project: &Project,
    track_id: &str,
    edge_ms: u64,
    exempt: &HashSet<&str>,
) -> Result<(), EditorError> {
    for moved in project.clips.iter().filter(|c| {
        c.track_id == track_id && c.start_ms >= edge_ms && !exempt.contains(c.id.as_str())
    }) {
        let Some(group_id) = moved.group_id.as_deref() else {
            continue;
        };
        let crosses_track = project.clips.iter().any(|other| {
            other.track_id != track_id
                && other.group_id.as_deref() == Some(group_id)
                && !exempt.contains(other.id.as_str())
        });
        if crosses_track {
            return Err(invalid_request(format!(
                "The transition would move clip {} away from its group {group_id} on another track",
                moved.id
            )));
        }
    }
    Ok(())
}

/// Moves every clip on `track_id` starting at or after `edge_ms` by
/// `delta_ms` -- that track only, never another.
fn shift_track(
    project: &mut Project,
    track_id: &str,
    edge_ms: u64,
    delta_ms: i64,
) -> Result<(), EditorError> {
    for c in project
        .clips
        .iter_mut()
        .filter(|c| c.track_id == track_id && c.start_ms >= edge_ms)
    {
        c.start_ms = c.start_ms.checked_add_signed(delta_ms).ok_or_else(|| {
            invalid_request(format!("clip {} cannot move by {delta_ms} ms", c.id))
        })?;
    }
    Ok(())
}

// ---- addTransition --------------------------------------------------------

/// `addTransition{fromClipId, toClipId, durationMs, transitionKind}`. The
/// side checks run BEFORE adjacency on purpose: once `from` already has a
/// transition out, nothing can start where it ends any more (its partner
/// overlaps that instant), so adjacency would otherwise answer first with
/// a message about the wrong rule. The fades the crossfade replaces --
/// `from`'s fade-out and `to`'s fade-in -- are cleared, as the reference
/// editor's `makeCrossfade` does: kept, they would dip to black/silence in
/// the middle of the blend. Undo restores them with everything else.
pub(super) fn add_transition(
    project: &Project,
    payload: &AddTransitionPayload,
) -> Result<(Project, String), EditorError> {
    if payload.from_clip_id == payload.to_clip_id {
        return Err(invalid_request("fromClipId and toClipId must differ"));
    }
    let from = find_clip(project, &payload.from_clip_id)?;
    let to = find_clip(project, &payload.to_clip_id)?;
    if from.track_id != to.track_id {
        return Err(invalid_request(format!(
            "A transition joins two clips on the same track ({} is on {}, {} on {})",
            from.id, from.track_id, to.id, to.track_id
        )));
    }
    ensure_unlocked(project, &from.track_id)?;

    if project.transitions.iter().any(|t| t.from == from.id) {
        return Err(invalid_request(format!(
            "Clip {} already has a transition on its out side",
            from.id
        )));
    }
    if project.transitions.iter().any(|t| t.to == to.id) {
        return Err(invalid_request(format!(
            "Clip {} already has a transition on its in side",
            to.id
        )));
    }
    if clip_end(from) != to.start_ms {
        return Err(invalid_request(format!(
            "Clip {} does not start where {} ends",
            to.id, from.id
        )));
    }

    let from_kind = find_asset(project, &from.asset_id)?.kind;
    if find_asset(project, &to.asset_id)?.kind != from_kind {
        return Err(invalid_request(format!(
            "Clips {} and {} carry different media",
            from.id, to.id
        )));
    }
    let expected = transition_kind_for(from_kind);
    if payload.kind != expected {
        return Err(invalid_request(format!(
            "These clips take a {} transition, not {}",
            kind_name(expected),
            kind_name(payload.kind)
        )));
    }
    check_duration(payload.duration_ms, from, to)?;
    check_groups(project, &to.track_id, to.start_ms, &HashSet::new())?;

    let (track_id, edge, from_id, to_id) = (
        to.track_id.clone(),
        to.start_ms,
        from.id.clone(),
        to.id.clone(),
    );
    let mut candidate = project.clone();
    shift_track(
        &mut candidate,
        &track_id,
        edge,
        -(payload.duration_ms as i64),
    )?;
    for c in candidate.clips.iter_mut() {
        if c.id == from_id {
            c.fade_out_ms = 0;
        } else if c.id == to_id {
            c.fade_in_ms = 0;
        }
    }
    candidate.transitions.push(Transition {
        id: new_entity_id("transition"),
        from: from_id,
        to: to_id,
        duration_ms: payload.duration_ms,
        kind: payload.kind,
        extra: Map::new(),
    });
    Ok((candidate, "Add transition".to_string()))
}

// ---- setTransitionDuration / removeTransition -----------------------------

/// `setTransitionDuration{transitionId, durationMs}`: re-bounds the
/// duration and moves `to` and everything after it on that track by the
/// DIFFERENCE, so the overlap is always exactly the new duration.
pub(super) fn set_transition_duration(
    project: &Project,
    payload: &SetTransitionDurationPayload,
) -> Result<(Project, String), EditorError> {
    let t = find_transition(project, &payload.transition_id)?;
    let from = find_clip(project, &t.from)?;
    let to = find_clip(project, &t.to)?;
    ensure_unlocked(project, &to.track_id)?;
    check_duration(payload.duration_ms, from, to)?;
    if payload.duration_ms == t.duration_ms {
        return Err(invalid_request("durationMs is unchanged"));
    }
    check_groups(project, &to.track_id, to.start_ms, &HashSet::new())?;

    let delta = t.duration_ms as i64 - payload.duration_ms as i64;
    let mut candidate = project.clone();
    shift_track(&mut candidate, &to.track_id, to.start_ms, delta)?;
    for tr in candidate.transitions.iter_mut().filter(|tr| tr.id == t.id) {
        tr.duration_ms = payload.duration_ms;
    }
    Ok((candidate, "Transition duration".to_string()))
}

/// `removeTransition{transitionId}`: drops the transition and moves `to`
/// and everything after it back by its duration -- the exact inverse of
/// `add_transition`'s shift, so the track's spacing is what it was.
pub(super) fn remove_transition(
    project: &Project,
    payload: &RemoveTransitionPayload,
) -> Result<(Project, String), EditorError> {
    let t = find_transition(project, &payload.transition_id)?;
    let to = find_clip(project, &t.to)?;
    ensure_unlocked(project, &to.track_id)?;
    let mut candidate = project.clone();
    detach(&mut candidate, &t.id.clone(), &HashSet::new())?;
    Ok((candidate, "Remove transition".to_string()))
}

/// Removes transition `id` from `project` and restores its spacing, after
/// the group check. Shared by `remove_transition` and `detach_for_delete`
/// so "remove" means one thing whether the user asked for it or a delete
/// implied it.
fn detach(project: &mut Project, id: &str, exempt: &HashSet<&str>) -> Result<(), EditorError> {
    let t = find_transition(project, id)?.clone();
    let to = find_clip(project, &t.to)?;
    let (track_id, edge) = (to.track_id.clone(), to.start_ms);
    check_groups(project, &track_id, edge, exempt)?;
    shift_track(project, &track_id, edge, t.duration_ms as i64)?;
    project.transitions.retain(|tr| tr.id != t.id);
    Ok(())
}

// ---- hooks for clips.rs (F12) ---------------------------------------------

/// `deleteClips`' first step (controller ruling after Task 7): every
/// transition touching a deleted clip is removed AND its spacing restored,
/// before the delete itself runs -- in the SAME candidate, so one undo
/// step. Returns the detached project and whether anything was detached
/// (for the undo label). Restoring first is also what keeps the close-gap
/// ripple's arithmetic right: it subtracts each deleted clip's whole
/// duration, which only equals the time it occupied once it no longer
/// overlaps a partner.
pub(super) fn detach_for_delete(
    project: &Project,
    ids: &HashSet<&str>,
) -> Result<(Project, bool), EditorError> {
    let touched: Vec<String> = project
        .transitions
        .iter()
        .filter(|t| ids.contains(t.from.as_str()) || ids.contains(t.to.as_str()))
        .map(|t| t.id.clone())
        .collect();
    let mut detached = project.clone();
    for id in &touched {
        detach(&mut detached, id, ids)?;
    }
    Ok((detached, !touched.is_empty()))
}

/// The overlap refusal `trimClip`/`moveClips` share: `None` when the
/// proposed span `[start_ms, end_ms)` of `clip_id` does not overlap
/// `other`, or overlaps it by EXACTLY the window of a transition joining
/// the two -- the one overlap a transition creates. Any other clip, or the
/// same pair at any other overlap, is refused. Scoped to the transitioned
/// PAIR on purpose: a third clip dropped into their window overlaps both
/// and is refused like any overlap.
pub(super) fn overlap_refusal(
    project: &Project,
    clip_id: &str,
    start_ms: u64,
    end_ms: u64,
    other: &Clip,
) -> Option<EditorError> {
    let other_end = clip_end(other);
    if !overlaps(start_ms, end_ms, other.start_ms, other_end) {
        return None;
    }
    let pair = project.transitions.iter().find(|t| {
        (t.from == clip_id && t.to == other.id) || (t.from == other.id && t.to == clip_id)
    });
    match pair {
        Some(t) => {
            let (from_end, to_start) = if t.from == clip_id {
                (end_ms, other.start_ms)
            } else {
                (other_end, start_ms)
            };
            (from_end.checked_sub(to_start) != Some(t.duration_ms)).then(|| {
                invalid_request(format!(
                    "Clip {clip_id} would break transition {} with {}; remove the transition first",
                    t.id, other.id
                ))
            })
        }
        None => Some(invalid_request(format!(
            "clip {clip_id} would overlap {} on track {}",
            other.id, other.track_id
        ))),
    }
}

/// Every transition in `candidate` must still have its geometry
/// (`validate_media::transition_geometry_error`); `action` names the
/// command in the refusal ("Trim", "Move", "Split", "Reorder"). A ripple
/// is phrased as the SPLIT the controller ruling keeps refusing.
pub(super) fn ensure_intact(candidate: &Project, action: &str) -> Result<(), EditorError> {
    for t in &candidate.transitions {
        let (Some(from), Some(to)) = (
            candidate.clips.iter().find(|c| c.id == t.from),
            candidate.clips.iter().find(|c| c.id == t.to),
        ) else {
            continue;
        };
        if let Some(reason) = transition_geometry_error(t, from, to) {
            let verb = if action == "Ripple" { "split" } else { "break" };
            return Err(invalid_request(format!(
                "{action} would {verb} transition {} ({reason}); remove the transition first",
                t.id
            )));
        }
    }
    Ok(())
}

// Tests live in the sibling `transitions_tests.rs`, the `clips.rs`/
// `clips_tests.rs` precedent.
#[cfg(test)]
#[path = "transitions_tests.rs"]
mod tests;
