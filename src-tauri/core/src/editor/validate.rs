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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model_cues::{Product, Record};

    const REFERENCE_WORKSPACE: &str =
        include_str!("../../../../tests/fixtures/editor/reference-workspace.example.json");

    fn base_project_json() -> serde_json::Value {
        serde_json::json!({
            "schema": "vault-buddy-video-project/3",
            "id": "proj1",
            "title": "A tutorial",
            "canvas": {"width": 1280, "height": 720, "fps": 30},
            "master_gain": 0.8,
            "assets": [
                {"id": "a1", "kind": "video", "name": "Asset One", "duration_ms": 5000}
            ],
            "tracks": [
                {
                    "id": "t1", "kind": "video", "name": "Track",
                    "visible": true, "locked": false, "muted": false, "solo": false,
                    "volume": 1
                }
            ],
            "clips": [
                {
                    "id": "c1", "asset_id": "a1", "track_id": "t1", "name": "Clip",
                    "start_ms": 0, "in_ms": 0, "out_ms": 1000,
                    "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
                    "opacity": 1, "volume": 1, "muted": false,
                    "x": 0, "y": 0, "w": 1, "h": 1
                }
            ],
            "effects": [],
            "markers": [],
            "transitions": [],
            "destination": {"vault": "v1", "folder": "Videos", "dated": false}
        })
    }

    fn base_project() -> Project {
        serde_json::from_value(base_project_json()).expect("base project fixture must parse")
    }

    fn base_record() -> Record {
        Record {
            id: "rec1".to_string(),
            revision: 1,
            created_at: "2026-09-21T00:00:00Z".to_string(),
            updated_at: "2026-09-21T00:00:00Z".to_string(),
            products: Vec::new(),
            extra: crate::editor::Map::new(),
        }
    }

    fn base_envelope() -> WorkspaceEnvelope {
        WorkspaceEnvelope {
            schema: super::super::WORKSPACE_SCHEMA.to_string(),
            project: base_project(),
            workspace: serde_json::json!({}),
            record: base_record(),
            saved_at: "2026-09-21T00:00:00Z".to_string(),
            extra: crate::editor::Map::new(),
        }
    }

    #[test]
    fn reference_example_is_valid() {
        let envelope: WorkspaceEnvelope = serde_json::from_str(REFERENCE_WORKSPACE).unwrap();
        assert!(
            validate_envelope(&envelope).is_ok(),
            "the reference tutorial workspace must validate cleanly"
        );
    }

    #[test]
    fn duplicate_clip_ids_are_rejected_naming_the_id() {
        let mut project = base_project();
        let dup = project.clips[0].clone();
        project.clips.push(dup);
        let err = validate_project(&project).unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidProject);
        assert!(err.message.contains("c1"), "message: {}", err.message);
    }

    #[test]
    fn dangling_asset_reference_is_rejected() {
        let mut project = base_project();
        project.clips[0].asset_id = "missing".to_string();
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("missing"), "message: {}", err.message);
    }

    #[test]
    fn audio_asset_on_video_track_is_rejected() {
        let mut project = base_project();
        project.assets.push(
            serde_json::from_value(serde_json::json!({
                "id": "a2", "kind": "audio", "name": "Audio Asset", "duration_ms": 5000
            }))
            .unwrap(),
        );
        project.clips[0].asset_id = "a2".to_string();
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("c1"), "message: {}", err.message);
    }

    #[test]
    fn empty_source_range_is_rejected() {
        let mut project = base_project();
        project.clips[0].in_ms = 500;
        project.clips[0].out_ms = 500;
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("c1"), "message: {}", err.message);
    }

    #[test]
    fn out_ms_beyond_asset_duration_is_rejected() {
        let mut project = base_project();
        project.clips[0].out_ms = project.assets[0].duration_ms + 1;
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("c1"), "message: {}", err.message);
    }

    #[test]
    fn fade_longer_than_half_the_clip_is_rejected() {
        // clips[0]'s output duration is exactly 1000 ms (in_ms 0, out_ms 1000, no speed).
        let mut too_long = base_project();
        too_long.clips[0].fade_in_ms = 501;
        let err = validate_project(&too_long).unwrap_err();
        assert!(
            err.message.contains("fade_in_ms"),
            "message: {}",
            err.message
        );

        let mut exactly_half = base_project();
        exactly_half.clips[0].fade_in_ms = 500;
        assert!(validate_project(&exactly_half).is_ok());
    }

    #[test]
    fn cyclic_linked_assets_are_rejected() {
        let mut project = base_project();
        project.clips.clear();
        project.assets = vec![
            serde_json::from_value(serde_json::json!({
                "id": "a1", "kind": "audio", "name": "A", "duration_ms": 1000,
                "linked_asset": "a2"
            }))
            .unwrap(),
            serde_json::from_value(serde_json::json!({
                "id": "a2", "kind": "audio", "name": "B", "duration_ms": 1000,
                "linked_asset": "a1"
            }))
            .unwrap(),
        ];
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("cycle"), "message: {}", err.message);
    }

    #[test]
    fn self_transition_is_rejected() {
        let mut project = base_project();
        project.transitions.push(
            serde_json::from_value(serde_json::json!({
                "id": "tr1", "from": "c1", "to": "c1", "duration_ms": 100, "kind": "dissolve"
            }))
            .unwrap(),
        );
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("tr1"), "message: {}", err.message);
    }

    #[test]
    fn cross_track_transition_is_rejected() {
        let mut project = base_project();
        project.tracks.push(
            serde_json::from_value(serde_json::json!({
                "id": "t2", "kind": "video", "name": "Track Two",
                "visible": true, "locked": false, "muted": false, "solo": false,
                "volume": 1
            }))
            .unwrap(),
        );
        project.clips.push(
            serde_json::from_value(serde_json::json!({
                "id": "c2", "asset_id": "a1", "track_id": "t2", "name": "Clip Two",
                "start_ms": 2000, "in_ms": 0, "out_ms": 1000,
                "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
                "opacity": 1, "volume": 1, "muted": false,
                "x": 0, "y": 0, "w": 1, "h": 1
            }))
            .unwrap(),
        );
        project.transitions.push(
            serde_json::from_value(serde_json::json!({
                "id": "tr1", "from": "c1", "to": "c2", "duration_ms": 100, "kind": "dissolve"
            }))
            .unwrap(),
        );
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("tr1"), "message: {}", err.message);
        assert!(
            err.message.contains("track"),
            "message should name the track mismatch: {}",
            err.message
        );
    }

    #[test]
    fn oversized_collection_is_rejected() {
        let mut project = base_project();
        let template = project.clips[0].clone();
        project.clips = (0..601)
            .map(|i| {
                let mut c = template.clone();
                c.id = format!("c{i}");
                c
            })
            .collect();
        let err = validate_project(&project).unwrap_err();
        assert!(err.message.contains("clips"), "message: {}", err.message);
    }

    #[test]
    fn snapshot_is_validated_too() {
        let mut envelope = base_envelope();
        let mut bad_snapshot = base_project();
        bad_snapshot.clips[0].asset_id = "missing".to_string();
        envelope.record.products.push(Product {
            id: "prod1".to_string(),
            project_id: envelope.project.id.clone(),
            name: "Product".to_string(),
            filename: "product.mp4".to_string(),
            mime: "video/mp4".to_string(),
            revision: 1,
            duration_ms: 1000,
            created_at: "2026-09-21T00:00:00Z".to_string(),
            edit_fingerprint: "abc123".to_string(),
            snapshot: Some(Box::new(bad_snapshot)),
            render_range: None,
            extra: crate::editor::Map::new(),
        });
        let err = validate_envelope(&envelope).unwrap_err();
        assert!(err.message.contains("missing"), "message: {}", err.message);
    }

    #[test]
    fn track_name_at_200_chars_is_valid_but_201_is_rejected() {
        let mut ok = base_project();
        ok.tracks[0].name = "n".repeat(200);
        assert!(validate_project(&ok).is_ok(), "200 chars must be accepted");

        let mut too_long = base_project();
        too_long.tracks[0].name = "n".repeat(201);
        let err = validate_project(&too_long).unwrap_err();
        assert!(err.message.contains("t1"), "message: {}", err.message);
    }
}
