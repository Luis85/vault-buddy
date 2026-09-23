//! Shared test fixtures for `core::editor`'s own test modules.
//!
//! Every task from here on hand-built a valid `Project` for its tests
//! (`time.rs`'s `empty_project`, `validate_tests.rs`'s own copy) — a third
//! independent copy of the same defaults (schema id, a supported canvas,
//! `master_gain`, empty collections, an empty `Destination`) was flagged in
//! review as a duplication trend rather than a one-off. This module is the
//! single shared builder new tests reach for instead: start from
//! `minimal_project()` and override only the fields the test cares about.
//!
//! `track`/`asset`/`clip` (Task 8, controller ruling): `clips_tests.rs` and
//! `time.rs`'s own test module each hand-built their own copies of these
//! three entity builders -- the same duplication trend `minimal_project`
//! was already created to stop, just one level down (an entity, not the
//! whole project). Moved here as a pure refactor before this task's own
//! new tests could pile a THIRD independent copy on top; `time.rs`'s old
//! `track(id, visible)`/`asset()`/`clip(id, track_id, start, in, out)`
//! (fixed kind/asset id/duration) are the same shapes as these with fewer
//! parameters, so its lone `visible: false` fixture now overrides
//! `Track.visible` directly on the returned value (every field here is
//! `pub`) rather than needing a second builder signature.
//!
//! `#[cfg(test)] pub(crate)`: this exists only for `core`'s own test builds
//! and is never part of the crate's public API.

use std::collections::BTreeSet;

use super::commands::CommandContext;
use super::model::{
    Asset, AssetKind, Canvas, Clip, Destination, FadeCurve, Project, Track, TrackKind,
};
use super::model_cues::{Effect, EffectKind};
use super::{Map, Num, PROJECT_SCHEMA};

fn num(v: i64) -> Num {
    Num::from(v)
}

/// A track with sensible defaults (`visible: true`, `muted: false`,
/// `solo: false`, `volume: 1`) -- override `.locked` via the `locked`
/// param (the one field enough tests vary to be worth a parameter) or any
/// other field directly on the returned `Track` (every field is `pub`).
pub(crate) fn track(id: &str, kind: TrackKind, locked: bool) -> Track {
    Track {
        id: id.to_string(),
        kind,
        name: id.to_string(),
        visible: true,
        locked,
        muted: false,
        solo: false,
        volume: num(1),
        extra: Map::new(),
    }
}

/// An asset with sensible defaults (name `"Asset <id>"`, no width/height/
/// size/builtin/media_type/linked_asset/original_name).
pub(crate) fn asset(id: &str, kind: AssetKind, duration_ms: u64) -> Asset {
    Asset {
        id: id.to_string(),
        kind,
        name: format!("Asset {id}"),
        duration_ms,
        width: None,
        height: None,
        size: None,
        builtin: None,
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    }
}

/// A clip with sensible defaults (fades 0, curve `linear`, opacity/volume
/// 1, `x 0, y 0, w 1, h 1`, speed unset -- i.e. the implicit 1.0x, no
/// group/layout/crop/adjustments/card).
pub(crate) fn clip(
    id: &str,
    track_id: &str,
    asset_id: &str,
    start_ms: u64,
    in_ms: u64,
    out_ms: u64,
) -> Clip {
    Clip {
        id: id.to_string(),
        asset_id: asset_id.to_string(),
        track_id: track_id.to_string(),
        name: id.to_string(),
        start_ms,
        in_ms,
        out_ms,
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
    }
}

/// A highlight effect on `clip_id` over the SOURCE interval `[start_ms,
/// end_ms)` (Task 31: the shared cue builder; `clips_tests.rs`,
/// `groups_tests.rs` and `tracks_tests.rs` still carry their own older
/// local copies of this same shape).
pub(crate) fn effect(id: &str, clip_id: &str, start_ms: u64, end_ms: u64) -> Effect {
    Effect {
        id: id.to_string(),
        clip_id: clip_id.to_string(),
        kind: EffectKind::Highlight,
        start_ms,
        end_ms,
        x: num(0),
        y: num(0),
        color: "#ffffff".to_string(),
        text: None,
        w: None,
        h: None,
        x2: None,
        y2: None,
        factor: None,
        font_size: None,
        stroke: None,
        dim: None,
        easing: None,
        number: None,
        background: None,
        extra: Map::new(),
    }
}

/// No asset has audio: the context for every command that never consults
/// `sources.json` facts (Task 27) -- a `static` so the borrow is `'static`.
static NO_AUDIO: BTreeSet<String> = BTreeSet::new();

pub(crate) fn no_context() -> CommandContext<'static> {
    CommandContext {
        assets_with_audio: &NO_AUDIO,
    }
}

/// A minimal `Project` that already passes `validate_project` (a supported
/// canvas, an in-range `master_gain`, every collection empty). Callers
/// override whichever fields their test is actually about — id, title,
/// canvas, assets/tracks/clips, destination — rather than restating every
/// default.
pub(crate) fn minimal_project() -> Project {
    Project {
        schema: PROJECT_SCHEMA.to_string(),
        id: "project".to_string(),
        title: "Project".to_string(),
        canvas: Canvas {
            width: 1280,
            height: 720,
            fps: 30,
            extra: Map::new(),
        },
        master_gain: 1.0,
        assets: Vec::new(),
        tracks: Vec::new(),
        clips: Vec::new(),
        effects: Vec::new(),
        markers: Vec::new(),
        transitions: Vec::new(),
        captions: None,
        destination: Destination {
            vault: String::new(),
            folder: String::new(),
            dated: false,
            extra: Map::new(),
        },
        extra: Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::validate_project;

    #[test]
    fn minimal_project_is_valid() {
        assert!(
            validate_project(&minimal_project()).is_ok(),
            "the shared minimal project fixture must itself be schema-valid"
        );
    }
}
