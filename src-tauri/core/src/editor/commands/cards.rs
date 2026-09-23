//! `addCard`/`updateCard`/`insertIntro` (Task 33; F-37, DATA-MODEL.md §
//! Entities: "editable generated card clips with preset/title/subtitle/
//! background/foreground/accent properties").
//!
//! **A card is a clip like any other**, backed by ONE builtin `card` asset
//! per project (`Builtin::Card`, `duration_ms = limits::MAX_DURATION_MS` --
//! a generated card has no natural length of its own, the same "no
//! natural bound" reasoning `clips::out_ms_bound` already applies to a
//! still image). `ensure_card_asset` mints it exactly once and every later
//! `addCard`/`insertIntro` reuses the SAME id -- the brief's "once per
//! project". A card is identified by its ASSET's `builtin` kind, never by
//! the clip's own inline `Card` content (`Clip.card`, which is what a card
//! LOOKS like, not what makes it one) -- `layout.rs`'s `check_color_targets`
//! doc restates the same rule for `setAdjustments`' refusal.
//!
//! **Colours are the one thing this task validates that nothing else in
//! `core::editor` does**: `updateCard`'s `background`/`foreground`/`accent`
//! must each be `#rrggbb` (case-insensitive hex digits) or the whole call
//! is refused, naming the offending field. `addCard` never takes colours on
//! the wire at all -- every new card starts at the SAME three defaults
//! (`DEFAULT_BACKGROUND`/`DEFAULT_FOREGROUND`/`DEFAULT_ACCENT`, the
//! contract reference's own literal values) regardless of `preset`; `preset`
//! itself is stored and read back but not yet consulted by anything in this
//! file (a later render/preview task's own per-preset layout is out of
//! scope here).
//!
//! **`trackId: null` always mints a brand-new, topmost (index 0) video
//! track** -- it never reuses an existing one, even an empty one, so a
//! caller that wants "just put a card somewhere reasonable" can never
//! silently land on a track that already has a lock or a clip on it.
//! `new_top_video_track` is the ONE place that happens, built on
//! `tracks::add_track` (the SAME `limits::MAX_TRACKS` ceiling and empty-name
//! refusal every other track add gets, rather than a second track
//! constructor -- the task's own instruction) so its refusal (a full
//! project) surfaces through `addCard`/`insertIntro` unchanged.
//!
//! **`insertIntro` is a uniform shift, and that is what makes it safe.**
//! Every existing clip's `start_ms` moves by exactly `durationMs` -- never
//! its `in_ms`/`out_ms`/`track_id`/anything else -- so every relative
//! distance in the project (a transition's `from`/`to` overlap, a group's
//! members, a cue's SOURCE-relative offset) is preserved by construction.
//! Effects/captions/markers are keyed on SOURCE time relative to their own
//! clip (`model_cues.rs`), so this file never touches those three
//! collections at all: "cue source times unchanged" is not a rule this code
//! enforces so much as a fact a start_ms-only shift cannot violate.
//! `transitions::ensure_intact` still runs afterward as the same defensive
//! backstop `layout::set_speed` keeps for its own output-duration change --
//! belt, not suspenders, since a uniform shift cannot actually break a
//! transition's geometry.
//!
//! **The refusal is atomic and locked-EMPTY tracks never trigger it.** Only
//! a POPULATED track (one with at least one clip) that is ALSO locked
//! blocks the whole command -- an empty locked track has nothing to shift,
//! so refusing over it would be refusing to protect nothing. Every check
//! (the lock scan, the overflow/ceiling scan) runs to completion BEFORE any
//! field of any clip is mutated, so a refusal never leaves a half-shifted
//! project -- the same "resolve every target, then mutate" discipline
//! `layout::set_layout`/`set_adjustments` use for their own multi-clip
//! atomicity.

use std::collections::HashSet;

use super::clips::{clip_end, ensure_unlocked, find_clip, find_track, invalid_request, overlaps};
use super::payloads::{AddCardPayload, AddTrackPayload, InsertIntroPayload, UpdateCardPayload};
use super::tracks;
use super::transitions;
use crate::editor::error::EditorError;
use crate::editor::ids::new_entity_id;
use crate::editor::limits;
use crate::editor::model::{
    Asset, AssetKind, Builtin, Card, CardPreset, Clip, FadeCurve, Project, TrackKind,
};
use crate::editor::{Map, Num};

const DEFAULT_BACKGROUND: &str = "#18191e";
const DEFAULT_FOREGROUND: &str = "#f0eef6";
const DEFAULT_ACCENT: &str = "#b6a2f5";
/// The name given to a video track this file mints itself (`trackId: null`
/// on `addCard`, and always for `insertIntro`) -- never shown as a
/// per-card label, just a sensible default the user can rename like any
/// other track.
const NEW_TRACK_NAME: &str = "Titles";

fn num(v: i64) -> Num {
    Num::from(v)
}

/// `#rrggbb`, case-insensitive hex digits -- exactly the CSS/HTML colour
/// literal shape the reference format's `background`/`foreground`/`accent`
/// fields use. No named colours, no `#rgb` shorthand, no alpha channel.
fn is_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

fn validate_hex(field: &str, value: &str) -> Result<(), EditorError> {
    if !is_hex_color(value) {
        return Err(invalid_request(format!(
            "{field} must be a #rrggbb colour, got {value:?}"
        )));
    }
    Ok(())
}

/// The one place a project's builtin `card` asset is minted. Returns the
/// EXISTING id when one is already present -- "once per project" (brief) --
/// so a second/third/… card never grows `Project.assets` at all.
fn ensure_card_asset(candidate: &mut Project) -> String {
    if let Some(existing) = candidate
        .assets
        .iter()
        .find(|a| a.builtin == Some(Builtin::Card))
    {
        return existing.id.clone();
    }
    let id = new_entity_id("asset");
    candidate.assets.push(Asset {
        id: id.clone(),
        kind: AssetKind::Video,
        name: "Title card".to_string(),
        duration_ms: limits::MAX_DURATION_MS,
        width: None,
        height: None,
        size: None,
        builtin: Some(Builtin::Card),
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    });
    id
}

/// A fresh, TOPMOST (index 0) video track via `tracks::add_track` -- the
/// same `limits::MAX_TRACKS` ceiling and empty-name refusal every other
/// track add gets, never a second constructor (the task's own instruction).
/// Returns the candidate project plus the new track's freshly-minted id.
fn new_top_video_track(project: &Project) -> Result<(Project, String), EditorError> {
    let (candidate, _label) = tracks::add_track(
        project,
        &AddTrackPayload {
            kind: TrackKind::Video,
            name: NEW_TRACK_NAME.to_string(),
            index: 0,
        },
    )?;
    let new_id = candidate.tracks[0].id.clone();
    Ok((candidate, new_id))
}

/// Resolves `addCard`'s target track: an explicit `trackId` must resolve to
/// an unlocked VIDEO track (never reused if it is audio or locked);
/// `None` always mints a brand-new topmost one (`new_top_video_track`),
/// never reusing an existing empty track.
fn resolve_target_track(
    project: &Project,
    track_id: Option<&str>,
) -> Result<(Project, String), EditorError> {
    match track_id {
        Some(id) => {
            let track = find_track(project, id)?;
            if track.kind != TrackKind::Video {
                return Err(invalid_request(format!("track {id} is not a video track")));
            }
            ensure_unlocked(project, id)?;
            Ok((project.clone(), id.to_string()))
        }
        None => new_top_video_track(project),
    }
}

fn new_card_clip(
    asset_id: &str,
    track_id: &str,
    preset: CardPreset,
    start_ms: u64,
    duration_ms: u64,
    title: &str,
    subtitle: &str,
) -> Clip {
    Clip {
        id: new_entity_id("clip"),
        asset_id: asset_id.to_string(),
        track_id: track_id.to_string(),
        name: title.to_string(),
        start_ms,
        in_ms: 0,
        out_ms: duration_ms,
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
        card: Some(Card {
            preset,
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            background: DEFAULT_BACKGROUND.to_string(),
            foreground: DEFAULT_FOREGROUND.to_string(),
            accent: DEFAULT_ACCENT.to_string(),
            extra: Map::new(),
        }),
        extra: Map::new(),
    }
}

/// The `[startMs, startMs+durationMs)` bounds check every card-inserting
/// command shares: a minimum span (`limits::MIN_CLIP_MS`, the
/// `setSpeed`/`trimClip` precedent) and a checked, non-overflowing end that
/// must not pass `limits::MAX_DURATION_MS`.
fn checked_card_end(start_ms: u64, duration_ms: u64) -> Result<u64, EditorError> {
    if duration_ms < limits::MIN_CLIP_MS {
        return Err(invalid_request(format!(
            "durationMs {duration_ms} is below the {} ms minimum",
            limits::MIN_CLIP_MS
        )));
    }
    let end = start_ms
        .checked_add(duration_ms)
        .ok_or_else(|| invalid_request("startMs + durationMs overflows"))?;
    if end > limits::MAX_DURATION_MS {
        return Err(invalid_request(format!(
            "card would end at {end} ms, past the {} ms maximum",
            limits::MAX_DURATION_MS
        )));
    }
    Ok(end)
}

// ---- addCard ----------------------------------------------------------------

/// `addCard{preset, trackId, startMs, durationMs, title, subtitle}` (F-37):
/// mints (once per project) the builtin `card` asset, resolves the target
/// track (see `resolve_target_track`), refuses an overlap with an existing
/// clip on that track, then inserts the new card clip. See the module doc
/// for why colours are never a wire field here.
pub(super) fn add_card(
    project: &Project,
    payload: &AddCardPayload,
) -> Result<(Project, String), EditorError> {
    let end = checked_card_end(payload.start_ms, payload.duration_ms)?;
    let (mut candidate, track_id) = resolve_target_track(project, payload.track_id.as_deref())?;

    for existing in candidate.clips.iter().filter(|c| c.track_id == track_id) {
        let existing_end = clip_end(existing);
        if overlaps(payload.start_ms, end, existing.start_ms, existing_end) {
            return Err(invalid_request(format!(
                "card would overlap {} on track {track_id}",
                existing.id
            )));
        }
    }

    let asset_id = ensure_card_asset(&mut candidate);
    candidate.clips.push(new_card_clip(
        &asset_id,
        &track_id,
        payload.preset,
        payload.start_ms,
        payload.duration_ms,
        &payload.title,
        &payload.subtitle,
    ));
    Ok((candidate, "Add title card".to_string()))
}

// ---- updateCard ---------------------------------------------------------

/// `updateCard{clipId, title?, subtitle?, background?, foreground?,
/// accent?}` (F-37): refuses an unknown clip, a clip that is not a card
/// (`Clip.card.is_none()`), a locked track, a malformed colour (checked
/// for EVERY field present before anything changes, so one bad field can
/// never leave another already written), and a payload that sets nothing.
pub(super) fn update_card(
    project: &Project,
    payload: &UpdateCardPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    if clip.card.is_none() {
        return Err(invalid_request(format!(
            "clip {} is not a title card",
            payload.clip_id
        )));
    }
    ensure_unlocked(project, &clip.track_id)?;

    if payload.title.is_none()
        && payload.subtitle.is_none()
        && payload.background.is_none()
        && payload.foreground.is_none()
        && payload.accent.is_none()
    {
        return Err(invalid_request("updateCard must set at least one field"));
    }
    for (field, value) in [
        ("background", &payload.background),
        ("foreground", &payload.foreground),
        ("accent", &payload.accent),
    ] {
        if let Some(v) = value {
            validate_hex(field, v)?;
        }
    }

    let mut candidate = project.clone();
    if let Some(c) = candidate.clips.iter_mut().find(|c| c.id == payload.clip_id) {
        if let Some(card) = c.card.as_mut() {
            if let Some(t) = &payload.title {
                card.title = t.clone();
            }
            if let Some(s) = &payload.subtitle {
                card.subtitle = s.clone();
            }
            if let Some(b) = &payload.background {
                card.background = b.clone();
            }
            if let Some(f) = &payload.foreground {
                card.foreground = f.clone();
            }
            if let Some(a) = &payload.accent {
                card.accent = a.clone();
            }
        }
    }
    Ok((candidate, "Edit title card".to_string()))
}

// ---- insertIntro ----------------------------------------------------------

/// The set of track ids that own at least one clip -- what "populated"
/// means for the lock refusal below (an empty track has nothing to shift).
fn populated_track_ids(project: &Project) -> HashSet<&str> {
    project.clips.iter().map(|c| c.track_id.as_str()).collect()
}

/// `insertIntro{durationMs, title, subtitle}` (F-37): refuses atomically
/// (nothing moves) if any POPULATED track is locked or if the shift would
/// push any clip past `limits::MAX_DURATION_MS`; otherwise shifts every
/// clip's `start_ms` by `durationMs` and inserts the intro card at 0 on a
/// brand-new topmost video track. See the module doc for why cue source
/// times, transitions and groups all survive this untouched.
pub(super) fn insert_intro(
    project: &Project,
    payload: &InsertIntroPayload,
) -> Result<(Project, String), EditorError> {
    checked_card_end(0, payload.duration_ms)?;

    let populated = populated_track_ids(project);
    for track in project
        .tracks
        .iter()
        .filter(|t| populated.contains(t.id.as_str()))
    {
        if track.locked {
            return Err(invalid_request(format!(
                "Unlock {} to insert an intro before everything",
                track.name
            )));
        }
    }
    for clip in &project.clips {
        let new_end = clip_end(clip)
            .checked_add(payload.duration_ms)
            .ok_or_else(|| invalid_request(format!("shifting clip {} overflows", clip.id)))?;
        if new_end > limits::MAX_DURATION_MS {
            return Err(invalid_request(format!(
                "clip {} would end at {new_end} ms, past the {} ms maximum",
                clip.id,
                limits::MAX_DURATION_MS
            )));
        }
    }

    let mut candidate = project.clone();
    for clip in candidate.clips.iter_mut() {
        // Checked above for every clip in `project`; `candidate` is a plain
        // clone of it, so this addition cannot overflow here either.
        clip.start_ms += payload.duration_ms;
    }

    let (mut candidate, track_id) = new_top_video_track(&candidate)?;
    let asset_id = ensure_card_asset(&mut candidate);
    candidate.clips.push(new_card_clip(
        &asset_id,
        &track_id,
        CardPreset::Intro,
        0,
        payload.duration_ms,
        &payload.title,
        &payload.subtitle,
    ));

    transitions::ensure_intact(&candidate, "Insert intro")?;
    Ok((candidate, "Insert intro".to_string()))
}

// Tests live in the sibling `cards_tests.rs`, the `clips.rs`/
// `clips_tests.rs` precedent.
#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
