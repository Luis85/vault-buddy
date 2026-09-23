//! Semantic validation for a tutorial project and its saved workspace
//! envelope (`DATA-MODEL.md` § Validation order, steps 3-8). The JSON
//! schema (`workspace.schema.json`) checks structure only ("Run semantic
//! validation for unique IDs, ranges, paths, cross-references,
//! transitions, linked-asset cycles and snapshot ownership" — its own
//! description); this module and `validate_media` are that authority.
//! `validate_project` never partially applies a candidate: the first rule
//! it finds broken stops the walk and returns `Err`, leaving the caller's
//! current, already-valid graph untouched (step 8's "installs atomically"
//! is the caller's job — this module only decides valid or not).

use std::collections::HashMap;

use super::error::{EditorError, EditorErrorCode};
use super::limits;
use super::model::{Asset, Clip, MediaType, Project, Track, TrackKind};
use super::model_cues::{CaptionCue, Effect, EffectKind, Marker, WorkspaceEnvelope};
use super::{validate_media, Num};

/// `Track.name`'s cap is `workspace.schema.json`'s `maxLength: 200` on the
/// `track` definition, stricter than the flat `limits::MAX_NAME_CHARS`
/// (300) every other named entity uses (controller ruling: a per-entity
/// schema cap wins wherever it is stricter than the flat limit).
const TRACK_NAME_MAX: usize = 200;

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

/// Reads a `Num` as `f64`, naming the entity/field on the rare failure
/// (`serde_json::Number::as_f64` returns `None` only for a value it cannot
/// represent as a finite float).
fn as_f64(id: &str, field: &str, n: &Num) -> Result<f64, EditorError> {
    n.as_f64()
        .ok_or_else(|| invalid(format!("{id}: {field} is not a finite number")))
}

fn check_range(id: &str, field: &str, value: f64, lo: f64, hi: f64) -> Result<(), EditorError> {
    if !(lo..=hi).contains(&value) {
        return Err(invalid(format!(
            "{id}: {field} {value} must be within [{lo},{hi}]"
        )));
    }
    Ok(())
}

/// `speed`'s effective value for arithmetic: the clip's own value when set
/// (already range-checked by `check_clip` before any caller outside this
/// module's clip loop can see it), else the reference format's implicit
/// 1.0×.
pub(crate) fn speed_or_default(speed: Option<&Num>) -> f64 {
    speed.and_then(Num::as_f64).unwrap_or(1.0)
}

/// `round((out_ms-in_ms)/speed)` (`DATA-MODEL.md` § Timing rules).
pub(crate) fn output_duration_ms(in_ms: u64, out_ms: u64, speed: f64) -> u64 {
    let raw = out_ms.saturating_sub(in_ms) as f64;
    (raw / speed).round() as u64
}

/// Builds an id -> entity map, rejecting the first id that fails
/// `is_valid_id` or repeats within the collection (`DATA-MODEL.md` §
/// Validation order step 4: "Require unique IDs per entity collection").
fn index_by_id<'a, T>(
    items: impl Iterator<Item = (&'a str, &'a T)>,
    kind: &str,
) -> Result<HashMap<&'a str, &'a T>, EditorError> {
    let mut map = HashMap::new();
    for (id, item) in items {
        if !super::is_valid_id(id) {
            return Err(invalid(format!("{kind} {id}: id is not a valid entity id")));
        }
        if map.insert(id, item).is_some() {
            return Err(invalid(format!("{kind} {id}: duplicate id")));
        }
    }
    Ok(map)
}

fn check_len(len: usize, max: usize, name: &str) -> Result<(), EditorError> {
    if len > max {
        return Err(invalid(format!(
            "project: {name} has {len} entries, exceeding the {max} maximum"
        )));
    }
    Ok(())
}

fn check_sizes(p: &Project) -> Result<(), EditorError> {
    check_len(p.assets.len(), limits::MAX_ASSETS, "assets")?;
    check_len(p.tracks.len(), limits::MAX_TRACKS, "tracks")?;
    check_len(p.clips.len(), limits::MAX_CLIPS, "clips")?;
    check_len(p.effects.len(), limits::MAX_EFFECTS, "effects")?;
    check_len(p.markers.len(), limits::MAX_MARKERS, "markers")?;
    check_len(p.transitions.len(), limits::MAX_TRANSITIONS, "transitions")?;
    if let Some(captions) = &p.captions {
        check_len(captions.cues.len(), limits::MAX_CAPTIONS, "captions")?;
    }
    Ok(())
}

fn check_name(id: &str, kind: &str, name: &str, max: usize) -> Result<(), EditorError> {
    if name.chars().count() > max {
        return Err(invalid(format!(
            "{kind} {id}: name exceeds {max} characters"
        )));
    }
    Ok(())
}

fn check_title_and_names(p: &Project) -> Result<(), EditorError> {
    if p.title.chars().count() > limits::MAX_TITLE_CHARS {
        return Err(invalid(format!(
            "project {}: title exceeds {} characters",
            p.id,
            limits::MAX_TITLE_CHARS
        )));
    }
    for asset in &p.assets {
        check_name(&asset.id, "asset", &asset.name, limits::MAX_NAME_CHARS)?;
    }
    for track in &p.tracks {
        check_name(&track.id, "track", &track.name, TRACK_NAME_MAX)?;
    }
    for clip in &p.clips {
        check_name(&clip.id, "clip", &clip.name, limits::MAX_NAME_CHARS)?;
    }
    Ok(())
}

fn check_canvas(p: &Project) -> Result<(), EditorError> {
    let pair = (p.canvas.width, p.canvas.height);
    if !limits::CANVASES.contains(&pair) {
        return Err(invalid(format!(
            "project {}: canvas {}x{} is not one of the supported presets",
            p.id, p.canvas.width, p.canvas.height
        )));
    }
    if p.canvas.fps != limits::CANVAS_FPS {
        return Err(invalid(format!(
            "project {}: canvas fps {} must be {}",
            p.id,
            p.canvas.fps,
            limits::CANVAS_FPS
        )));
    }
    Ok(())
}

fn check_master_gain(p: &Project) -> Result<(), EditorError> {
    if !(0.0..=1.0).contains(&p.master_gain) {
        return Err(invalid(format!(
            "project {}: master_gain {} must be within [0,1]",
            p.id, p.master_gain
        )));
    }
    Ok(())
}

/// `Track.volume` (`workspace.schema.json`'s `track.volume`: `minimum: 0,
/// maximum: 2`) — controller ruling: added alongside the fields the brief
/// already listed by name, since it is the same class of scalar-range
/// check (step 3) the schema bounds just as concretely.
fn check_track(track: &Track) -> Result<(), EditorError> {
    let v = as_f64(&track.id, "volume", &track.volume)?;
    check_range(&track.id, "volume", v, 0.0, 2.0)?;
    Ok(())
}

fn check_clip(
    clip: &Clip,
    assets: &HashMap<&str, &Asset>,
    tracks: &HashMap<&str, &Track>,
) -> Result<(), EditorError> {
    let asset = *assets.get(clip.asset_id.as_str()).ok_or_else(|| {
        invalid(format!(
            "clip {}: asset_id {} does not resolve",
            clip.id, clip.asset_id
        ))
    })?;
    let track = *tracks.get(clip.track_id.as_str()).ok_or_else(|| {
        invalid(format!(
            "clip {}: track_id {} does not resolve",
            clip.id, clip.track_id
        ))
    })?;

    let want_track_kind = match asset.kind {
        super::model::AssetKind::Audio => TrackKind::Audio,
        super::model::AssetKind::Video => TrackKind::Video,
    };
    if track.kind != want_track_kind {
        return Err(invalid(format!(
            "clip {}: {:?} asset {} cannot sit on a {:?} track",
            clip.id, asset.kind, asset.id, track.kind
        )));
    }

    if clip.in_ms >= clip.out_ms {
        return Err(invalid(format!(
            "clip {}: in_ms {} must be before out_ms {}",
            clip.id, clip.in_ms, clip.out_ms
        )));
    }
    let is_image = matches!(asset.media_type, Some(MediaType::Image));
    let out_bound = if is_image {
        limits::MAX_DURATION_MS
    } else {
        asset.duration_ms
    };
    if clip.out_ms > out_bound {
        return Err(invalid(format!(
            "clip {}: out_ms {} exceeds the {} ms {}",
            clip.id,
            clip.out_ms,
            out_bound,
            if is_image {
                "maximum duration"
            } else {
                "asset duration"
            }
        )));
    }

    if let Some(speed) = clip.speed.as_ref() {
        let sf = as_f64(&clip.id, "speed", speed)?;
        check_range(&clip.id, "speed", sf, limits::SPEED_MIN, limits::SPEED_MAX)?;
    }
    let speed = speed_or_default(clip.speed.as_ref());
    let output_duration = output_duration_ms(clip.in_ms, clip.out_ms, speed);
    let end = clip.start_ms.checked_add(output_duration).ok_or_else(|| {
        invalid(format!(
            "clip {}: start_ms + output duration overflows",
            clip.id
        ))
    })?;
    if end > limits::MAX_DURATION_MS {
        return Err(invalid(format!(
            "clip {}: ends at {end} ms, exceeding the {} ms maximum",
            clip.id,
            limits::MAX_DURATION_MS
        )));
    }

    let x = as_f64(&clip.id, "x", &clip.x)?;
    check_range(&clip.id, "x", x, 0.0, 1.0)?;
    let y = as_f64(&clip.id, "y", &clip.y)?;
    check_range(&clip.id, "y", y, 0.0, 1.0)?;
    let w = as_f64(&clip.id, "w", &clip.w)?;
    check_range(&clip.id, "w", w, 0.1, 1.0)?;
    let h = as_f64(&clip.id, "h", &clip.h)?;
    check_range(&clip.id, "h", h, 0.1, 1.0)?;

    // `Clip.opacity`/`Clip.volume` (`workspace.schema.json`: opacity
    // `[0,1]`, volume `[0,2]`) — controller ruling, same class of check as
    // x/y/w/h just above.
    let opacity = as_f64(&clip.id, "opacity", &clip.opacity)?;
    check_range(&clip.id, "opacity", opacity, 0.0, 1.0)?;
    let volume = as_f64(&clip.id, "volume", &clip.volume)?;
    check_range(&clip.id, "volume", volume, 0.0, 2.0)?;

    if let Some(crop_zoom) = clip.crop_zoom.as_ref() {
        let v = as_f64(&clip.id, "crop_zoom", crop_zoom)?;
        check_range(&clip.id, "crop_zoom", v, 1.0, 3.0)?;
    }

    if clip.fade_in_ms.saturating_mul(2) > output_duration {
        return Err(invalid(format!(
            "clip {}: fade_in_ms {} exceeds half the {} ms output duration",
            clip.id, clip.fade_in_ms, output_duration
        )));
    }
    if clip.fade_out_ms.saturating_mul(2) > output_duration {
        return Err(invalid(format!(
            "clip {}: fade_out_ms {} exceeds half the {} ms output duration",
            clip.id, clip.fade_out_ms, output_duration
        )));
    }

    Ok(())
}

fn require_text(effect: &Effect) -> Result<(), EditorError> {
    if effect.text.is_none() {
        return Err(invalid(format!(
            "effect {}: {:?} requires text",
            effect.id, effect.kind
        )));
    }
    Ok(())
}

fn check_effect(effect: &Effect, clips: &HashMap<&str, &Clip>) -> Result<(), EditorError> {
    if !clips.contains_key(effect.clip_id.as_str()) {
        return Err(invalid(format!(
            "effect {}: clip_id {} does not resolve",
            effect.id, effect.clip_id
        )));
    }
    if effect.start_ms >= effect.end_ms {
        return Err(invalid(format!(
            "effect {}: start_ms {} must be before end_ms {}",
            effect.id, effect.start_ms, effect.end_ms
        )));
    }
    match effect.kind {
        EffectKind::Text => require_text(effect)?,
        EffectKind::Arrow => {
            if effect.x2.is_none() || effect.y2.is_none() {
                return Err(invalid(format!(
                    "effect {}: arrow requires x2 and y2",
                    effect.id
                )));
            }
        }
        EffectKind::Zoom => {
            if effect.factor.is_none() {
                return Err(invalid(format!(
                    "effect {}: zoom requires factor",
                    effect.id
                )));
            }
        }
        EffectKind::Step => {
            if effect.number.is_none() {
                return Err(invalid(format!(
                    "effect {}: step requires number",
                    effect.id
                )));
            }
            require_text(effect)?;
        }
        EffectKind::Highlight | EffectKind::Spotlight | EffectKind::Mask => {}
    }
    Ok(())
}

fn check_caption(cue: &CaptionCue, clips: &HashMap<&str, &Clip>) -> Result<(), EditorError> {
    if !clips.contains_key(cue.clip_id.as_str()) {
        return Err(invalid(format!(
            "caption {}: clip_id {} does not resolve",
            cue.id, cue.clip_id
        )));
    }
    if cue.start_ms >= cue.end_ms {
        return Err(invalid(format!(
            "caption {}: start_ms {} must be before end_ms {}",
            cue.id, cue.start_ms, cue.end_ms
        )));
    }
    Ok(())
}

fn check_marker(
    marker: &Marker,
    clips: &HashMap<&str, &Clip>,
    assets: &HashMap<&str, &Asset>,
) -> Result<(), EditorError> {
    let clip = *clips.get(marker.clip_id.as_str()).ok_or_else(|| {
        invalid(format!(
            "marker {}: clip_id {} does not resolve",
            marker.id, marker.clip_id
        ))
    })?;
    let asset = *assets.get(clip.asset_id.as_str()).ok_or_else(|| {
        invalid(format!(
            "marker {}: clip {} has an unresolved asset",
            marker.id, clip.id
        ))
    })?;
    if marker.source_ms > asset.duration_ms {
        return Err(invalid(format!(
            "marker {}: source_ms {} is outside asset {}'s {} ms duration",
            marker.id, marker.source_ms, asset.id, asset.duration_ms
        )));
    }
    Ok(())
}

/// Validates a `Project` graph against `DATA-MODEL.md` § Validation order
/// steps 3-7, in the order the brief lists them so an error names the
/// first rule the document actually breaks rather than whichever one a
/// different traversal order would have reached first.
pub fn validate_project(p: &Project) -> Result<(), EditorError> {
    if p.schema != super::PROJECT_SCHEMA {
        return Err(invalid(format!(
            "project {}: schema must be {:?}, got {:?}",
            p.id,
            super::PROJECT_SCHEMA,
            p.schema
        )));
    }
    check_sizes(p)?;

    let assets_by_id = index_by_id(p.assets.iter().map(|a| (a.id.as_str(), a)), "asset")?;
    let tracks_by_id = index_by_id(p.tracks.iter().map(|t| (t.id.as_str(), t)), "track")?;
    let clips_by_id = index_by_id(p.clips.iter().map(|c| (c.id.as_str(), c)), "clip")?;
    index_by_id(p.effects.iter().map(|e| (e.id.as_str(), e)), "effect")?;
    index_by_id(p.markers.iter().map(|m| (m.id.as_str(), m)), "marker")?;
    index_by_id(
        p.transitions.iter().map(|t| (t.id.as_str(), t)),
        "transition",
    )?;
    if let Some(captions) = &p.captions {
        index_by_id(captions.cues.iter().map(|c| (c.id.as_str(), c)), "caption")?;
    }

    check_title_and_names(p)?;
    check_canvas(p)?;
    check_master_gain(p)?;

    for track in &p.tracks {
        check_track(track)?;
    }

    for asset in &p.assets {
        if asset.duration_ms > limits::MAX_DURATION_MS {
            return Err(invalid(format!(
                "asset {}: duration_ms {} exceeds the {} ms maximum",
                asset.id,
                asset.duration_ms,
                limits::MAX_DURATION_MS
            )));
        }
    }

    for clip in &p.clips {
        check_clip(clip, &assets_by_id, &tracks_by_id)?;
    }

    for effect in &p.effects {
        check_effect(effect, &clips_by_id)?;
    }

    if let Some(captions) = &p.captions {
        for cue in &captions.cues {
            check_caption(cue, &clips_by_id)?;
        }
    }

    for marker in &p.markers {
        check_marker(marker, &clips_by_id, &assets_by_id)?;
    }

    validate_media::check_transitions(&p.transitions, &clips_by_id, &assets_by_id)?;
    validate_media::check_track_overlaps(&p.clips, &p.transitions)?;
    validate_media::check_linked_assets(&p.assets)?;

    Ok(())
}

/// Validates the saved workspace envelope: the workspace schema string,
/// the `project` graph (`validate_project`), and the record — `revision`,
/// the product count, unique product ids, each product's `project_id`
/// agreement, and every retained snapshot (step 7: "Validate every
/// retained snapshot and its referenced sources, not only the active
/// graph").
pub fn validate_envelope(e: &WorkspaceEnvelope) -> Result<(), EditorError> {
    if e.schema != super::WORKSPACE_SCHEMA {
        return Err(invalid(format!(
            "workspace: schema must be {:?}, got {:?}",
            super::WORKSPACE_SCHEMA,
            e.schema
        )));
    }
    validate_project(&e.project)?;

    if e.record.revision < 1 {
        return Err(invalid(format!(
            "record {}: revision must be >= 1",
            e.record.id
        )));
    }
    check_len(e.record.products.len(), limits::MAX_PRODUCTS, "products")?;

    let mut seen = std::collections::HashSet::new();
    for product in &e.record.products {
        if !seen.insert(product.id.as_str()) {
            return Err(invalid(format!("product {}: duplicate id", product.id)));
        }
        if product.project_id != e.project.id {
            return Err(invalid(format!(
                "product {}: project_id {} does not match the project {}",
                product.id, product.project_id, e.project.id
            )));
        }
        if let Some(snapshot) = &product.snapshot {
            validate_project(snapshot)?;
        }
    }
    Ok(())
}

// Tests live in the sibling `validate_tests.rs` (not inline) so this
// file stays well under the 800-nonblank-line Rust cap as the
// DATA-MODEL.md § Validation order rule coverage grows.
#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
