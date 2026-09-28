//! The project graph: canvas, assets, tracks, clips and their layout/mix
//! properties. Field names are the interchange document's own snake_case
//! spelling (R3) — the IPC envelope wrapping this graph is camelCase, but
//! the graph itself travels in document spelling so a browser-reference
//! project opens natively and vice versa (`DATA-MODEL.md` § Entities).

use super::model_cues::{CaptionSettings, Effect, Marker, Transition};
use super::{Map, Num};
use serde::{Deserialize, Serialize};

/// The non-destructive, render-affecting composition (`DATA-MODEL.md` §
/// Entities). No `deny_unknown_fields`: an unrecognized top-level key
/// lands in `extra` and round-trips (R3) instead of being rejected or
/// silently dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub canvas: Canvas,
    pub master_gain: f64,
    pub assets: Vec<Asset>,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub effects: Vec<Effect>,
    pub markers: Vec<Marker>,
    pub transitions: Vec<Transition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captions: Option<CaptionSettings>,
    pub destination: Destination,
    #[serde(flatten)]
    pub extra: Map,
}

/// The output frame: one of `limits::CANVASES` at `limits::CANVAS_FPS`
/// (not enforced here — that is a later validation task's job).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Video,
    Audio,
}

/// A procedurally-supplied builtin asset the reference tutorial project
/// composes (a presenter take, the screen recording, synthesized cues…).
/// Distinct from a user-imported original, which carries no `builtin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Builtin {
    Presenter,
    Screen,
    Detail,
    Cues,
    Ambient,
    Card,
}

/// The one media-kind refinement the reference format distinguishes: a
/// video-kind asset backed by a still image rather than a frame clock
/// (`DATA-MODEL.md`: "Never assume all visual assets have video frame
/// clocks").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Image,
}

/// An original immutable source or procedural source
/// (`DATA-MODEL.md` § Entities). `linked_asset` supports detached audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub kind: AssetKind,
    pub name: String,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<Builtin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_asset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Video,
}

/// An ordered composition layer or audio lane (`DATA-MODEL.md` §
/// Entities). Visibility and audio mute are separate controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub kind: TrackKind,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub muted: bool,
    pub solo: bool,
    pub volume: Num,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FadeCurve {
    Linear,
    Smooth,
    EqualPower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameShape {
    Circle,
    Rounded,
    Rectangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Cover,
    Contain,
}

/// A clip's rotation, one of the four quarter turns. Serializes as the
/// plain integer degrees (`0`/`90`/`180`/`270`) the reference document
/// uses, via `TryFrom<u16>`/`Into<u16>` rather than a string enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum Rotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl TryFrom<u16> for Rotation {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Rotation::Deg0),
            90 => Ok(Rotation::Deg90),
            180 => Ok(Rotation::Deg180),
            270 => Ok(Rotation::Deg270),
            other => Err(format!(
                "rotation must be a quarter turn (0, 90, 180 or 270), got {other}"
            )),
        }
    }
}

impl From<Rotation> for u16 {
    fn from(value: Rotation) -> Self {
        match value {
            Rotation::Deg0 => 0,
            Rotation::Deg90 => 90,
            Rotation::Deg180 => 180,
            Rotation::Deg270 => 270,
        }
    }
}

/// Brightness/contrast/saturation/sepia/grayscale — the reference format
/// requires every field once the object is present at all (`DATA-MODEL.md`
/// § Layout and compositing properties); semantic range validation is a
/// later task's job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Adjustments {
    pub brightness: Num,
    pub contrast: Num,
    pub saturation: Num,
    pub sepia: Num,
    pub grayscale: Num,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardPreset {
    Intro,
    Chapter,
    Outro,
    Blank,
}

/// A titles/chapter card's editable content — an ordinary generated clip
/// with card-specific fields (`DATA-MODEL.md`: "editable generated card
/// clips with preset/title/subtitle/background/foreground/accent
/// properties"). Colors are plain `#rrggbb` strings here; format
/// validation is a later task's job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub preset: CardPreset,
    pub title: String,
    pub subtitle: String,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// An instance of a source range at an output time (`DATA-MODEL.md` §
/// Entities, § Timing rules). The half-open source interval is
/// `[in_ms, out_ms)`, starting at `start_ms` on the output timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub asset_id: String,
    pub track_id: String,
    pub name: String,
    pub start_ms: u64,
    pub in_ms: u64,
    pub out_ms: u64,
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    pub fade_curve: FadeCurve,
    pub opacity: Num,
    pub volume: Num,
    pub muted: bool,
    pub x: Num,
    pub y: Num,
    pub w: Num,
    pub h: Num,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_shape: Option<FrameShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<Fit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_y: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_pitch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_zoom: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adjustments: Option<Adjustments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
    #[serde(flatten)]
    pub extra: Map,
}

/// Where "Save into a vault" publishes (`DATA-MODEL.md`; the tenth
/// sanctioned vault write, a later task). `vault` holds the Obsidian vault
/// ID, never a display name (R3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Destination {
    pub vault: String,
    pub folder: String,
    pub dated: bool,
    #[serde(flatten)]
    pub extra: Map,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_accepts_only_quarter_turns() {
        assert_eq!(
            serde_json::from_value::<Rotation>(serde_json::json!(90)).unwrap(),
            Rotation::Deg90,
        );
        assert!(serde_json::from_value::<Rotation>(serde_json::json!(45)).is_err());
    }
}
