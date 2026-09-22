//! The seven core clip commands (F-07, F-08, F-09, F-10, `DATA-MODEL.md` §
//! Timing rules; A03, A11): `insertClip`, `updateClip`, `splitClip`,
//! `trimClip`, `deleteClips`, `moveClips`, `reorderClip`. Cue reassignment
//! for `splitClip` lives in the sibling `cue_follow.rs` (see its own doc);
//! everything else -- the shared `ensure_unlocked` guard, overlap checks,
//! and each command's own entity surgery -- stays here.
//!
//! Later tasks (8: groups, 29: fades, 30: transitions) extend THIS file
//! with more clip commands rather than adding a new family module, so the
//! shared helpers below (`ensure_unlocked`, `find_clip`/`find_track`/
//! `find_asset`, `clip_span`/`clip_end`/`clip_duration`, `overlaps`) are
//! deliberately free functions any later arm in this file can reuse.
//!
//! `pub(super)` on `invalid_request`/`ensure_unlocked`/`find_clip`/
//! `find_track`/`find_asset`/`overlaps`/`clip_end`/`checked_output_end`
//! (Task 8): the sibling `groups.rs` reuses these directly instead of a
//! second copy of the same overlap/locked-track/checked-arithmetic rules
//! (the task brief's own instruction). `clip_span`/`clip_duration`/
//! `apply_delta`/`num` stay private -- nothing outside this file needs
//! them yet.

use std::collections::HashSet;

use super::cue_follow;
use super::payloads::{
    DeleteClipsPayload, InsertClipPayload, MoveClipsPayload, ReorderClipPayload, ReorderDirection,
    SplitClipPayload, TrimClipPayload, UpdateClipPayload,
};
use crate::editor::error::{EditorError, EditorErrorCode};
use crate::editor::ids::new_entity_id;
use crate::editor::limits;
use crate::editor::model::{
    Asset, AssetKind, Clip, FadeCurve, MediaType, Project, Track, TrackKind,
};
use crate::editor::time::{self, ClipSpan};
use crate::editor::validate::speed_or_default;
use crate::editor::{Map, Num};

pub(super) fn invalid_request(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidRequest, message)
}

fn num(v: i64) -> Num {
    Num::from(v)
}

/// Half-open interval overlap: `[s1,e1)` and `[s2,e2)` overlap iff each
/// starts strictly before the other ends.
pub(super) fn overlaps(s1: u64, e1: u64, s2: u64, e2: u64) -> bool {
    s1 < e2 && s2 < e1
}

fn clip_span(clip: &Clip) -> ClipSpan {
    ClipSpan {
        start_ms: clip.start_ms,
        in_ms: clip.in_ms,
        out_ms: clip.out_ms,
        speed: speed_or_default(clip.speed.as_ref()),
    }
}

pub(super) fn clip_end(clip: &Clip) -> u64 {
    time::clip_output_end(&clip_span(clip))
}

fn clip_duration(clip: &Clip) -> u64 {
    time::clip_output_duration(
        clip.in_ms,
        clip.out_ms,
        speed_or_default(clip.speed.as_ref()),
    )
}

/// The `time::clip_output_end`-shaped computation, but via CHECKED
/// arithmetic: unlike `clip_end` above (which only ever reads an EXISTING,
/// already-`validate_project`-checked clip, so its `start_ms` is provably
/// bounded), this is for a PROPOSED span built directly from an
/// unvalidated IPC payload's own `startMs` (`insertClip`/`trimClip`/
/// `moveClips`) -- a caller-chosen `startMs` near `u64::MAX` must not
/// panic the process (debug) or silently wrap into a bogus overlap check
/// (release) before `validate_project` ever gets a chance to reject it.
/// Mirrors `validate::check_clip`'s own `checked_add` guard for an
/// INSTALLED clip's `start_ms + output_duration`.
pub(super) fn checked_output_end(span: &ClipSpan) -> Result<u64, EditorError> {
    let duration = time::clip_output_duration(span.in_ms, span.out_ms, span.speed);
    span.start_ms
        .checked_add(duration)
        .ok_or_else(|| invalid_request("startMs + output duration overflows"))
}

/// Applies a (possibly negative) delta to an existing clip's `start_ms` via
/// checked arithmetic: `checked_add_signed` returns `None` on EITHER an
/// overflow (an unreasonably large positive `deltaMs`) or a result that
/// would go negative. `moveClips`' shared-delta clamp already guarantees
/// no clip's new start goes negative for a legitimate (clamped) delta, so
/// in practice this only ever rejects the overflow case -- but it is the
/// checked primitive that makes that guarantee load-bearing instead of
/// assumed.
fn apply_delta(start_ms: u64, delta_ms: i64) -> Result<u64, EditorError> {
    start_ms
        .checked_add_signed(delta_ms)
        .ok_or_else(|| invalid_request("deltaMs overflows the clip's new position"))
}

pub(super) fn find_clip<'a>(project: &'a Project, id: &str) -> Result<&'a Clip, EditorError> {
    project
        .clips
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| invalid_request(format!("clip {id} does not resolve")))
}

pub(super) fn find_track<'a>(project: &'a Project, id: &str) -> Result<&'a Track, EditorError> {
    project
        .tracks
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| invalid_request(format!("track {id} does not resolve")))
}

pub(super) fn find_asset<'a>(project: &'a Project, id: &str) -> Result<&'a Asset, EditorError> {
    project
        .assets
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| invalid_request(format!("asset {id} does not resolve")))
}

/// The shared guard named in the brief: every clip-mutating command in
/// this file (and, per the brief, every later task's clip command) calls
/// this before touching the track's clips. A track id that does not
/// resolve is not this guard's job -- the caller's own `find_track`/
/// `find_clip` lookups catch that with their own message.
pub(super) fn ensure_unlocked(project: &Project, track_id: &str) -> Result<(), EditorError> {
    if let Some(track) = project.tracks.iter().find(|t| t.id == track_id) {
        if track.locked {
            return Err(invalid_request(format!("Track {} is locked", track.name)));
        }
    }
    Ok(())
}

// ---- insertClip -----------------------------------------------------------

/// `insertClip{assetId, trackId, startMs, inMs, outMs}`: mints a fresh
/// `clip-…` id, applies the reference defaults (fades 0, curve `linear`,
/// opacity 1, volume 1, muted false, `x 0, y 0, w 1, h 1`, speed unset —
/// i.e. the implicit 1.0×), and refuses to overlap another clip already on
/// the target track. **Decision, not in the brief**: the payload carries
/// no `name` (the Contract reference lists only `assetId, trackId,
/// startMs, inMs, outMs`), so the new clip's name defaults to the
/// resolved asset's own name -- `updateClip` renames it afterward if the
/// caller wants something else.
pub(super) fn insert_clip(
    project: &Project,
    payload: &InsertClipPayload,
) -> Result<(Project, String), EditorError> {
    ensure_unlocked(project, &payload.track_id)?;
    let asset = find_asset(project, &payload.asset_id)?;
    find_track(project, &payload.track_id)?;

    if payload.in_ms >= payload.out_ms {
        return Err(invalid_request("inMs must be before outMs"));
    }

    let new_span = ClipSpan {
        start_ms: payload.start_ms,
        in_ms: payload.in_ms,
        out_ms: payload.out_ms,
        speed: 1.0,
    };
    let new_end = checked_output_end(&new_span)?;
    for existing in project
        .clips
        .iter()
        .filter(|c| c.track_id == payload.track_id)
    {
        let existing_end = clip_end(existing);
        if overlaps(payload.start_ms, new_end, existing.start_ms, existing_end) {
            return Err(invalid_request(format!(
                "clip would overlap {} on track {}",
                existing.id, payload.track_id
            )));
        }
    }

    let mut candidate = project.clone();
    candidate.clips.push(Clip {
        id: new_entity_id("clip"),
        asset_id: payload.asset_id.clone(),
        track_id: payload.track_id.clone(),
        name: asset.name.clone(),
        start_ms: payload.start_ms,
        in_ms: payload.in_ms,
        out_ms: payload.out_ms,
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
    });
    Ok((candidate, "Insert clip".to_string()))
}

// ---- updateClip -------------------------------------------------------------

/// `updateClip{clipId, name}`: trims the name and refuses an empty result
/// -- the `meta::rename` precedent (`commands/meta.rs`). The 300-char
/// maximum (`limits::MAX_NAME_CHARS`) is deliberately left to
/// `validate_project`, the same atomicity reasoning `meta::rename`'s own
/// doc comment gives for the title's 160-char maximum.
pub(super) fn update_clip(
    project: &Project,
    payload: &UpdateClipPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    let trimmed = payload.name.trim();
    if trimmed.is_empty() {
        return Err(invalid_request("name must not be empty"));
    }

    let mut candidate = project.clone();
    for c in candidate.clips.iter_mut() {
        if c.id == payload.clip_id {
            c.name = trimmed.to_string();
        }
    }
    Ok((candidate, "Rename clip".to_string()))
}

// ---- splitClip --------------------------------------------------------------

/// `splitClip{clipId, atMs}` (`DATA-MODEL.md` § Timing rules; A03, A11).
/// `atMs` must be strictly inside the clip's OUTPUT interval -- exactly at
/// either edge is refused, since that would split off an empty half.
/// `s = source_at(clip, atMs)` is always `Some` once that guard passes
/// (the boundary check IS `clip_is_active`'s own condition), so the
/// `expect` below documents an invariant rather than guessing at one.
///
/// The ORIGINAL clip id stays on the LEFT half (`out_ms` becomes `s`,
/// `fade_out_ms` drops to 0 -- the original tail fade no longer applies to
/// an internal cut); a FRESH `clip-…` id carries the RIGHT half (`in_ms`
/// becomes `s`, `start_ms` becomes `atMs`, `fade_in_ms` drops to 0 for the
/// same reason, every other field cloned from the original). Cue
/// reassignment (effects, captions, markers, transitions) is
/// `cue_follow`'s job.
pub(super) fn split_clip(
    project: &Project,
    payload: &SplitClipPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    let span = clip_span(clip);
    let output_end = time::clip_output_end(&span);
    if payload.at_ms <= clip.start_ms || payload.at_ms >= output_end {
        return Err(invalid_request("Split point is at a clip boundary"));
    }
    let s = time::source_at(&span, payload.at_ms).expect(
        "atMs strictly inside [start_ms, output_end) is exactly clip_is_active's own condition",
    );

    let right_id = new_entity_id("clip");

    let mut right_half = clip.clone();
    right_half.id = right_id.clone();
    right_half.in_ms = s;
    right_half.start_ms = payload.at_ms;
    right_half.fade_in_ms = 0;

    let mut candidate = project.clone();
    for c in candidate.clips.iter_mut() {
        if c.id == clip.id {
            c.out_ms = s;
            c.fade_out_ms = 0;
        }
    }
    candidate.clips.push(right_half);

    cue_follow::split_effects(&mut candidate.effects, &clip.id, &right_id, s);
    cue_follow::reassign_markers(&mut candidate.markers, &clip.id, &right_id, s);
    if let Some(captions) = candidate.captions.as_mut() {
        cue_follow::split_captions(&mut captions.cues, &clip.id, &right_id, s);
    }
    cue_follow::repoint_transitions(&mut candidate.transitions, &clip.id, &right_id);

    Ok((candidate, "Split clip".to_string()))
}

// ---- trimClip -----------------------------------------------------------

/// `trimClip{clipId, startMs, inMs, outMs}`: sets the clip's placement and
/// source range directly (never a delta). `in < out`, the new `outMs`
/// must stay within the asset's own duration (`limits::MAX_DURATION_MS`
/// for an image asset, the same exemption `validate::check_clip` makes),
/// and the new placement must not overlap another clip on the same track.
/// Cue times are source-linked (`DATA-MODEL.md`: "Trim changes the
/// visible intersection, not the original cue timestamps") -- this
/// function touches no effect/caption/marker at all.
pub(super) fn trim_clip(
    project: &Project,
    payload: &TrimClipPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    if payload.in_ms >= payload.out_ms {
        return Err(invalid_request("inMs must be before outMs"));
    }

    let asset = find_asset(project, &clip.asset_id)?;
    let is_image = matches!(asset.media_type, Some(MediaType::Image));
    let out_bound = if is_image {
        limits::MAX_DURATION_MS
    } else {
        asset.duration_ms
    };
    if payload.out_ms > out_bound {
        return Err(invalid_request(format!(
            "outMs {} exceeds the {} ms {}",
            payload.out_ms,
            out_bound,
            if is_image {
                "maximum duration"
            } else {
                "asset duration"
            }
        )));
    }

    let new_span = ClipSpan {
        start_ms: payload.start_ms,
        in_ms: payload.in_ms,
        out_ms: payload.out_ms,
        speed: speed_or_default(clip.speed.as_ref()),
    };
    let new_end = checked_output_end(&new_span)?;
    for other in project
        .clips
        .iter()
        .filter(|c| c.track_id == clip.track_id && c.id != clip.id)
    {
        let other_end = clip_end(other);
        if overlaps(payload.start_ms, new_end, other.start_ms, other_end) {
            return Err(invalid_request(format!(
                "clip {} would overlap {} on track {}",
                clip.id, other.id, clip.track_id
            )));
        }
    }

    let mut candidate = project.clone();
    for c in candidate.clips.iter_mut() {
        if c.id == payload.clip_id {
            c.start_ms = payload.start_ms;
            c.in_ms = payload.in_ms;
            c.out_ms = payload.out_ms;
        }
    }
    Ok((candidate, "Trim clip".to_string()))
}

// ---- deleteClips ------------------------------------------------------------

/// `deleteClips{clipIds, closeGap}`: removes the clips plus every
/// effect/caption/marker/transition that referenced one of them. With
/// `closeGap`, later clips on the SAME track (never another track --
/// "track-local ripple") shift earlier by however much of that track's own
/// removed span precedes them. Refused, atomically (nothing installed),
/// when a clip that WOULD shift is grouped with a clip on a different
/// track (`"Ripple would break group <id>"` -- shifting it alone would
/// desync the group), or when a deleted clip carries a transition (ripple
/// would move one endpoint of an association that assumes fixed timing).
pub(super) fn delete_clips(
    project: &Project,
    payload: &DeleteClipsPayload,
) -> Result<(Project, String), EditorError> {
    if payload.clip_ids.is_empty() {
        return Err(invalid_request("clipIds must not be empty"));
    }
    let ids: HashSet<&str> = payload.clip_ids.iter().map(String::as_str).collect();

    let mut track_ids: Vec<String> = Vec::new();
    for id in &payload.clip_ids {
        let clip = find_clip(project, id)?;
        if !track_ids.iter().any(|t| t == &clip.track_id) {
            track_ids.push(clip.track_id.clone());
        }
    }
    for track_id in &track_ids {
        ensure_unlocked(project, track_id)?;
    }

    if payload.close_gap {
        for id in &ids {
            if project
                .transitions
                .iter()
                .any(|t| t.from == *id || t.to == *id)
            {
                return Err(invalid_request(format!(
                    "Ripple would cross a transition on clip {id}"
                )));
            }
        }
        for track_id in &track_ids {
            let deleted: Vec<&Clip> = project
                .clips
                .iter()
                .filter(|c| c.track_id == *track_id && ids.contains(c.id.as_str()))
                .collect();
            for remaining in project
                .clips
                .iter()
                .filter(|c| c.track_id == *track_id && !ids.contains(c.id.as_str()))
            {
                let would_shift = deleted.iter().any(|d| clip_end(d) <= remaining.start_ms);
                if !would_shift {
                    continue;
                }
                if let Some(group_id) = &remaining.group_id {
                    let crosses_track = project.clips.iter().any(|other| {
                        other.track_id != *track_id
                            && other.group_id.as_deref() == Some(group_id.as_str())
                    });
                    if crosses_track {
                        return Err(invalid_request(format!(
                            "Ripple would break group {group_id}"
                        )));
                    }
                }
            }
        }
    }

    let mut candidate = project.clone();
    if payload.close_gap {
        for track_id in &track_ids {
            let deleted: Vec<&Clip> = project
                .clips
                .iter()
                .filter(|c| c.track_id == *track_id && ids.contains(c.id.as_str()))
                .collect();
            for clip in candidate
                .clips
                .iter_mut()
                .filter(|c| c.track_id == *track_id && !ids.contains(c.id.as_str()))
            {
                let shift: u64 = deleted
                    .iter()
                    .filter(|d| clip_end(d) <= clip.start_ms)
                    .map(|d| clip_duration(d))
                    .sum();
                clip.start_ms = clip.start_ms.saturating_sub(shift);
            }
        }
    }

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

    let label = if payload.close_gap {
        "Delete clips and close the gap"
    } else {
        "Delete clips"
    };
    Ok((candidate, label.to_string()))
}

// ---- moveClips ----------------------------------------------------------

/// `moveClips{clipIds, deltaMs, trackId}`: ONE shared delta applied to
/// every clip in the group -- clamped, for the WHOLE group at once, so the
/// earliest-starting clip never goes below 0 (never clamped per clip,
/// which would change the clips' relative spacing and desync the group,
/// `DATA-MODEL.md`: "A group move uses a shared clamped offset, not
/// separate per-clip clamping"). `trackId` is only accepted when moving a
/// single clip (retargeting a whole group to one track at once is not
/// this command's job); when given, the target track must be unlocked
/// and kind-compatible with the moved clip's asset. Overlapping ANY
/// destination clip rejects the WHOLE group atomically -- checked in a
/// read-only pass over `project` before `candidate` is ever built.
///
/// **Group expansion (Task 8, F13)**: before any of the above, the
/// explicit `clipIds` selection widens to every clip sharing a `group_id`
/// with a named clip -- a grouped partner moves too even when the caller
/// only named one member, which is what keeps the group's relative
/// offsets intact (they all receive the SAME clamped delta, computed
/// below over the EXPANDED set). Task 7 shipped this command before any
/// group could exist to expand into, so this behaviour could only land in
/// the task that introduces `groupClips` at all. `trackId`'s
/// single-clip-only rule is checked against the EXPANDED count, not the
/// caller's raw `clipIds` -- retargeting a whole group to one track is
/// still not this command's job even when the group was only implied.
pub(super) fn move_clips(
    project: &Project,
    payload: &MoveClipsPayload,
) -> Result<(Project, String), EditorError> {
    if payload.clip_ids.is_empty() {
        return Err(invalid_request("clipIds must not be empty"));
    }

    // Expand the explicit selection to whole groups: a clip's grouped
    // partners move together even when the caller only named one member.
    // Expansion is transitive (a partner pulled in this pass can itself
    // pull in a third) though in practice every member of one group
    // shares the identical `group_id`, so a single pass already reaches
    // all of them.
    let mut expanded: Vec<String> = payload.clip_ids.clone();
    let mut seen: HashSet<String> = expanded.iter().cloned().collect();
    let mut idx = 0;
    while idx < expanded.len() {
        let clip = find_clip(project, &expanded[idx])?;
        if let Some(group_id) = clip.group_id.clone() {
            for partner in project
                .clips
                .iter()
                .filter(|c| c.group_id.as_deref() == Some(group_id.as_str()))
            {
                if seen.insert(partner.id.clone()) {
                    expanded.push(partner.id.clone());
                }
            }
        }
        idx += 1;
    }

    if payload.track_id.is_some() && expanded.len() != 1 {
        return Err(invalid_request(
            "trackId is only valid when moving a single clip",
        ));
    }
    let ids: HashSet<&str> = expanded.iter().map(String::as_str).collect();

    let mut clips: Vec<&Clip> = Vec::with_capacity(expanded.len());
    for id in &expanded {
        clips.push(find_clip(project, id)?);
    }
    // Any locked member -- including a partner pulled in only by group
    // expansion, never explicitly named by the caller -- rejects the
    // WHOLE move.
    for clip in &clips {
        ensure_unlocked(project, &clip.track_id)?;
    }

    let dest_track_id: Option<&str> = payload.track_id.as_deref();
    if let Some(dest) = dest_track_id {
        ensure_unlocked(project, dest)?;
        let track = find_track(project, dest)?;
        let clip = clips[0];
        let asset = find_asset(project, &clip.asset_id)?;
        let want_kind = match asset.kind {
            AssetKind::Audio => TrackKind::Audio,
            AssetKind::Video => TrackKind::Video,
        };
        if track.kind != want_kind {
            return Err(invalid_request(format!(
                "clip {} cannot move to track {dest}: a {:?} asset needs a {want_kind:?} track",
                clip.id, asset.kind
            )));
        }
    }

    let min_start = clips.iter().map(|c| c.start_ms).min().unwrap();
    let clamped_delta = if payload.delta_ms < 0 {
        payload.delta_ms.max(-(min_start as i64))
    } else {
        payload.delta_ms
    };

    // One checked `new_start` per moved clip, computed ONCE here and reused
    // for the mutation below -- never recomputed with the same unchecked
    // arithmetic twice.
    let mut new_starts: Vec<(String, u64)> = Vec::with_capacity(clips.len());
    for clip in &clips {
        let new_start = apply_delta(clip.start_ms, clamped_delta)?;
        // `apply_delta`'s checked arithmetic alone is not enough to reject
        // an unreasonable positive `deltaMs`: u64 has enough headroom
        // above i64::MAX that adding it never actually overflows u64, so
        // an unbounded delta would otherwise sail through as a
        // business-nonsensical (but arithmetically "valid") position.
        // Bound it against the same ceiling `validate::check_clip` already
        // enforces on an installed clip, so this returns an explicit
        // `invalidRequest` here rather than relying solely on the
        // `validate_project` backstop.
        if new_start > limits::MAX_DURATION_MS {
            return Err(invalid_request(format!(
                "clip {}: deltaMs would move it to {} ms, exceeding the {} ms maximum",
                clip.id,
                new_start,
                limits::MAX_DURATION_MS
            )));
        }
        new_starts.push((clip.id.clone(), new_start));
        let target_track = dest_track_id.unwrap_or(clip.track_id.as_str());
        let new_span = ClipSpan {
            start_ms: new_start,
            in_ms: clip.in_ms,
            out_ms: clip.out_ms,
            speed: speed_or_default(clip.speed.as_ref()),
        };
        let new_end = checked_output_end(&new_span)?;
        for other in project
            .clips
            .iter()
            .filter(|c| c.track_id == target_track && !ids.contains(c.id.as_str()))
        {
            let other_end = clip_end(other);
            if overlaps(new_start, new_end, other.start_ms, other_end) {
                return Err(invalid_request(format!(
                    "clip {} would overlap {} on track {target_track}",
                    clip.id, other.id
                )));
            }
        }
    }

    let mut candidate = project.clone();
    for clip in candidate
        .clips
        .iter_mut()
        .filter(|c| ids.contains(c.id.as_str()))
    {
        let new_start = new_starts
            .iter()
            .find(|(id, _)| *id == clip.id)
            .map(|(_, s)| *s)
            .expect("every moved clip has a precomputed new start from the check pass above");
        clip.start_ms = new_start;
        if let Some(dest) = dest_track_id {
            clip.track_id = dest.to_string();
        }
    }
    Ok((candidate, "Move clips".to_string()))
}

// ---- reorderClip ----------------------------------------------------------

/// `reorderClip{clipId, direction}`: swaps the clip with its adjacent
/// neighbour on the same track (by current `start_ms`), preserving the
/// PAIR's combined span `[a.start, b.end)` -- `a` is whichever of the two
/// currently starts earlier, `b` the later one. After the swap `b` (now
/// first) starts exactly where `a` used to start, and `a` (now second)
/// ENDS exactly where `b` used to end (`a`'s new start is `b_end -
/// a_duration`) -- so both the pair's outer boundary AND any gap that sat
/// between them survive unchanged; only the order flips. A missing
/// neighbour (already first/last on the track) is refused rather than a
/// silent no-op.
pub(super) fn reorder_clip(
    project: &Project,
    payload: &ReorderClipPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    let mut same_track: Vec<&Clip> = project
        .clips
        .iter()
        .filter(|c| c.track_id == clip.track_id)
        .collect();
    same_track.sort_by_key(|c| c.start_ms);
    let idx = same_track
        .iter()
        .position(|c| c.id == clip.id)
        .expect("clip is a member of its own track's clip list");

    let neighbor = match payload.direction {
        ReorderDirection::Earlier => {
            if idx == 0 {
                return Err(invalid_request("Already first"));
            }
            same_track[idx - 1]
        }
        ReorderDirection::Later => {
            if idx + 1 >= same_track.len() {
                return Err(invalid_request("Already last"));
            }
            same_track[idx + 1]
        }
    };

    // `a` is whichever of the pair currently starts earlier, `b` the
    // later one.
    let (a, b) = if payload.direction == ReorderDirection::Earlier {
        (neighbor, clip)
    } else {
        (clip, neighbor)
    };
    let a_id = a.id.clone();
    let b_id = b.id.clone();
    let a_start = a.start_ms;
    let a_duration = clip_duration(a);
    let b_end = clip_end(b);
    let a_new_start = b_end.checked_sub(a_duration).ok_or_else(|| {
        invalid_request("reorder would place the earlier clip before the start of the timeline")
    })?;

    let mut candidate = project.clone();
    for c in candidate.clips.iter_mut() {
        if c.id == b_id {
            c.start_ms = a_start;
        } else if c.id == a_id {
            c.start_ms = a_new_start;
        }
    }
    Ok((candidate, "Reorder clip".to_string()))
}

// Tests live in the sibling `clips_tests.rs` (not inline) so this file
// stays well under the 800-nonblank-line Rust cap as tasks 8/29/30 extend
// it (the `validate.rs`/`validate_tests.rs` precedent).
#[cfg(test)]
#[path = "clips_tests.rs"]
mod tests;
