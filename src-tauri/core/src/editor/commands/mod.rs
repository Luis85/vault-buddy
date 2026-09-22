//! `EditorCommand` (the IPC command envelope's `kind`-tagged union) and
//! `InternalCommand` (native-only, never `Deserialize`), plus their
//! dispatch (`apply`/`apply_internal`) into family modules (R14, F-13, F31).
//!
//! SIXTEEN kinds are implemented so far: `rename`/`setDestination` (Task 6,
//! `meta.rs`), the two `EditorSession` intercepts before ever calling
//! `apply` at all, `undo`/`redo` (see `session.rs`'s `execute`), the seven
//! core clip commands `insertClip`/`updateClip`/`splitClip`/`trimClip`/
//! `deleteClips`/`moveClips`/`reorderClip` (Task 7, `clips.rs`; cue
//! reassignment for `splitClip` lives in the sibling `cue_follow.rs`), and
//! `groupClips`/`ungroupClips`/`duplicateClips`/`pasteFragment`/`cutClips`
//! (Task 8, `groups.rs`). `moveClips`'s own group-EXPANSION behaviour also
//! landed this task, but stays in `clips.rs` (F13: Task 7 shipped
//! `moveClips` before any group could exist to expand into). Every other
//! kind falls through to the shared "not available yet" arm below -- each
//! later task adds its own explicit arm ABOVE the fallback and deletes
//! that kind's row from `unimplemented_kinds_are_invalid_request_not_panic`'s
//! table (this module's own tests), so the table shrinks monotonically
//! task by task; say so explicitly in that task's own report rather than
//! re-verifying the whole table at the end.

mod clips;
mod cue_follow;
mod groups;
mod meta;
pub mod payloads;

use serde::{Deserialize, Serialize};

use crate::editor::error::{EditorError, EditorErrorCode};
use crate::editor::model::Project;
use payloads::*;

/// The wire command envelope (Contract reference `Commands` paragraph).
/// `#[serde(tag = "kind", rename_all = "camelCase")]` merges each payload
/// struct's own fields alongside the tag at the SAME JSON level -- e.g.
/// `{"kind":"splitClip","clipId":"c","atMs":1200}` -- rather than nesting
/// them under a `"payload"` key. Variant names are `PascalCase` in Rust and
/// become the exact lower-`camelCase` kind strings the Contract reference
/// lists (`rename_all` on an internally-tagged enum lower-cases only the
/// leading character, so `SplitClip` -> `splitClip`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditorCommand {
    Rename(RenamePayload),
    Undo,
    Redo,
    InsertClip(InsertClipPayload),
    UpdateClip(UpdateClipPayload),
    SplitClip(SplitClipPayload),
    TrimClip(TrimClipPayload),
    DeleteClips(DeleteClipsPayload),
    MoveClips(MoveClipsPayload),
    ReorderClip(ReorderClipPayload),
    GroupClips(GroupClipsPayload),
    UngroupClips(UngroupClipsPayload),
    DuplicateClips(DuplicateClipsPayload),
    PasteFragment(PasteFragmentPayload),
    CutClips(CutClipsPayload),
    AddTrack(AddTrackPayload),
    RenameTrack(RenameTrackPayload),
    MoveTrack(MoveTrackPayload),
    SetTrackFlags(SetTrackFlagsPayload),
    DeleteTrack(DeleteTrackPayload),
    SetClipMix(SetClipMixPayload),
    SetMasterGain(SetMasterGainPayload),
    DetachAudio(DetachAudioPayload),
    SetFades(SetFadesPayload),
    AddTransition(AddTransitionPayload),
    SetTransitionDuration(SetTransitionDurationPayload),
    RemoveTransition(RemoveTransitionPayload),
    SetSpeed(SetSpeedPayload),
    SetLayout(SetLayoutPayload),
    SetAdjustments(SetAdjustmentsPayload),
    SetCanvas(SetCanvasPayload),
    AddCard(AddCardPayload),
    UpdateCard(UpdateCardPayload),
    InsertIntro(InsertIntroPayload),
    AddEffect(AddEffectPayload),
    UpdateEffect(UpdateEffectPayload),
    RemoveEffect(RemoveEffectPayload),
    SetCaptionSettings(SetCaptionSettingsPayload),
    AddCaption(AddCaptionPayload),
    UpdateCaption(UpdateCaptionPayload),
    SplitCaption(SplitCaptionPayload),
    RemoveCaptions(RemoveCaptionsPayload),
    AddMarker(AddMarkerPayload),
    UpdateMarker(UpdateMarkerPayload),
    RemoveMarker(RemoveMarkerPayload),
    SetDestination(SetDestinationPayload),
}

/// Native-only commands a Rust caller issues directly (never `Deserialize`
/// -- they never cross IPC, so there is no wire shape to pin and no `kind`
/// tag to give them).
#[derive(Debug, Clone, PartialEq)]
pub enum InternalCommand {
    AddAssets(AddAssetsPayload),
    ImportCaptions(ImportCaptionsPayload),
    // Boxed: `RestoreSnapshotPayload` embeds a whole `Project`, which would
    // otherwise make every `InternalCommand` value as large as the biggest
    // variant (clippy::large_enum_variant) even when it is a small one like
    // `AddAssets`.
    RestoreSnapshot(Box<RestoreSnapshotPayload>),
    RelinkAssets(RelinkAssetsPayload),
}

fn not_yet(kind: &str) -> EditorError {
    EditorError::new(
        EditorErrorCode::InvalidRequest,
        format!("{kind} is not available yet"),
    )
}

/// The wire `kind` tag string for `cmd`, read back off its own `Serialize`
/// impl rather than duplicated in a second match arm-by-arm -- the two can
/// never drift apart this way (a renamed variant renames its own message
/// for free).
fn kind_of(cmd: &EditorCommand) -> String {
    serde_json::to_value(cmd)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_string)))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Applies one `EditorCommand` to `project`, returning a candidate project
/// plus a human-readable undo label. This function NEVER installs the
/// candidate itself -- `EditorSession::execute` runs `validate_project` on
/// it first and only then replaces `project`, so a command whose result
/// would be schema-invalid never reaches a live session (see that module's
/// doc for why `meta::rename` deliberately does not duplicate
/// `validate_project`'s own title-length check).
pub fn apply(project: &Project, cmd: &EditorCommand) -> Result<(Project, String), EditorError> {
    match cmd {
        EditorCommand::Rename(p) => meta::rename(project, p),
        EditorCommand::SetDestination(p) => meta::set_destination(project, p),
        EditorCommand::InsertClip(p) => clips::insert_clip(project, p),
        EditorCommand::UpdateClip(p) => clips::update_clip(project, p),
        EditorCommand::SplitClip(p) => clips::split_clip(project, p),
        EditorCommand::TrimClip(p) => clips::trim_clip(project, p),
        EditorCommand::DeleteClips(p) => clips::delete_clips(project, p),
        EditorCommand::MoveClips(p) => clips::move_clips(project, p),
        EditorCommand::ReorderClip(p) => clips::reorder_clip(project, p),
        EditorCommand::GroupClips(p) => groups::group_clips(project, p),
        EditorCommand::UngroupClips(p) => groups::ungroup_clips(project, p),
        EditorCommand::DuplicateClips(p) => groups::duplicate_clips(project, p),
        EditorCommand::PasteFragment(p) => groups::paste_fragment(project, p),
        EditorCommand::CutClips(p) => groups::cut_clips(project, p),
        EditorCommand::Undo | EditorCommand::Redo => Err(EditorError::new(
            EditorErrorCode::InvalidRequest,
            "undo/redo are dispatched by EditorSession::execute, never by apply",
        )),
        other => Err(not_yet(&kind_of(other))),
    }
}

/// The `InternalCommand` counterpart of `apply` -- same candidate-then-
/// caller-validates contract. `InternalCommand` is not `Deserialize`, so
/// there is no wire tag to read back; each arm names its own kind
/// literally instead.
pub fn apply_internal(
    project: &Project,
    cmd: &InternalCommand,
) -> Result<(Project, String), EditorError> {
    match cmd {
        InternalCommand::AddAssets(p) => meta::add_assets(project, p),
        InternalCommand::ImportCaptions(_) => Err(not_yet("importCaptions")),
        InternalCommand::RestoreSnapshot(_) => Err(not_yet("restoreSnapshot")),
        InternalCommand::RelinkAssets(_) => Err(not_yet("relinkAssets")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::{CardPreset, TrackKind};
    use crate::editor::model_cues::{EffectKind, TransitionKind};
    use crate::editor::test_support::minimal_project;

    fn num(v: i64) -> crate::editor::Num {
        crate::editor::Num::from(v)
    }

    /// Every `EditorCommand` kind NOT implemented so far (`rename`, `undo`,
    /// `redo`, `setDestination`, and the seven clip commands are -- see the
    /// module doc). Each later task deletes its own row here as it
    /// replaces `apply`'s fallback with a real arm; report that removal
    /// explicitly in that task's own report rather than re-verifying the
    /// whole table at once.
    fn unimplemented_commands() -> Vec<(&'static str, EditorCommand)> {
        vec![
            (
                "addTrack",
                EditorCommand::AddTrack(AddTrackPayload {
                    kind: TrackKind::Video,
                    name: "Track".into(),
                    index: 0,
                }),
            ),
            (
                "renameTrack",
                EditorCommand::RenameTrack(RenameTrackPayload {
                    track_id: "t1".into(),
                    name: "Track".into(),
                }),
            ),
            (
                "moveTrack",
                EditorCommand::MoveTrack(MoveTrackPayload {
                    track_id: "t1".into(),
                    to_index: 1,
                }),
            ),
            (
                "setTrackFlags",
                EditorCommand::SetTrackFlags(SetTrackFlagsPayload {
                    track_id: "t1".into(),
                    visible: Some(true),
                    locked: None,
                    muted: None,
                    solo: None,
                    volume: None,
                }),
            ),
            (
                "deleteTrack",
                EditorCommand::DeleteTrack(DeleteTrackPayload {
                    track_id: "t1".into(),
                }),
            ),
            (
                "setClipMix",
                EditorCommand::SetClipMix(SetClipMixPayload {
                    clip_ids: vec!["c1".into()],
                    volume: Some(num(1)),
                    muted: None,
                }),
            ),
            (
                "setMasterGain",
                EditorCommand::SetMasterGain(SetMasterGainPayload { gain: 0.8 }),
            ),
            (
                "detachAudio",
                EditorCommand::DetachAudio(DetachAudioPayload {
                    clip_id: "c1".into(),
                    audio_track_id: None,
                }),
            ),
            (
                "setFades",
                EditorCommand::SetFades(SetFadesPayload {
                    clip_id: "c1".into(),
                    fade_in_ms: Some(100),
                    fade_out_ms: None,
                    fade_curve: None,
                }),
            ),
            (
                "addTransition",
                EditorCommand::AddTransition(AddTransitionPayload {
                    from_clip_id: "c1".into(),
                    to_clip_id: "c2".into(),
                    duration_ms: 500,
                    kind: TransitionKind::Dissolve,
                }),
            ),
            (
                "setTransitionDuration",
                EditorCommand::SetTransitionDuration(SetTransitionDurationPayload {
                    transition_id: "tr1".into(),
                    duration_ms: 500,
                }),
            ),
            (
                "removeTransition",
                EditorCommand::RemoveTransition(RemoveTransitionPayload {
                    transition_id: "tr1".into(),
                }),
            ),
            (
                "setSpeed",
                EditorCommand::SetSpeed(SetSpeedPayload {
                    clip_id: "c1".into(),
                    speed: num(1),
                    preserve_pitch: true,
                }),
            ),
            (
                "setLayout",
                EditorCommand::SetLayout(SetLayoutPayload {
                    clip_ids: vec!["c1".into()],
                    x: Some(num(0)),
                    y: None,
                    w: None,
                    h: None,
                    opacity: None,
                    fit: None,
                    frame_shape: None,
                    rotation: None,
                    mirror: None,
                    flip_y: None,
                    crop_zoom: None,
                    crop_x: None,
                    crop_y: None,
                }),
            ),
            (
                "setAdjustments",
                EditorCommand::SetAdjustments(SetAdjustmentsPayload {
                    clip_ids: vec!["c1".into()],
                    adjustments: None,
                }),
            ),
            (
                "setCanvas",
                EditorCommand::SetCanvas(SetCanvasPayload {
                    width: 1280,
                    height: 720,
                }),
            ),
            (
                "addCard",
                EditorCommand::AddCard(AddCardPayload {
                    preset: CardPreset::Intro,
                    track_id: None,
                    start_ms: 0,
                    duration_ms: 1_000,
                    title: "Title".into(),
                    subtitle: "Subtitle".into(),
                }),
            ),
            (
                "updateCard",
                EditorCommand::UpdateCard(UpdateCardPayload {
                    clip_id: "c1".into(),
                    title: Some("T".into()),
                    subtitle: None,
                    background: None,
                    foreground: None,
                    accent: None,
                }),
            ),
            (
                "insertIntro",
                EditorCommand::InsertIntro(InsertIntroPayload {
                    duration_ms: 1_000,
                    title: "Title".into(),
                    subtitle: "Subtitle".into(),
                }),
            ),
            (
                "addEffect",
                EditorCommand::AddEffect(AddEffectPayload {
                    clip_id: "c1".into(),
                    kind: EffectKind::Highlight,
                    start_ms: 0,
                    end_ms: 500,
                    props: EffectProps::Highlight(HighlightEffectProps {
                        x: Some(num(0)),
                        y: Some(num(0)),
                        ..Default::default()
                    }),
                }),
            ),
            (
                "updateEffect",
                EditorCommand::UpdateEffect(UpdateEffectPayload {
                    effect_id: "e1".into(),
                    start_ms: None,
                    end_ms: None,
                    props: None,
                }),
            ),
            (
                "removeEffect",
                EditorCommand::RemoveEffect(RemoveEffectPayload {
                    effect_id: "e1".into(),
                }),
            ),
            (
                "setCaptionSettings",
                EditorCommand::SetCaptionSettings(SetCaptionSettingsPayload {
                    enabled: Some(true),
                    burn_in: None,
                    font_size: None,
                    position: None,
                    background: None,
                }),
            ),
            (
                "addCaption",
                EditorCommand::AddCaption(AddCaptionPayload {
                    clip_id: "c1".into(),
                    start_ms: 0,
                    end_ms: 500,
                    text: "Hi".into(),
                }),
            ),
            (
                "updateCaption",
                EditorCommand::UpdateCaption(UpdateCaptionPayload {
                    caption_id: "cap1".into(),
                    start_ms: None,
                    end_ms: None,
                    text: Some("Hi".into()),
                }),
            ),
            (
                "splitCaption",
                EditorCommand::SplitCaption(SplitCaptionPayload {
                    caption_id: "cap1".into(),
                    at_ms: 250,
                }),
            ),
            (
                "removeCaptions",
                EditorCommand::RemoveCaptions(RemoveCaptionsPayload {
                    caption_ids: vec!["cap1".into()],
                }),
            ),
            (
                "addMarker",
                EditorCommand::AddMarker(AddMarkerPayload {
                    clip_id: "c1".into(),
                    source_ms: 0,
                    title: "M".into(),
                }),
            ),
            (
                "updateMarker",
                EditorCommand::UpdateMarker(UpdateMarkerPayload {
                    marker_id: "m1".into(),
                    source_ms: None,
                    title: Some("M".into()),
                }),
            ),
            (
                "removeMarker",
                EditorCommand::RemoveMarker(RemoveMarkerPayload {
                    marker_id: "m1".into(),
                }),
            ),
        ]
    }

    #[test]
    fn unimplemented_commands_table_has_thirty_rows() {
        // A vacuity guard, the `shared_fixture_table_has_ten_cases`
        // precedent (`time.rs`): 46 total EditorCommand kinds minus the
        // sixteen implemented so far (rename, undo, redo, setDestination,
        // insertClip, updateClip, splitClip, trimClip, deleteClips,
        // moveClips, reorderClip, groupClips, ungroupClips,
        // duplicateClips, pasteFragment, cutClips).
        assert_eq!(unimplemented_commands().len(), 30);
    }

    #[test]
    fn unimplemented_kinds_are_invalid_request_not_panic() {
        let project = minimal_project();
        for (kind, cmd) in unimplemented_commands() {
            let err = apply(&project, &cmd)
                .err()
                .unwrap_or_else(|| panic!("{kind}: apply() must reject an unimplemented kind"));
            assert_eq!(
                err.code,
                EditorErrorCode::InvalidRequest,
                "{kind}: wrong error code"
            );
            assert!(
                err.message.contains(kind) && err.message.contains("is not available yet"),
                "{kind}: message {:?} does not name the kind",
                err.message
            );
        }
    }

    #[test]
    fn apply_rejects_undo_and_redo_directly() {
        // Undo/redo are implemented by `EditorSession::execute`, which
        // intercepts them before ever calling `apply` -- calling `apply`
        // with them directly is a programming error, not an unimplemented
        // kind, so the message must differ from the "not available yet"
        // fallback's.
        let project = minimal_project();
        for cmd in [EditorCommand::Undo, EditorCommand::Redo] {
            let err = apply(&project, &cmd).unwrap_err();
            assert_eq!(err.code, EditorErrorCode::InvalidRequest);
            assert!(!err.message.contains("is not available yet"));
        }
    }

    #[test]
    fn apply_dispatches_rename_and_set_destination() {
        let project = minimal_project();
        let (renamed, label) = apply(
            &project,
            &EditorCommand::Rename(RenamePayload {
                title: "New Title".to_string(),
            }),
        )
        .unwrap();
        assert_eq!(renamed.title, "New Title");
        assert_eq!(label, "Rename");

        let (redirected, label) = apply(
            &project,
            &EditorCommand::SetDestination(SetDestinationPayload {
                vault_id: "vault-1".to_string(),
                folder: "Tutorials".to_string(),
                dated: true,
            }),
        )
        .unwrap();
        assert_eq!(redirected.destination.vault, "vault-1");
        assert_eq!(label, "Set destination");
    }

    #[test]
    fn add_track_transition_and_effect_do_not_lose_the_outer_kind_tag() {
        // Regression: AddTrackPayload/AddTransitionPayload/AddEffectPayload
        // each carry their OWN `kind` field (TrackKind/TransitionKind/
        // EffectKind). `EditorCommand`'s wire tag is ALSO `"kind"`,
        // internally-tagged into the same JSON object -- so before the
        // `#[serde(rename = "...")]` overrides in `payloads.rs`, the
        // payload's own field silently overwrote the command's own tag on
        // serialization (`kind_of` observed `"video"` where `"addTrack"`
        // belonged), which would have corrupted every such command on the
        // real wire, not just this test helper.
        let add_track = EditorCommand::AddTrack(AddTrackPayload {
            kind: TrackKind::Video,
            name: "Track".into(),
            index: 0,
        });
        let value = serde_json::to_value(&add_track).unwrap();
        assert_eq!(value["kind"], "addTrack");
        assert_eq!(value["trackKind"], "video");
        assert_eq!(
            serde_json::from_value::<EditorCommand>(value).unwrap(),
            add_track
        );

        let add_transition = EditorCommand::AddTransition(AddTransitionPayload {
            from_clip_id: "c1".into(),
            to_clip_id: "c2".into(),
            duration_ms: 500,
            kind: TransitionKind::Dissolve,
        });
        let value = serde_json::to_value(&add_transition).unwrap();
        assert_eq!(value["kind"], "addTransition");
        assert_eq!(value["transitionKind"], "dissolve");

        let add_effect = EditorCommand::AddEffect(AddEffectPayload {
            clip_id: "c1".into(),
            kind: EffectKind::Highlight,
            start_ms: 0,
            end_ms: 500,
            props: EffectProps::Highlight(HighlightEffectProps::default()),
        });
        let value = serde_json::to_value(&add_effect).unwrap();
        assert_eq!(value["kind"], "addEffect");
        assert_eq!(value["effectKind"], "highlight");
        assert_eq!(
            serde_json::from_value::<EditorCommand>(value).unwrap(),
            add_effect,
            "addEffect must round-trip through its hand-written Deserialize too"
        );
    }

    #[test]
    fn apply_internal_dispatches_add_assets_and_stubs_the_rest() {
        use crate::editor::model::{Asset, AssetKind};

        let project = minimal_project();
        let asset = Asset {
            id: "a1".to_string(),
            kind: AssetKind::Video,
            name: "a1".to_string(),
            duration_ms: 1_000,
            width: None,
            height: None,
            size: None,
            builtin: None,
            media_type: None,
            linked_asset: None,
            original_name: None,
            extra: crate::editor::Map::new(),
        };
        let (candidate, label) = apply_internal(
            &project,
            &InternalCommand::AddAssets(AddAssetsPayload {
                assets: vec![asset],
            }),
        )
        .unwrap();
        assert_eq!(candidate.assets.len(), 1);
        assert_eq!(label, "Add assets");

        let err = apply_internal(
            &project,
            &InternalCommand::RelinkAssets(RelinkAssetsPayload {
                asset_ids: vec!["a1".to_string()],
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(err.message.contains("is not available yet"));
    }
}
