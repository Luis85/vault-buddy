//! `setClipMix`/`setMasterGain`/`detachAudio` (Task 27; F-05, F-24, F-25).
//!
//! **Everything here is RENDER-AFFECTING** -- a clip's volume and mute, the
//! master gain, a detached audio clip. The workspace's `monitor_muted` and
//! the transport's monitoring volume are NOT, and never reach this file:
//! they live in `editorWorkspace` and change only what the user hears
//! while previewing (NATIVE-MEDIA.md: "Browser monitoring volume/mute never
//! changes the exported mix unless the user changes a render-affecting
//! control"). Track volume/mute/solo are `tracks::set_track_flags`'s, whose
//! module doc states the solo rule the mixer UI and the preview both read.
//!
//! **Ranges are refused here, not left to `validate_project`** (unlike
//! `tracks::set_track_flags`' own volume): a slider that sends 2.5 must get
//! the command's own `invalidRequest` naming the range, not an
//! `invalidProject` about a candidate the user never saw. `validate_project`
//! keeps its identical backstop (`check_clip`, `check_master_gain`).
//!
//! **`detachAudio` needs a fact the graph does not hold**: whether the
//! clip's source media HAS an audio stream. That lives in `sources.json`
//! (`hasAudio`) and arrives through `CommandContext::assets_with_audio`,
//! which the shell fills (`session_commands::execute_in`). A staged capture
//! recorded with no audio device, or an imported silent video, is refused
//! -- detaching it would add an audio clip with nothing in it (R20).

use std::collections::HashSet;

use super::clips::{
    clip_end, ensure_unlocked, find_asset, find_clip, find_track, invalid_request, overlaps,
};
use super::payloads::{DetachAudioPayload, SetClipMixPayload, SetMasterGainPayload};
use super::CommandContext;
use crate::editor::error::EditorError;
use crate::editor::ids::{is_valid_id, new_entity_id};
use crate::editor::limits;
use crate::editor::model::{Asset, AssetKind, Clip, MediaType, Project, Track, TrackKind};
use crate::editor::{Map, Num};

/// `Clip.volume`'s inclusive range (`workspace.schema.json`: `[0,2]`).
const CLIP_VOLUME_MAX: f64 = 2.0;
/// `Project.master_gain`'s inclusive range (`[0,1]`).
const MASTER_GAIN_MAX: f64 = 1.0;
/// The linked asset's id is `<source id>` + this (the brief's `<id>-audio`).
const AUDIO_ID_SUFFIX: &str = "-audio";
/// Appended to the source's (and the clip's) name.
const AUDIO_NAME_SUFFIX: &str = " · audio";
/// The name of the track `detachAudio` creates when no `audioTrackId` is
/// given (the reference editor's own label for it).
const DETACHED_TRACK_NAME: &str = "Detached audio";

// ---- setClipMix -----------------------------------------------------------------

/// `setClipMix{clipIds, volume?, muted?}`: sets whichever of the two fields
/// the payload carries on every named clip. Refuses an empty `clipIds`, a
/// payload that changes nothing, an unknown clip, a clip on a locked track
/// (`clips::ensure_unlocked`) and a volume outside `[0,2]`.
pub(super) fn set_clip_mix(
    project: &Project,
    payload: &SetClipMixPayload,
) -> Result<(Project, String), EditorError> {
    if payload.clip_ids.is_empty() {
        return Err(invalid_request("clipIds must not be empty"));
    }
    if payload.volume.is_none() && payload.muted.is_none() {
        return Err(invalid_request("setClipMix must set volume or muted"));
    }
    if let Some(volume) = &payload.volume {
        let v = volume.as_f64().unwrap_or(f64::NAN);
        if !(0.0..=CLIP_VOLUME_MAX).contains(&v) {
            return Err(invalid_request(format!(
                "volume {volume} must be within [0,2]"
            )));
        }
    }
    let ids: HashSet<&str> = payload.clip_ids.iter().map(String::as_str).collect();
    for id in &ids {
        let clip = find_clip(project, id)?;
        ensure_unlocked(project, &clip.track_id)?;
    }

    let mut candidate = project.clone();
    for c in candidate
        .clips
        .iter_mut()
        .filter(|c| ids.contains(c.id.as_str()))
    {
        if let Some(v) = &payload.volume {
            c.volume = v.clone();
        }
        if let Some(m) = payload.muted {
            c.muted = m;
        }
    }
    let label = match (&payload.volume, payload.muted) {
        (Some(_), _) => "Clip volume",
        (None, Some(true)) => "Mute clip",
        _ => "Unmute clip",
    };
    Ok((candidate, label.to_string()))
}

// ---- setMasterGain ----------------------------------------------------------------

/// `setMasterGain{gain}`: `gain` within `[0,1]` (the render never boosts
/// the master bus; per-clip and per-track gains go to 2).
pub(super) fn set_master_gain(
    project: &Project,
    payload: &SetMasterGainPayload,
) -> Result<(Project, String), EditorError> {
    if !(0.0..=MASTER_GAIN_MAX).contains(&payload.gain) {
        return Err(invalid_request(format!(
            "gain {} must be within [0,1]",
            payload.gain
        )));
    }
    let mut candidate = project.clone();
    candidate.master_gain = payload.gain;
    Ok((candidate, "Master gain".to_string()))
}

// ---- detachAudio ------------------------------------------------------------------

/// `name` + `AUDIO_NAME_SUFFIX`, cutting `name` (never the suffix) so the
/// result stays within `limits::MAX_NAME_CHARS` -- a 300-character source
/// name must not make its own detach fail validation.
fn audio_name(name: &str) -> String {
    let room = limits::MAX_NAME_CHARS - AUDIO_NAME_SUFFIX.chars().count();
    let mut out: String = name.chars().take(room).collect();
    out.push_str(AUDIO_NAME_SUFFIX);
    out
}

/// The clip's asset, provided it is a video with an audio stream -- the
/// brief's precondition, and the only thing in this file that reads
/// `CommandContext`.
fn audible_video<'a>(
    project: &'a Project,
    clip: &Clip,
    ctx: &CommandContext<'_>,
) -> Result<&'a Asset, EditorError> {
    let source = find_asset(project, &clip.asset_id)?;
    if source.kind != AssetKind::Video || matches!(source.media_type, Some(MediaType::Image)) {
        return Err(invalid_request("Only a video clip's audio can be detached"));
    }
    if !ctx.assets_with_audio.contains(&source.id) {
        return Err(invalid_request(format!(
            "{} has no audio to detach",
            source.name
        )));
    }
    Ok(source)
}

/// The linked audio asset for `source`: the EXISTING one when an earlier
/// detach of another clip of the same source already made it (`Ok(None)`
/// -- nothing to add), a new one otherwise. An asset already wearing the
/// derived id that is NOT this source's detached audio is refused, never
/// overwritten or shadowed.
fn linked_audio_asset(
    project: &Project,
    source: &Asset,
) -> Result<(String, Option<Asset>), EditorError> {
    let id = format!("{}{AUDIO_ID_SUFFIX}", source.id);
    if !is_valid_id(&id) {
        return Err(invalid_request(format!(
            "asset id {} is too long to derive a detached-audio id from",
            source.id
        )));
    }
    if let Some(existing) = project.assets.iter().find(|a| a.id == id) {
        if existing.kind != AssetKind::Audio || existing.linked_asset.as_deref() != Some(&source.id)
        {
            return Err(invalid_request(format!(
                "asset {id} already exists and is not {}'s detached audio",
                source.id
            )));
        }
        return Ok((id, None));
    }
    let asset = Asset {
        id: id.clone(),
        kind: AssetKind::Audio,
        name: audio_name(&source.name),
        duration_ms: source.duration_ms,
        width: None,
        height: None,
        size: None,
        builtin: None,
        media_type: None,
        linked_asset: Some(source.id.clone()),
        original_name: None,
        extra: Map::new(),
    };
    Ok((id, Some(asset)))
}

/// Where the detached clip goes: the named audio track (unlocked, and free
/// over the clip's output span), or -- `None` -- a new audio track, which
/// is `Some(track)` in the result so the caller adds it.
fn target_track(
    project: &Project,
    clip: &Clip,
    audio_track_id: Option<&str>,
) -> Result<(String, Option<Track>), EditorError> {
    let Some(id) = audio_track_id else {
        if project.tracks.len() >= limits::MAX_TRACKS {
            return Err(invalid_request(format!(
                "a new audio track would exceed the {} track maximum",
                limits::MAX_TRACKS
            )));
        }
        let track = Track {
            id: new_entity_id("trk"),
            kind: TrackKind::Audio,
            name: DETACHED_TRACK_NAME.to_string(),
            visible: true,
            locked: false,
            muted: false,
            solo: false,
            volume: Num::from(1),
            extra: Map::new(),
        };
        return Ok((track.id.clone(), Some(track)));
    };
    let track = find_track(project, id)?;
    if track.kind != TrackKind::Audio {
        return Err(invalid_request(format!(
            "Track {} is not an audio track",
            track.name
        )));
    }
    ensure_unlocked(project, id)?;
    let end = clip_end(clip);
    if let Some(other) = project
        .clips
        .iter()
        .filter(|c| c.track_id == id)
        .find(|c| overlaps(clip.start_ms, end, c.start_ms, clip_end(c)))
    {
        return Err(invalid_request(format!(
            "the detached audio would overlap {} on track {}",
            other.id, track.name
        )));
    }
    Ok((id.to_string(), None))
}

/// The detached clip: IDENTICAL `start/in/out/speed` (and pitch intent,
/// level and fades, so what is heard does not change), on `track_id`,
/// audible. Its visual fields are the `insertClip` defaults -- a picture
/// transform means nothing on an audio track -- and it joins no group: a
/// grouped pair could no longer be moved to another track one clip at a
/// time (`clips::move_clips` refuses `trackId` for an expanded group), so
/// keeping them together is the user's explicit Group, not a side effect.
fn detached_clip(clip: &Clip, asset_id: &str, track_id: &str) -> Clip {
    Clip {
        id: new_entity_id("clip"),
        asset_id: asset_id.to_string(),
        track_id: track_id.to_string(),
        name: audio_name(&clip.name),
        muted: false,
        opacity: Num::from(1),
        x: Num::from(0),
        y: Num::from(0),
        w: Num::from(1),
        h: Num::from(1),
        rotation: None,
        frame_shape: None,
        fit: None,
        mirror: None,
        flip_y: None,
        group_id: None,
        crop_zoom: None,
        crop_x: None,
        crop_y: None,
        adjustments: None,
        card: None,
        extra: Map::new(),
        ..clip.clone()
    }
}

/// `detachAudio{clipId, audioTrackId}` -- see the module doc. One
/// candidate, so one undo step: the linked asset (unless it exists), the
/// new track (unless one was named), the detached clip, and the original
/// clip muted. `validate_media::check_linked_assets` (run by
/// `validate_project` on the candidate) re-checks the link's shape and
/// that the link graph stays acyclic.
pub(super) fn detach_audio(
    project: &Project,
    payload: &DetachAudioPayload,
    ctx: &CommandContext<'_>,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    let source = audible_video(project, clip, ctx)?;
    let (audio_id, new_asset) = linked_audio_asset(project, source)?;
    let already = project.clips.iter().any(|c| {
        c.asset_id == audio_id
            && (c.start_ms, c.in_ms, c.out_ms) == (clip.start_ms, clip.in_ms, clip.out_ms)
    });
    if already {
        return Err(invalid_request(format!(
            "The audio of {} is already detached",
            clip.name
        )));
    }
    let (track_id, new_track) = target_track(project, clip, payload.audio_track_id.as_deref())?;

    let detached = detached_clip(clip, &audio_id, &track_id);
    let mut candidate = project.clone();
    candidate.assets.extend(new_asset);
    candidate.tracks.extend(new_track);
    candidate.clips.push(detached);
    if let Some(original) = candidate.clips.iter_mut().find(|c| c.id == clip.id) {
        original.muted = true;
    }
    Ok((candidate, "Detach audio".to_string()))
}

#[cfg(test)]
#[path = "mix_tests.rs"]
mod tests;
