//! `addTrack`/`renameTrack`/`moveTrack`/`setTrackFlags`/`deleteTrack`
//! (Task 23; F-06, `FEATURE-CATALOG.md` F-04).
//!
//! **Compositing order is literal `Project.tracks` Vec order** -- index 0
//! is the FRONTMOST (topmost) layer, matching F-04's "Upper layers cover
//! lower layers; invisible tracks are excluded from output." `moveTrack`
//! reorders this Vec directly (`insert`/`remove`, no separate `order`
//! field to keep in sync), so every later render/preview pass that walks
//! `project.tracks` in order already gets the right stacking for free.
//!
//! **Solo semantics** (documented here per the brief, not yet consumed by
//! any mixer or render plan -- both are later tasks): when ANY track is
//! soloed (`Track.solo == true`), the intended audible set is exactly the
//! soloed tracks -- every other track's clips render silent for that pass,
//! regardless of their own `muted` flag. A soloed track's own `muted`
//! still wins over solo (mute is the stronger override) -- the same
//! precedence the reference editor's `clipGain` uses (`(solo && !tr.solo)`
//! sits behind `tr.muted`). Nothing in this file computes gain; it only
//! stores the flags a later mixer/render task reads.
//!
//! `ensure_unlocked`-style guards: `renameTrack`/`moveTrack`/`deleteTrack`
//! all refuse outright when their OWN target track is locked. `setTrackFlags`
//! is the one exception with nuance -- the `locked` field itself may
//! ALWAYS be toggled (lock or unlock), but while the track IS locked, any
//! OTHER field present in the same payload is refused, so a caller cannot
//! smuggle a visibility/mute/solo/volume change through in the same call
//! that also unlocks it.

use std::collections::HashSet;

use super::payloads::{
    AddTrackPayload, DeleteTrackPayload, MoveTrackPayload, RenameTrackPayload, SetTrackFlagsPayload,
};
use crate::editor::error::{EditorError, EditorErrorCode};
use crate::editor::ids::new_entity_id;
use crate::editor::limits;
use crate::editor::model::{Project, Track};
use crate::editor::{Map, Num};

fn invalid_request(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidRequest, message)
}

fn num(v: i64) -> Num {
    Num::from(v)
}

fn find_track<'a>(project: &'a Project, id: &str) -> Result<&'a Track, EditorError> {
    project
        .tracks
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| invalid_request(format!("track {id} does not resolve")))
}

fn locked_refusal(track: &Track) -> EditorError {
    invalid_request(format!("Track {} is locked", track.name))
}

// ---- addTrack ---------------------------------------------------------------

/// `addTrack{trackKind, name, index}`: mints a fresh `trk-…` id (`ids::
/// new_entity_id`) and inserts at `index`, clamped to the current track
/// count so an out-of-range index appends at the end instead of erroring
/// -- a caller that just wants "add after everything else" does not have
/// to know the exact count first. Refuses an empty (after trim) name --
/// the `meta::rename`/`clips::update_clip` precedent -- and the
/// `limits::MAX_TRACKS` count explicitly, mirroring `meta::add_assets`'s
/// own bound check: this command has no caller that runs
/// `validate_project` for it directly, so the 32-track ceiling would
/// otherwise never be enforced by a direct unit test against this
/// function alone.
pub(super) fn add_track(
    project: &Project,
    payload: &AddTrackPayload,
) -> Result<(Project, String), EditorError> {
    let trimmed = payload.name.trim();
    if trimmed.is_empty() {
        return Err(invalid_request("name must not be empty"));
    }
    if project.tracks.len() >= limits::MAX_TRACKS {
        return Err(invalid_request(format!(
            "adding a track would bring the project to {} entries, exceeding the {} maximum",
            project.tracks.len() + 1,
            limits::MAX_TRACKS
        )));
    }

    let mut candidate = project.clone();
    let index = (payload.index as usize).min(candidate.tracks.len());
    candidate.tracks.insert(
        index,
        Track {
            id: new_entity_id("trk"),
            kind: payload.kind,
            name: trimmed.to_string(),
            visible: true,
            locked: false,
            muted: false,
            solo: false,
            volume: num(1),
            extra: Map::new(),
        },
    );
    Ok((candidate, "Add track".to_string()))
}

// ---- renameTrack --------------------------------------------------------------

/// `renameTrack{trackId, name}`: refuses a locked track outright, then the
/// same trim-and-refuse-empty rule as `meta::rename`/`clips::update_clip`.
/// The 200-char maximum (`validate::TRACK_NAME_MAX`) is deliberately left
/// to `validate_project`, the same atomicity reasoning those two document.
pub(super) fn rename_track(
    project: &Project,
    payload: &RenameTrackPayload,
) -> Result<(Project, String), EditorError> {
    let track = find_track(project, &payload.track_id)?;
    if track.locked {
        return Err(locked_refusal(track));
    }
    let trimmed = payload.name.trim();
    if trimmed.is_empty() {
        return Err(invalid_request("name must not be empty"));
    }

    let mut candidate = project.clone();
    for t in candidate.tracks.iter_mut() {
        if t.id == payload.track_id {
            t.name = trimmed.to_string();
        }
    }
    Ok((candidate, "Rename track".to_string()))
}

// ---- moveTrack ------------------------------------------------------------

/// `moveTrack{trackId, toIndex}`: reorders `Project.tracks` directly --
/// index 0 is frontmost (this module's own doc). `toIndex` is clamped to
/// the POST-removal length, so an out-of-range index appends at the end
/// rather than erroring (the `addTrack` precedent above). Refuses a
/// locked track.
pub(super) fn move_track(
    project: &Project,
    payload: &MoveTrackPayload,
) -> Result<(Project, String), EditorError> {
    let track = find_track(project, &payload.track_id)?;
    if track.locked {
        return Err(locked_refusal(track));
    }

    let mut candidate = project.clone();
    let from = candidate
        .tracks
        .iter()
        .position(|t| t.id == payload.track_id)
        .expect("find_track above already resolved this id");
    let moved = candidate.tracks.remove(from);
    let to = (payload.to_index as usize).min(candidate.tracks.len());
    candidate.tracks.insert(to, moved);
    Ok((candidate, "Reorder tracks".to_string()))
}

// ---- setTrackFlags ----------------------------------------------------------

/// `setTrackFlags{trackId, visible?, locked?, muted?, solo?, volume?}` --
/// see this module's own doc for the locked/unlock carve-out. `volume`'s
/// `[0,2]` range (`workspace.schema.json`'s `track.volume`) is deferred to
/// `validate_project` (`validate::check_track`), the same "checked at
/// install, not here" posture `meta::rename` documents for its own
/// maximum.
pub(super) fn set_track_flags(
    project: &Project,
    payload: &SetTrackFlagsPayload,
) -> Result<(Project, String), EditorError> {
    let track = find_track(project, &payload.track_id)?;
    let other_field_set = payload.visible.is_some()
        || payload.muted.is_some()
        || payload.solo.is_some()
        || payload.volume.is_some();
    if track.locked && other_field_set {
        return Err(invalid_request(format!(
            "Track {} is locked; unlock it before changing any other flag",
            track.name
        )));
    }

    let mut candidate = project.clone();
    for t in candidate.tracks.iter_mut() {
        if t.id != payload.track_id {
            continue;
        }
        if let Some(v) = payload.visible {
            t.visible = v;
        }
        if let Some(v) = payload.locked {
            t.locked = v;
        }
        if let Some(v) = payload.muted {
            t.muted = v;
        }
        if let Some(v) = payload.solo {
            t.solo = v;
        }
        if let Some(v) = &payload.volume {
            t.volume = v.clone();
        }
    }

    let label = if payload.locked.is_some() && !other_field_set {
        if payload.locked == Some(true) {
            "Lock track"
        } else {
            "Unlock track"
        }
    } else {
        "Update track"
    };
    Ok((candidate, label.to_string()))
}

// ---- deleteTrack --------------------------------------------------------------

/// `deleteTrack{trackId}`: refuses a locked track; otherwise removes the
/// track AND every clip on it, plus those clips' effects/markers/captions/
/// transitions -- one candidate, one undo step (`clips::delete_clips`'s
/// own multi-collection cleanup pattern, keyed by track rather than an
/// explicit clip-id list). Label is exactly `"Delete track <name>"` (the
/// brief's own literal value).
pub(super) fn delete_track(
    project: &Project,
    payload: &DeleteTrackPayload,
) -> Result<(Project, String), EditorError> {
    let track = find_track(project, &payload.track_id)?;
    if track.locked {
        return Err(locked_refusal(track));
    }
    let label = format!("Delete track {}", track.name);

    let ids: HashSet<&str> = project
        .clips
        .iter()
        .filter(|c| c.track_id == payload.track_id)
        .map(|c| c.id.as_str())
        .collect();

    let mut candidate = project.clone();
    candidate.tracks.retain(|t| t.id != payload.track_id);
    candidate.clips.retain(|c| !ids.contains(c.id.as_str()));
    candidate
        .effects
        .retain(|e| !ids.contains(e.clip_id.as_str()));
    candidate
        .markers
        .retain(|m| !ids.contains(m.clip_id.as_str()));
    if let Some(captions) = candidate.captions.as_mut() {
        captions.cues.retain(|c| !ids.contains(c.clip_id.as_str()));
    }
    candidate
        .transitions
        .retain(|t| !ids.contains(t.from.as_str()) && !ids.contains(t.to.as_str()));

    Ok((candidate, label))
}

// Tests live in the sibling `tracks_tests.rs` (not inline), the `clips.rs`/
// `clips_tests.rs` precedent.
#[cfg(test)]
#[path = "tracks_tests.rs"]
mod tests;
