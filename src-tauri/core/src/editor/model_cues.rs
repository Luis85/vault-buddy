//! Clip-linked teaching cues (effects, captions, chapter markers), pairwise
//! transitions, the rendered-product ledger and the saved workspace
//! envelope that wraps a `Project` (`DATA-MODEL.md` § Entities,
//! § Persistence and source identity).

use super::model::Project;
use super::{Map, Num};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectKind {
    Text,
    Arrow,
    Highlight,
    Spotlight,
    Zoom,
    Step,
    Mask,
}

/// A clip-linked teaching cue (`DATA-MODEL.md` § Entities, § Layout and
/// compositing properties): "Do not flatten them into drawn pixels inside
/// project storage." `start_ms`/`end_ms` are SOURCE time, like every
/// clip-linked cue — trim changes the visible intersection, not these
/// timestamps (§ Timing rules).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub id: String,
    pub clip_id: String,
    pub kind: EffectKind,
    pub start_ms: u64,
    pub end_ms: u64,
    pub x: Num,
    pub y: Num,
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x2: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y2: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "fontSize")]
    pub font_size: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub easing: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptionPosition {
    Top,
    Bottom,
}

/// A caption cue: clip-linked source time; on-screen timing derives from
/// the clip's actual output range/speed (`DATA-MODEL.md` § Entities).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionCue {
    pub id: String,
    pub clip_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// Project-level caption presentation plus the source-linked cue records
/// (`DATA-MODEL.md` § Layout and compositing properties). Caption
/// visibility in preview and inclusion in output are explicit settings
/// here, not derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionSettings {
    pub enabled: bool,
    pub burn_in: bool,
    pub font_size: Num,
    pub position: CaptionPosition,
    pub background: bool,
    pub cues: Vec<CaptionCue>,
    #[serde(flatten)]
    pub extra: Map,
}

/// A chapter marker: clip-linked source time; its output timestamp is
/// derived (`DATA-MODEL.md` § Entities).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub clip_id: String,
    pub source_ms: u64,
    pub title: String,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransitionKind {
    Dissolve,
    EqualPower,
}

/// An explicit relationship between two compatible clips, not a hidden
/// track overlap (`DATA-MODEL.md` § Entities, § Fade and transition
/// rules). The reference model permits one paired transition association
/// per clip; a different overlapping-transition algebra is a later,
/// explicit contract change, never a silent extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub id: String,
    pub from: String,
    pub to: String,
    pub duration_ms: u64,
    pub kind: TransitionKind,
    #[serde(flatten)]
    pub extra: Map,
}

/// A render's requested sub-range of the project's output timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderRange {
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(flatten)]
    pub extra: Map,
}

/// An immutable rendered product: its exact source revision and an
/// independent edit snapshot, encoded binary stored separately
/// (`DATA-MODEL.md` § Entities, § Persistence and source identity). A
/// product's missing encoded binary does not erase its lineage — that is
/// why `snapshot` is retained even when the file is gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub filename: String,
    pub mime: String,
    pub revision: u64,
    pub duration_ms: u64,
    pub created_at: String,
    pub edit_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Box<Project>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_range: Option<RenderRange>,
    #[serde(flatten)]
    pub extra: Map,
}

/// Revision history summary (`DATA-MODEL.md` § Entities): undo still
/// advances the current revision even though it changes the graph back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub products: Vec<Product>,
    #[serde(flatten)]
    pub extra: Map,
}

/// The saved workspace envelope: `{schema, project, workspace, record,
/// saved_at}` (`DATA-MODEL.md` § Persistence and source identity). Only
/// `project` is a typed graph here — `workspace` is deliberately an opaque
/// `serde_json::Value` (view preferences a later task sanitizes
/// separately; they never grant source access on their own, per
/// `workspace.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceEnvelope {
    pub schema: String,
    pub project: Project,
    pub workspace: serde_json::Value,
    pub record: Record,
    pub saved_at: String,
    #[serde(flatten)]
    pub extra: Map,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::TrackKind;

    const REFERENCE_WORKSPACE: &str =
        include_str!("../../../../tests/fixtures/editor/reference-workspace.example.json");

    #[test]
    fn reference_example_round_trips_as_json_value() {
        let original: serde_json::Value = serde_json::from_str(REFERENCE_WORKSPACE).unwrap();
        let envelope: WorkspaceEnvelope = serde_json::from_str(REFERENCE_WORKSPACE).unwrap();
        let round_tripped = serde_json::to_value(&envelope).unwrap();
        assert_eq!(round_tripped, original);
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let mut original: serde_json::Value = serde_json::from_str(REFERENCE_WORKSPACE).unwrap();
        let future = serde_json::json!({"a": 1});
        original["project"]["future_field"] = future.clone();
        original["project"]["clips"][0]["future_field"] = future.clone();
        original["project"]["effects"][0]["future_field"] = future.clone();

        let envelope: WorkspaceEnvelope = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(
            envelope.project.extra.get("future_field"),
            Some(&future),
            "project lost an unknown field"
        );
        assert_eq!(
            envelope.project.clips[0].extra.get("future_field"),
            Some(&future),
            "clip lost an unknown field"
        );
        assert_eq!(
            envelope.project.effects[0].extra.get("future_field"),
            Some(&future),
            "effect lost an unknown field"
        );

        let round_tripped = serde_json::to_value(&envelope).unwrap();
        assert_eq!(round_tripped, original);
    }

    #[test]
    fn unknown_enum_variant_is_rejected() {
        assert!(serde_json::from_value::<EffectKind>(serde_json::json!("blur")).is_err());
        assert!(serde_json::from_value::<TrackKind>(serde_json::json!("midi")).is_err());
    }

    #[test]
    fn font_size_keeps_its_camel_case_spelling() {
        let effect = Effect {
            id: "e1".to_string(),
            clip_id: "c1".to_string(),
            kind: EffectKind::Text,
            start_ms: 0,
            end_ms: 100,
            x: serde_json::Number::from(0),
            y: serde_json::Number::from(0),
            color: "#ffffff".to_string(),
            text: None,
            w: None,
            h: None,
            x2: None,
            y2: None,
            factor: None,
            font_size: Some(serde_json::Number::from(29)),
            stroke: None,
            dim: None,
            easing: None,
            number: None,
            background: None,
            extra: crate::editor::Map::new(),
        };
        let value = serde_json::to_value(&effect).unwrap();
        assert!(value.get("fontSize").is_some(), "missing fontSize: {value}");
        assert!(
            value.get("font_size").is_none(),
            "leaked font_size: {value}"
        );
    }
}
