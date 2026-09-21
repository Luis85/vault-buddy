//! Payload structs for every `EditorCommand`/`InternalCommand` variant
//! (Contract reference `Commands` paragraph, `global-constraints.md`) --
//! split out of `commands/mod.rs` from the start (F31) so the dispatch/enum
//! file does not grow toward the 800-line Rust cap as later tasks add arms.
//!
//! Every `EditorCommand` payload struct is `#[serde(rename_all =
//! "camelCase")]`; the wire `kind` tag itself lives on `EditorCommand`, not
//! here. `InternalCommand` payloads never cross IPC (`InternalCommand` is
//! not `Deserialize`), so they reuse the model's own entity types directly
//! (`Asset`, `Project`) rather than inventing a second shape for the same
//! data.
//!
//! `ClipboardFragment` and `EffectProps` are two of the structs the wire
//! needs before their owning tasks land (F9: Task 8's clipboard behaviour,
//! Task 34's per-kind effect defaults). Their STRUCT NAMES and enum tags
//! are FIXED here and never renamed; their FIELD SETS may still grow as
//! later tasks need them (the brief's rule) -- editing one of the per-kind
//! effect structs below to add a field is a normal future evolution, not a
//! reopening of the decision recorded here. The per-kind effect structs
//! (not `ClipboardFragment`) ARE individually `#[serde(deny_unknown_
//! fields)]` -- controller ruling, Task 6 fix round 1 -- which is a
//! DIFFERENT guarantee than "this shape is closed forever": it rejects a
//! field that belongs to a DIFFERENT kind (e.g. `fontSize`, a `text`-only
//! field, on an `arrow` effect) as a decode error today, not a silently
//! swallowed typo.
//!
//! **Decision, recorded per controller ruling (Task 6 fix round 1)**:
//! `EffectProps` is the seven-variant union the original brief named
//! (`Text`/`Arrow`/`Highlight`/`Spotlight`/`Zoom`/`Step`/`Mask`), one
//! variant per `EffectKind`, not the flat 15-field struct Task 6 shipped
//! first. `addEffect`'s wire shape is `{clipId, kind, startMs, endMs,
//! props}` with `kind` (wire name `effectKind`) a SIBLING of `props`; a
//! plain `#[derive(Deserialize)]` cannot make `props`'s shape depend on a
//! sibling field's value, so `AddEffectPayload` gets a hand-written
//! `Deserialize` (below) that reads `effectKind` first and decodes `props`
//! into the matching variant via `EffectProps::from_kind_and_value`.
//! `EffectProps` itself derives `Serialize` only, as `#[serde(untagged)]`
//! (a variant serializes as exactly its own flat fields, no second tag) --
//! see `EffectProps`'s own doc for why it does NOT derive `Deserialize`.

use serde::{Deserialize, Serialize};

use crate::editor::model::{
    Adjustments, Asset, CardPreset, Clip, FadeCurve, Fit, FrameShape, Rotation, TrackKind,
};
use crate::editor::model_cues::{
    CaptionCue, CaptionPosition, Effect, EffectKind, Marker, TransitionKind,
};
use crate::editor::{Num, Project};

// ---- rename / destination ------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePayload {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDestinationPayload {
    pub vault_id: String,
    pub folder: String,
    pub dated: bool,
}

// ---- clips ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertClipPayload {
    pub asset_id: String,
    pub track_id: String,
    pub start_ms: u64,
    pub in_ms: u64,
    pub out_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClipPayload {
    pub clip_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitClipPayload {
    pub clip_id: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimClipPayload {
    pub clip_id: String,
    pub start_ms: u64,
    pub in_ms: u64,
    pub out_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteClipsPayload {
    pub clip_ids: Vec<String>,
    pub close_gap: bool,
}

/// `deltaMs` is signed (decision, recorded): moving a clip earlier on its
/// track is a negative delta, and "all times integer ms" in the Contract
/// reference does not itself rule that out for a DELTA (as opposed to an
/// absolute timestamp) -- every other `*Ms` field in this file names an
/// absolute or duration value and stays `u64`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveClipsPayload {
    pub clip_ids: Vec<String>,
    pub delta_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReorderDirection {
    Earlier,
    Later,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderClipPayload {
    pub clip_id: String,
    pub direction: ReorderDirection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupClipsPayload {
    pub clip_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UngroupClipsPayload {
    pub group_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateClipsPayload {
    pub clip_ids: Vec<String>,
    pub offset_ms: u64,
}

/// `pasteFragment{fragment, trackId, atMs}`'s cross-copied contents (F9,
/// Task 8's own Rust behaviour). Every entity type here is the project
/// model's own -- a fragment is a snippet of real clips/effects/captions/
/// markers awaiting a new home, not a second parallel shape for them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardFragment {
    pub clips: Vec<Clip>,
    pub effects: Vec<Effect>,
    pub captions: Vec<CaptionCue>,
    pub markers: Vec<Marker>,
    pub origin_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteFragmentPayload {
    pub fragment: ClipboardFragment,
    pub track_id: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutClipsPayload {
    pub clip_ids: Vec<String>,
    pub close_gap: bool,
}

// ---- tracks -----------------------------------------------------------

/// **Decision, recorded (not in the brief -- discovered by the
/// `unimplemented_kinds_are_invalid_request_not_panic` table test):**
/// `EditorCommand`'s wire tag is internally-tagged `"kind"`
/// (`#[serde(tag = "kind")]`, merged into the SAME JSON object as this
/// payload's own fields). The Contract reference spells this command's own
/// track-kind field `kind` too -- e.g. `addTrack{kind, name, index}` --
/// which cannot coexist with the enum's own `"kind"` tag at that same
/// level: a JS/TS object literal cannot carry two properties both named
/// `kind` (the second write silently wins), and on the Rust side
/// `serde_json::to_value` was observed doing exactly that -- the resulting
/// object's `"kind"` held `"video"` (this field's value), not `"addTrack"`
/// (the command's own tag), corrupting the very value a decoder needs to
/// pick this variant at all. The wire field is renamed `trackKind` to
/// resolve the collision; the Rust field itself stays `kind` since nothing
/// about the *Rust* name conflicts. Flagged for controller review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTrackPayload {
    #[serde(rename = "trackKind")]
    pub kind: TrackKind,
    pub name: String,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameTrackPayload {
    pub track_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveTrackPayload {
    pub track_id: String,
    pub to_index: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTrackFlagsPayload {
    pub track_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solo: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Num>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTrackPayload {
    pub track_id: String,
}

// ---- mix --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetClipMixPayload {
    pub clip_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
}

/// `gain: f64`, matching `Project.master_gain`'s own type (`model.rs`) --
/// the one project-graph numeric field that is a plain `f64` rather than
/// `Num`, so this payload mirrors it exactly instead of introducing a
/// `Num`-vs-`f64` conversion this command would otherwise have to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMasterGainPayload {
    pub gain: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachAudioPayload {
    pub clip_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_track_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFadesPayload {
    pub clip_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_curve: Option<FadeCurve>,
}

// ---- transitions --------------------------------------------------------

/// Wire field renamed `transitionKind` -- see `AddTrackPayload`'s doc for
/// why a payload's own `kind` field cannot share the JSON level with
/// `EditorCommand`'s internally-tagged `"kind"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTransitionPayload {
    pub from_clip_id: String,
    pub to_clip_id: String,
    pub duration_ms: u64,
    #[serde(rename = "transitionKind")]
    pub kind: TransitionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTransitionDurationPayload {
    pub transition_id: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveTransitionPayload {
    pub transition_id: String,
}

// ---- speed / layout / adjustments / canvas -------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSpeedPayload {
    pub clip_id: String,
    pub speed: Num,
    pub preserve_pitch: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLayoutPayload {
    pub clip_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<Fit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_shape: Option<FrameShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_y: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_zoom: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_y: Option<Num>,
}

/// `adjustments: Adjustments|null` -- the whole object is nullable (`null`
/// clears it), but once present every one of `Adjustments`' five fields is
/// required, which is exactly what reusing `model::Adjustments` already
/// enforces without a second copy of that rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAdjustmentsPayload {
    pub clip_ids: Vec<String>,
    pub adjustments: Option<Adjustments>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCanvasPayload {
    pub width: u32,
    pub height: u32,
}

// ---- cards ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCardPayload {
    pub preset: CardPreset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCardPayload {
    pub clip_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertIntroPayload {
    pub duration_ms: u64,
    pub title: String,
    pub subtitle: String,
}

// ---- effects ------------------------------------------------------------

/// One effect kind's own field set (Task 34's Behavior section, per the
/// cross-task facts this task's brief carried). Every field is `Option`:
/// Task 34 applies per-kind defaults for whatever a caller omits, so
/// nothing here is required at the wire layer -- but `deny_unknown_fields`
/// still rejects a field that belongs to a DIFFERENT kind (see the module
/// doc).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
}

/// See `TextEffectProps`'s doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArrowEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x2: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y2: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Num>,
}

/// See `TextEffectProps`'s doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HighlightEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Num>,
}

/// See `TextEffectProps`'s doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpotlightEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<Num>,
}

/// See `TextEffectProps`'s doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ZoomEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub easing: Option<Num>,
}

/// See `TextEffectProps`'s doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// See `TextEffectProps`'s doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaskEffectProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// The seven-kind union (see the module doc for the representation
/// decision). `#[serde(untagged)]` serializes a variant as exactly its own
/// struct's fields -- no second tag inside `props` -- which is what makes
/// `AddEffectPayload`'s `{effectKind, ..., props: {..flat fields..}}`
/// shape round-trip.
///
/// Deliberately NOT `Deserialize`: an untagged enum decodes by trying each
/// variant in declaration order until one parses, and since EVERY field on
/// EVERY variant here is `Option`, that guess is hopelessly ambiguous --
/// `{"x": 1, "y": 2}` would parse as `Text` (the first variant) no matter
/// which kind the caller actually meant. Decoding instead goes through
/// `from_kind_and_value`, driven by the SIBLING `effectKind` field on the
/// command that carries this (`AddEffectPayload`'s hand-written
/// `Deserialize`, below) or by the target effect's existing kind
/// (`UpdateEffectPayload`'s doc).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EffectProps {
    Text(TextEffectProps),
    Arrow(ArrowEffectProps),
    Highlight(HighlightEffectProps),
    Spotlight(SpotlightEffectProps),
    Zoom(ZoomEffectProps),
    Step(StepEffectProps),
    Mask(MaskEffectProps),
}

impl EffectProps {
    /// Decodes `value` into the variant matching `kind`, rejecting any
    /// field that kind's struct does not declare (each struct's own
    /// `deny_unknown_fields`) -- e.g. `fontSize` (a `text`-only field) on
    /// an `arrow`'s `props` is a decode error, not a silently-dropped
    /// typo.
    pub fn from_kind_and_value(
        kind: EffectKind,
        value: serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        Ok(match kind {
            EffectKind::Text => EffectProps::Text(serde_json::from_value(value)?),
            EffectKind::Arrow => EffectProps::Arrow(serde_json::from_value(value)?),
            EffectKind::Highlight => EffectProps::Highlight(serde_json::from_value(value)?),
            EffectKind::Spotlight => EffectProps::Spotlight(serde_json::from_value(value)?),
            EffectKind::Zoom => EffectProps::Zoom(serde_json::from_value(value)?),
            EffectKind::Step => EffectProps::Step(serde_json::from_value(value)?),
            EffectKind::Mask => EffectProps::Mask(serde_json::from_value(value)?),
        })
    }
}

/// `addEffect{clipId, kind, startMs, endMs, props}`. `start_ms`/`end_ms`
/// are SOURCE time (global-constraints `Commands` paragraph: "Effect/
/// caption startMs/endMs and marker sourceMs are SOURCE time"), unlike
/// every clip-placement command's output-time `startMs`. Wire field
/// renamed `effectKind` -- see `AddTrackPayload`'s doc for why a payload's
/// own `kind` field cannot share the JSON level with `EditorCommand`'s
/// internally-tagged `"kind"`.
///
/// `Deserialize` is hand-written below, not derived: `props` must be
/// decoded according to the SIBLING `effectKind` field, which a plain
/// `#[derive(Deserialize)]` cannot express (one field's shape cannot
/// depend on another field's value in a derived impl). `Serialize` stays
/// derived -- `EffectProps`'s own `#[serde(untagged)]` already produces
/// the flat `props` object this needs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddEffectPayload {
    pub clip_id: String,
    #[serde(rename = "effectKind")]
    pub kind: EffectKind,
    pub start_ms: u64,
    pub end_ms: u64,
    pub props: EffectProps,
}

impl<'de> Deserialize<'de> for AddEffectPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            clip_id: String,
            #[serde(rename = "effectKind")]
            kind: EffectKind,
            start_ms: u64,
            end_ms: u64,
            props: serde_json::Value,
        }
        let raw = Raw::deserialize(deserializer)?;
        let props = EffectProps::from_kind_and_value(raw.kind, raw.props)
            .map_err(serde::de::Error::custom)?;
        Ok(AddEffectPayload {
            clip_id: raw.clip_id,
            kind: raw.kind,
            start_ms: raw.start_ms,
            end_ms: raw.end_ms,
            props,
        })
    }
}

/// `updateEffect{effectId, startMs?, endMs?, props?}`. `props` stays a raw
/// `serde_json::Value` here rather than `EffectProps` -- an update carries
/// no sibling `effectKind` (an effect's kind cannot change once it
/// exists), so there is nothing for a decoder to dispatch on at THIS
/// layer. Task 34, which has the live `Project` (and therefore the target
/// effect's EXISTING kind) in hand at apply time, is the one that calls
/// `EffectProps::from_kind_and_value` against it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEffectPayload {
    pub effect_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEffectPayload {
    pub effect_id: String,
}

// ---- captions -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCaptionSettingsPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burn_in: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<CaptionPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
}

/// `addCaption{clipId, startMs, endMs, text}` -- SOURCE time, see
/// `AddEffectPayload`'s note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCaptionPayload {
    pub clip_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCaptionPayload {
    pub caption_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitCaptionPayload {
    pub caption_id: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveCaptionsPayload {
    pub caption_ids: Vec<String>,
}

// ---- markers ------------------------------------------------------------

/// `addMarker{clipId, sourceMs, title}` -- SOURCE time, see
/// `AddEffectPayload`'s note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMarkerPayload {
    pub clip_id: String,
    pub source_ms: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMarkerPayload {
    pub marker_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveMarkerPayload {
    pub marker_id: String,
}

// ---- InternalCommand payloads (never Deserialize'd from IPC) ------------

/// `AddAssets{assets}`: fully-formed `Asset` entities to append to the
/// project (this task's own arm, `commands::meta::add_assets`) -- a later
/// import task builds these from a probed file; this layer only validates
/// id uniqueness and the `limits::MAX_ASSETS` count.
#[derive(Debug, Clone, PartialEq)]
pub struct AddAssetsPayload {
    pub assets: Vec<Asset>,
}

/// One parsed subtitle cue (`captions_io`'s later job), source time,
/// awaiting a freshly-generated `CaptionCue` id at apply time -- distinct
/// from `CaptionCue` itself, which already carries an id and a `clip_id`
/// that a freshly-imported cue does not have yet.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedCaptionCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportCaptionsPayload {
    pub clip_id: String,
    pub cues: Vec<ImportedCaptionCue>,
    pub replace: bool,
}

/// `RestoreSnapshot{productId, project}`: reinstalls a rendered product's
/// frozen `Project` snapshot as the live project (a later render/product
/// task is the one that calls this).
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreSnapshotPayload {
    pub product_id: String,
    pub project: Project,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelinkAssetsPayload {
    pub asset_ids: Vec<String>,
}

// Tests live in the sibling `payloads_tests.rs` (not inline) so this file
// stays well under the 800-nonblank-line Rust cap (the `validate.rs`/
// `validate_tests.rs` precedent).
#[cfg(test)]
#[path = "payloads_tests.rs"]
mod tests;
