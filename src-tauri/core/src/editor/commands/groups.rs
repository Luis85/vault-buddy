//! Group/ungroup, duplicate, cut and paste-fragment (F-11, F-12, Task 8):
//! `groupClips`, `ungroupClips`, `duplicateClips`, `pasteFragment`,
//! `cutClips`. `moveClips`'s own group-EXPANSION behaviour (a selection
//! widens to every clip sharing a group before the shared delta is
//! computed) lives in the sibling `clips.rs` instead (F13: Task 7 shipped
//! `moveClips` before any group existed to expand into, so that behaviour
//! could only land here, in the task that introduces groups at all).
//!
//! Every command below reuses `clips.rs`'s own helpers (`ensure_unlocked`,
//! `find_clip`/`find_track`/`find_asset`, `overlaps`/`clip_end`/
//! `checked_output_end`) rather than re-implementing the same overlap/
//! locked-track/checked-arithmetic rules a THIRD time in this file (the
//! task brief's own instruction).

use std::collections::{HashMap, HashSet};

use super::clips::{
    checked_output_end, clip_end, ensure_unlocked, find_asset, find_clip, find_track,
    invalid_request, overlaps,
};
use super::payloads::{
    CutClipsPayload, DeleteClipsPayload, DuplicateClipsPayload, GroupClipsPayload,
    PasteFragmentPayload, UngroupClipsPayload,
};
use crate::editor::error::{EditorError, EditorErrorCode};
use crate::editor::ids::new_entity_id;
use crate::editor::limits;
use crate::editor::model::{AssetKind, Clip, TrackKind};
use crate::editor::model_cues::{CaptionCue, CaptionPosition, CaptionSettings};
use crate::editor::time::ClipSpan;
use crate::editor::validate::speed_or_default;
use crate::editor::{Map, Num, Project};

/// `base + delta`, checked against both u64 overflow (an adversarial
/// `offsetMs`/`atMs` from IPC) and `limits::MAX_DURATION_MS` (the same
/// ceiling `moveClips`'s own `apply_delta` bound enforces on an installed
/// clip) -- mirrors `clips.rs`'s own checked-arithmetic discipline for a
/// PROPOSED, not-yet-validated position.
fn checked_add_bounded(base: u64, delta: u64, what: &str) -> Result<u64, EditorError> {
    let value = base
        .checked_add(delta)
        .ok_or_else(|| invalid_request(format!("{what} overflows the new position")))?;
    if value > limits::MAX_DURATION_MS {
        return Err(invalid_request(format!(
            "{what} would move it to {value} ms, exceeding the {} ms maximum",
            limits::MAX_DURATION_MS
        )));
    }
    Ok(value)
}

/// Every NEW clip in `new_clips` must not overlap any EXISTING clip on the
/// same track (`project`, untouched) or any OTHER new clip in the same
/// batch -- checked read-only, before either caller (`duplicateClips`/
/// `pasteFragment`) ever builds a candidate. Uses `checked_output_end`
/// (not the plain `clip_end`) for each NEW clip's own span, since its
/// `in_ms`/`out_ms` may come straight from an unvalidated `ClipboardFragment`
/// (`pasteFragment`'s case) rather than an already-`validate_project`-
/// checked clip.
fn check_no_overlap(project: &Project, new_clips: &[Clip]) -> Result<(), EditorError> {
    for (i, added) in new_clips.iter().enumerate() {
        let added_span = ClipSpan {
            start_ms: added.start_ms,
            in_ms: added.in_ms,
            out_ms: added.out_ms,
            speed: speed_or_default(added.speed.as_ref()),
        };
        let added_end = checked_output_end(&added_span)?;
        for existing in project
            .clips
            .iter()
            .filter(|c| c.track_id == added.track_id)
        {
            let existing_end = clip_end(existing);
            if overlaps(added.start_ms, added_end, existing.start_ms, existing_end) {
                return Err(invalid_request(format!(
                    "clip {} would overlap {} on track {}",
                    added.id, existing.id, added.track_id
                )));
            }
        }
        for other in new_clips.iter().skip(i + 1) {
            if other.track_id != added.track_id {
                continue;
            }
            let other_span = ClipSpan {
                start_ms: other.start_ms,
                in_ms: other.in_ms,
                out_ms: other.out_ms,
                speed: speed_or_default(other.speed.as_ref()),
            };
            let other_end = checked_output_end(&other_span)?;
            if overlaps(added.start_ms, added_end, other.start_ms, other_end) {
                return Err(invalid_request(format!(
                    "clip {} would overlap clip {} on track {}",
                    added.id, other.id, added.track_id
                )));
            }
        }
    }
    Ok(())
}

/// Merges freshly-copied caption cues into `project.captions`, creating a
/// (disabled, sensible-default) `CaptionSettings` container when the
/// project has none yet. A fragment/duplicate source can only carry
/// non-empty caption cues when its SOURCE project already had a
/// `CaptionSettings` (cues live INSIDE it, never bare), but a paste can
/// land in a DIFFERENT project/session than it was copied from, so the
/// destination is not guaranteed to have one already. R20 ("nothing is
/// faked"): the cues themselves are never dropped -- only the enabled/
/// display toggle defaults to off, since nothing here should silently
/// turn captions ON for a project that never had them.
fn extend_captions(project: &mut Project, new_cues: Vec<CaptionCue>) {
    if new_cues.is_empty() {
        return;
    }
    match project.captions.as_mut() {
        Some(settings) => settings.cues.extend(new_cues),
        None => {
            project.captions = Some(CaptionSettings {
                enabled: false,
                burn_in: false,
                font_size: Num::from(16),
                position: CaptionPosition::Bottom,
                background: false,
                cues: new_cues,
                extra: Map::new(),
            });
        }
    }
}

// ---- groupClips / ungroupClips -------------------------------------------

/// `groupClips{clipIds}`: at least 2 DISTINCT clips (dedup runs BEFORE
/// this count check, so `["c1","c1"]` -- two entries naming the same
/// clip -- is refused just like `["c1"]` would be, never treated as a
/// pair), sets a FRESH `group_id` on every one of them -- overwriting any
/// group a clip was previously in (re-grouping a clip is not an error, it
/// just moves it into the new group).
pub(super) fn group_clips(
    project: &Project,
    payload: &GroupClipsPayload,
) -> Result<(Project, String), EditorError> {
    let ids: HashSet<&str> = payload.clip_ids.iter().map(String::as_str).collect();
    if ids.len() < 2 {
        return Err(invalid_request("groupClips requires at least 2 clips"));
    }
    let mut clips: Vec<&Clip> = Vec::with_capacity(ids.len());
    for id in &ids {
        clips.push(find_clip(project, id)?);
    }
    for clip in &clips {
        ensure_unlocked(project, &clip.track_id)?;
    }

    let group_id = new_entity_id("group");
    let mut candidate = project.clone();
    for c in candidate.clips.iter_mut() {
        if ids.contains(c.id.as_str()) {
            c.group_id = Some(group_id.clone());
        }
    }
    Ok((candidate, "Group clips".to_string()))
}

/// `ungroupClips{groupId}`: clears `group_id` from every clip currently
/// carrying it. An unknown group (nothing carries it) is refused rather
/// than a silent no-op -- the same posture `reorderClip`'s missing
/// neighbour takes (`clips.rs`).
pub(super) fn ungroup_clips(
    project: &Project,
    payload: &UngroupClipsPayload,
) -> Result<(Project, String), EditorError> {
    let members: Vec<&Clip> = project
        .clips
        .iter()
        .filter(|c| c.group_id.as_deref() == Some(payload.group_id.as_str()))
        .collect();
    if members.is_empty() {
        return Err(invalid_request(format!(
            "group {} does not resolve",
            payload.group_id
        )));
    }
    for clip in &members {
        ensure_unlocked(project, &clip.track_id)?;
    }

    let mut candidate = project.clone();
    for c in candidate.clips.iter_mut() {
        if c.group_id.as_deref() == Some(payload.group_id.as_str()) {
            c.group_id = None;
        }
    }
    Ok((candidate, "Ungroup clips".to_string()))
}

// ---- duplicateClips -------------------------------------------------------

/// `duplicateClips{clipIds, offsetMs}`: every named clip, plus every
/// effect/caption/marker attached to it, gets a FRESH id (mutation check:
/// reusing a source clip's id for one duplicate must turn
/// `duplicate_mints_fresh_ids_everywhere` red). A group shared by more
/// than one of the DUPLICATED clips is re-minted ONCE into a fresh common
/// group id, so the duplicates stay grouped with each other -- never with
/// the originals. `offsetMs` is applied to every duplicated clip's own
/// `start_ms` directly (never re-derived per clip), which is what
/// preserves their relative spacing automatically -- the same
/// "one shared delta" posture `moveClips`'s group expansion uses. Each
/// duplicate lands on the SAME track as its source, which must be
/// unlocked and free of overlap at the new position -- checked in a
/// read-only pass over `project` before `candidate` is ever built.
pub(super) fn duplicate_clips(
    project: &Project,
    payload: &DuplicateClipsPayload,
) -> Result<(Project, String), EditorError> {
    if payload.clip_ids.is_empty() {
        return Err(invalid_request("clipIds must not be empty"));
    }
    let mut originals: Vec<&Clip> = Vec::with_capacity(payload.clip_ids.len());
    for id in &payload.clip_ids {
        originals.push(find_clip(project, id)?);
    }
    for clip in &originals {
        ensure_unlocked(project, &clip.track_id)?;
    }
    let ids: HashSet<&str> = payload.clip_ids.iter().map(String::as_str).collect();

    let mut clip_id_map: HashMap<String, String> = HashMap::new();
    let mut group_id_map: HashMap<String, String> = HashMap::new();
    for clip in &originals {
        clip_id_map.insert(clip.id.clone(), new_entity_id("clip"));
        if let Some(group_id) = &clip.group_id {
            group_id_map
                .entry(group_id.clone())
                .or_insert_with(|| new_entity_id("group"));
        }
    }

    let mut new_clips: Vec<Clip> = Vec::with_capacity(originals.len());
    for clip in &originals {
        let new_start = checked_add_bounded(clip.start_ms, payload.offset_ms, "offsetMs")?;
        let mut duplicate = (*clip).clone();
        duplicate.id = clip_id_map[&clip.id].clone();
        duplicate.start_ms = new_start;
        duplicate.group_id = clip.group_id.as_ref().map(|g| group_id_map[g].clone());
        new_clips.push(duplicate);
    }

    check_no_overlap(project, &new_clips)?;

    let mut candidate = project.clone();
    let new_effects: Vec<_> = project
        .effects
        .iter()
        .filter(|e| ids.contains(e.clip_id.as_str()))
        .map(|e| {
            let mut copy = e.clone();
            copy.id = new_entity_id("effect");
            copy.clip_id = clip_id_map[&e.clip_id].clone();
            copy
        })
        .collect();
    let new_markers: Vec<_> = project
        .markers
        .iter()
        .filter(|m| ids.contains(m.clip_id.as_str()))
        .map(|m| {
            let mut copy = m.clone();
            copy.id = new_entity_id("marker");
            copy.clip_id = clip_id_map[&m.clip_id].clone();
            copy
        })
        .collect();
    let new_captions: Vec<CaptionCue> = project
        .captions
        .iter()
        .flat_map(|c| c.cues.iter())
        .filter(|c| ids.contains(c.clip_id.as_str()))
        .map(|c| {
            let mut copy = c.clone();
            copy.id = new_entity_id("caption");
            copy.clip_id = clip_id_map[&c.clip_id].clone();
            copy
        })
        .collect();

    candidate.clips.extend(new_clips);
    candidate.effects.extend(new_effects);
    candidate.markers.extend(new_markers);
    extend_captions(&mut candidate, new_captions);

    Ok((candidate, "Duplicate clips".to_string()))
}

// ---- pasteFragment / cutClips ---------------------------------------------

/// `pasteFragment{fragment, trackId, atMs}`: every clip/effect/caption/
/// marker the fragment carries gets a FRESH id (same fresh-id discipline
/// and mutation check as `duplicateClips`), re-pointed onto the fresh
/// clip ids; a group shared by more than one fragment clip is re-minted
/// once into a fresh common group id. EVERY pasted clip lands on the ONE
/// target track (`trackId`) -- a fragment is not redistributed across its
/// clips' original tracks -- which must be unlocked, kind-compatible with
/// each clip's asset, and free of overlap at the pasted position. Every
/// asset a fragment clip references must already exist in THIS project,
/// or the whole paste is refused as `sourceMissing` BEFORE anything else
/// is even computed -- a fragment copied in a different session (or after
/// its source asset was removed) must not silently invent a dangling
/// `asset_id`.
pub(super) fn paste_fragment(
    project: &Project,
    payload: &PasteFragmentPayload,
) -> Result<(Project, String), EditorError> {
    let fragment = &payload.fragment;
    if fragment.clips.is_empty() {
        return Err(invalid_request("fragment has no clips to paste"));
    }
    ensure_unlocked(project, &payload.track_id)?;
    let track = find_track(project, &payload.track_id)?;

    for clip in &fragment.clips {
        if find_asset(project, &clip.asset_id).is_err() {
            return Err(EditorError::new(
                EditorErrorCode::SourceMissing,
                format!(
                    "asset {} referenced by the fragment does not exist in this project",
                    clip.asset_id
                ),
            ));
        }
    }

    let mut clip_id_map: HashMap<String, String> = HashMap::new();
    let mut group_id_map: HashMap<String, String> = HashMap::new();
    for clip in &fragment.clips {
        clip_id_map.insert(clip.id.clone(), new_entity_id("clip"));
        if let Some(group_id) = &clip.group_id {
            group_id_map
                .entry(group_id.clone())
                .or_insert_with(|| new_entity_id("group"));
        }
    }

    let mut new_clips: Vec<Clip> = Vec::with_capacity(fragment.clips.len());
    for clip in &fragment.clips {
        let asset = find_asset(project, &clip.asset_id)?;
        let want_kind = match asset.kind {
            AssetKind::Audio => TrackKind::Audio,
            AssetKind::Video => TrackKind::Video,
        };
        if track.kind != want_kind {
            return Err(invalid_request(format!(
                "clip {} cannot paste onto track {}: a {:?} asset needs a {want_kind:?} track",
                clip.id, payload.track_id, asset.kind
            )));
        }
        let relative = clip
            .start_ms
            .checked_sub(fragment.origin_ms)
            .ok_or_else(|| {
                invalid_request("fragment clip starts before the fragment's own originMs")
            })?;
        let new_start = checked_add_bounded(payload.at_ms, relative, "atMs")?;

        let mut pasted = clip.clone();
        pasted.id = clip_id_map[&clip.id].clone();
        pasted.track_id = payload.track_id.clone();
        pasted.start_ms = new_start;
        pasted.group_id = clip.group_id.as_ref().map(|g| group_id_map[g].clone());
        new_clips.push(pasted);
    }

    check_no_overlap(project, &new_clips)?;

    let mut candidate = project.clone();
    let new_effects: Vec<_> = fragment
        .effects
        .iter()
        .filter_map(|e| {
            clip_id_map.get(&e.clip_id).map(|new_id| {
                let mut copy = e.clone();
                copy.id = new_entity_id("effect");
                copy.clip_id = new_id.clone();
                copy
            })
        })
        .collect();
    let new_markers: Vec<_> = fragment
        .markers
        .iter()
        .filter_map(|m| {
            clip_id_map.get(&m.clip_id).map(|new_id| {
                let mut copy = m.clone();
                copy.id = new_entity_id("marker");
                copy.clip_id = new_id.clone();
                copy
            })
        })
        .collect();
    let new_captions: Vec<CaptionCue> = fragment
        .captions
        .iter()
        .filter_map(|c| {
            clip_id_map.get(&c.clip_id).map(|new_id| {
                let mut copy = c.clone();
                copy.id = new_entity_id("caption");
                copy.clip_id = new_id.clone();
                copy
            })
        })
        .collect();

    candidate.clips.extend(new_clips);
    candidate.effects.extend(new_effects);
    candidate.markers.extend(new_markers);
    extend_captions(&mut candidate, new_captions);

    Ok((candidate, "Paste".to_string()))
}

/// `cutClips{clipIds, closeGap}`: a fragment-free delete -- the frontend is
/// the one that builds a `ClipboardFragment` (via `buildFragment`, before
/// dispatching this) and stashes it locally; Rust's own job is only the
/// delete half, sharing `deleteClips`'s exact semantics (same group/
/// transition ripple refusals, `clips.rs`) but labelled "Cut" for the undo
/// history rather than "Delete clips"/"Delete clips and close the gap".
pub(super) fn cut_clips(
    project: &Project,
    payload: &CutClipsPayload,
) -> Result<(Project, String), EditorError> {
    let (candidate, _label) = super::clips::delete_clips(
        project,
        &DeleteClipsPayload {
            clip_ids: payload.clip_ids.clone(),
            close_gap: payload.close_gap,
        },
    )?;
    Ok((candidate, "Cut".to_string()))
}

// Tests live in the sibling `groups_tests.rs` (not inline), the `clips.rs`/
// `clips_tests.rs` precedent.
#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
