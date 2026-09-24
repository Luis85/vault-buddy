//! `EditorCommand` (the IPC command envelope's `kind`-tagged union) and
//! `InternalCommand` (native-only, never `Deserialize`), plus their
//! dispatch (`apply`/`apply_internal`) into family modules (R14, F-13, F31).
//!
//! ALL forty-six kinds are implemented (Task 36 landed the last eight):
//! `rename`/`setDestination`
//! (Task 6, `meta.rs`), the two `EditorSession` intercepts before ever
//! calling `apply` at all, `undo`/`redo` (see `session.rs`'s `execute`), the
//! seven core clip commands `insertClip`/`updateClip`/`splitClip`/`trimClip`/
//! `deleteClips`/`moveClips`/`reorderClip` (Task 7, `clips.rs`; cue
//! reassignment for `splitClip` lives in the sibling `cue_follow.rs`),
//! `groupClips`/`ungroupClips`/`duplicateClips`/`pasteFragment`/`cutClips`
//! (Task 8, `groups.rs`), and `addTrack`/`renameTrack`/`moveTrack`/
//! `setTrackFlags`/`deleteTrack` (Task 23, `tracks.rs`), `setClipMix`/
//! `setMasterGain`/`detachAudio` (Task 27, `mix.rs` -- `detachAudio` is the
//! one arm that reads `CommandContext`), and `setFades` (Task 29,
//! `fades.rs` -- also the fade-clamp arm F11 adds to `clips::trim_clip`,
//! which stays in `clips.rs` for the same "extend the owning file" reason
//! `moveClips`' group expansion stayed in `clips.rs` under Task 8), and
//! `addTransition`/`setTransitionDuration`/`removeTransition` (Task 30,
//! `transitions.rs` -- plus the hooks `clips.rs`'s delete/trim/move/split/
//! reorder call so they respect an existing transition), and `setSpeed`/
//! `setLayout` (Task 31, `layout.rs` -- `setSpeed` reuses the fade clamp and
//! both transition hooks, because it changes a clip's output duration the
//! way a trim does), and `setCanvas`/`setAdjustments` (Task 32, `layout.rs`
//! beside their two Task 31 siblings -- `setAdjustments` reuses
//! `check_targets`'s video-track/unlocked discipline for its own
//! `check_color_targets`, adding the title-card refusal), and `addCard`/
//! `updateCard`/`insertIntro` (Task 33, `cards.rs` -- the builtin `card`
//! asset is minted once per project and reused by every later card;
//! `insertIntro` shifts every clip's `start_ms` uniformly, which is why it
//! never has to touch `effects`/`markers`/`captions` (their source times
//! are clip-relative already) or re-derive transition/group geometry (a
//! uniform shift preserves every relative offset by construction) --
//! `transitions::ensure_intact` still runs as the same defensive check
//! `setSpeed` above uses), and `addEffect`/`updateEffect`/`removeEffect`
//! (Task 34, `cues.rs` -- the seven teaching-cue kinds; `start_ms`/`end_ms`
//! are SOURCE time, stored verbatim, which is what makes a cue survive
//! `splitClip`'s already-shipped `cue_follow.rs` reassignment and every
//! other clip command's move/trim/speed change untouched -- Task 7's own
//! claim, exercised here rather than re-implemented), and
//! `setCaptionSettings`/`addCaption`/`updateCaption`/`splitCaption`/
//! `removeCaptions` (Task 36, `captions.rs` -- plus
//! `InternalCommand::ImportCaptions`) and `addMarker`/`updateMarker`/
//! `removeMarker` (Task 36, beside the teaching cues in `cues.rs`, with the
//! derived output-time `chapters` list).
//! `moveClips`'s own group-EXPANSION behaviour also landed with Task 8, but
//! stays in `clips.rs` (F13: Task 7 shipped `moveClips` before any group
//! could exist to expand into). `apply`'s match has NO wildcard arm any
//! more: the "not available yet" fallback went with the last eight kinds,
//! so a NEW `EditorCommand` variant now fails to compile until it gets an
//! arm (the exhaustiveness the old shrinking table only approximated).
//! `apply_internal` lost its own `not_yet` fallback with Task 46's
//! `RestoreSnapshot`, the last native-only kind to land.
//!
//! **The frontend kept its own copy of this table, and nothing enforced
//! they agreed.** `src/editor/actionMeta.ts`'s `UNIMPLEMENTED_KINDS`
//! (re-exported from `src/editor/actions.ts`, Task 17) was a hand-copy of
//! the SAME kind strings the old `unimplemented_commands()` table listed --
//! both are EMPTY since Task 36 (the frontend set stays, pinned at size 0,
//! so a future gated kind has somewhere to go). It gated
//! every teaching-tool/fade/transition/detachAudio/ratio action in the
//! preview toolbar and context menu so none of them ever sends a command
//! this file would reject. **Task 23 left one deliberate exception**:
//! the frontend set kept `addTrack` gated even though this file already
//! implemented it, because the two `ActionId`s that map to it
//! (`addTrackVideo`/`addTrackAudio`) had no command builder -- nobody had
//! built an "add a new track" UI surface, so ungating it there would have
//! made an enabled button send nothing. **Task 26 removed that exception**:
//! dropping a media asset below the timeline's last lane now sends `addTrack`
//! directly (`TimelineView.vue`, bypassing the `ActionId` registry the same
//! way `TrackHeader.vue` already calls `editorProject.execute` directly for
//! `renameTrack`/`moveTrack`/`setTrackFlags`/`deleteTrack`), so the "no
//! consuming UI yet" condition no longer holds and `UNIMPLEMENTED_KINDS` no
//! longer names `addTrack`. There was no build-time or test-time link
//! between the two lists: a new `EditorCommand` kind that ships gated must
//! be added to `UNIMPLEMENTED_KINDS` by hand, and removed from it in the
//! commit that gives it an arm here.

pub mod captions;
mod cards;
mod clips;
mod cue_follow;
mod cues;
pub use cues::{chapters, Chapter};
pub mod fades;
mod groups;
mod layout;
mod meta;
mod mix;
pub mod payloads;
mod tracks;
mod transitions;

use std::collections::BTreeSet;

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

/// Facts a command needs that live OUTSIDE the interchange graph (Task 27,
/// F15). Today that is one: which assets' source media carry an audio
/// stream -- a fact `sources.json` records (`SourceRecord.has_audio`) and
/// `Project` deliberately does not, since the graph is the portable edit
/// and `sources.json` is where the bytes (and what was probed about them)
/// live. `core` stays Tauri-free: the SHELL reads `sources.json` and fills
/// this in (`session_commands::execute_in`), which is the only place shell
/// state crosses into a core call. Borrowed, so a caller that has the set
/// already never copies it per command.
#[derive(Debug, Clone, Copy)]
pub struct CommandContext<'a> {
    /// Ids of assets whose source media has an audio stream. `detachAudio`
    /// refuses a clip whose asset is not in this set.
    pub assets_with_audio: &'a BTreeSet<String>,
}

/// Applies one `EditorCommand` to `project`, returning a candidate project
/// plus a human-readable undo label. This function NEVER installs the
/// candidate itself -- `EditorSession::execute` runs `validate_project` on
/// it first and only then replaces `project`, so a command whose result
/// would be schema-invalid never reaches a live session (see that module's
/// doc for why `meta::rename` deliberately does not duplicate
/// `validate_project`'s own title-length check).
pub fn apply(
    project: &Project,
    cmd: &EditorCommand,
    ctx: &CommandContext<'_>,
) -> Result<(Project, String), EditorError> {
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
        EditorCommand::AddTrack(p) => tracks::add_track(project, p),
        EditorCommand::RenameTrack(p) => tracks::rename_track(project, p),
        EditorCommand::MoveTrack(p) => tracks::move_track(project, p),
        EditorCommand::SetTrackFlags(p) => tracks::set_track_flags(project, p),
        EditorCommand::DeleteTrack(p) => tracks::delete_track(project, p),
        EditorCommand::SetClipMix(p) => mix::set_clip_mix(project, p),
        EditorCommand::SetMasterGain(p) => mix::set_master_gain(project, p),
        EditorCommand::DetachAudio(p) => mix::detach_audio(project, p, ctx),
        EditorCommand::SetFades(p) => fades::set_fades(project, p),
        EditorCommand::AddTransition(p) => transitions::add_transition(project, p),
        EditorCommand::SetTransitionDuration(p) => transitions::set_transition_duration(project, p),
        EditorCommand::RemoveTransition(p) => transitions::remove_transition(project, p),
        EditorCommand::SetSpeed(p) => layout::set_speed(project, p),
        EditorCommand::SetLayout(p) => layout::set_layout(project, p),
        EditorCommand::SetAdjustments(p) => layout::set_adjustments(project, p),
        EditorCommand::SetCanvas(p) => layout::set_canvas(project, p),
        EditorCommand::AddCard(p) => cards::add_card(project, p),
        EditorCommand::UpdateCard(p) => cards::update_card(project, p),
        EditorCommand::InsertIntro(p) => cards::insert_intro(project, p),
        EditorCommand::AddEffect(p) => cues::add_effect(project, p),
        EditorCommand::UpdateEffect(p) => cues::update_effect(project, p),
        EditorCommand::RemoveEffect(p) => cues::remove_effect(project, p),
        EditorCommand::SetCaptionSettings(p) => captions::set_caption_settings(project, p),
        EditorCommand::AddCaption(p) => captions::add_caption(project, p),
        EditorCommand::UpdateCaption(p) => captions::update_caption(project, p),
        EditorCommand::SplitCaption(p) => captions::split_caption(project, p),
        EditorCommand::RemoveCaptions(p) => captions::remove_captions(project, p),
        EditorCommand::AddMarker(p) => cues::add_marker(project, p),
        EditorCommand::UpdateMarker(p) => cues::update_marker(project, p),
        EditorCommand::RemoveMarker(p) => cues::remove_marker(project, p),
        EditorCommand::Undo | EditorCommand::Redo => Err(EditorError::new(
            EditorErrorCode::InvalidRequest,
            "undo/redo are dispatched by EditorSession::execute, never by apply",
        )),
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
        InternalCommand::ImportCaptions(p) => captions::import_captions(project, p),
        InternalCommand::RestoreSnapshot(p) => meta::restore_snapshot(project, p),
        InternalCommand::RelinkAssets(p) => meta::relink_assets(project, p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::TrackKind;
    use crate::editor::model_cues::{EffectKind, TransitionKind};
    use crate::editor::test_support::{minimal_project, no_context};

    /// The last eight kinds the old "not available yet" table listed, which
    /// Task 36 implemented (the table shrank task by task from Task 6 on:
    /// Task 29 took `setFades`, Task 30 the three transition kinds, Task 31
    /// `setSpeed`/`setLayout`, Task 32 `setAdjustments`/`setCanvas`, Task 34
    /// the three effect kinds). Kept as a regression table: each one must
    /// now reach a REAL arm -- refused for its own reason against a project
    /// with no such clip/caption/marker, never by the vanished fallback.
    fn formerly_unimplemented_commands() -> Vec<(&'static str, EditorCommand)> {
        vec![
            (
                "setCaptionSettings",
                // An out-of-range font: the only way this kind can be
                // refused without a clip to point at.
                EditorCommand::SetCaptionSettings(SetCaptionSettingsPayload {
                    enabled: None,
                    burn_in: None,
                    font_size: Some(crate::editor::Num::from(99)),
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
    fn formerly_unimplemented_kinds_reach_a_real_arm() {
        let rows = formerly_unimplemented_commands();
        // Vacuity guard: all eight, not a silently shortened table.
        assert_eq!(rows.len(), 8);
        let project = minimal_project();
        for (kind, cmd) in rows {
            let err = apply(&project, &cmd, &no_context())
                .err()
                .unwrap_or_else(|| panic!("{kind}: no clip/caption/marker exists to act on"));
            assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{kind}");
            assert!(
                !err.message.contains("is not available yet"),
                "{kind}: still reaches the old fallback: {:?}",
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
            let err = apply(&project, &cmd, &no_context()).unwrap_err();
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
            &no_context(),
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
            &no_context(),
        )
        .unwrap();
        assert_eq!(redirected.destination.vault, "vault-1");
        assert_eq!(label, "Set destination");
    }

    #[test]
    fn paste_fragment_wire_literal() {
        // The literal-JSON wire pin `ClipboardFragment` (and its carrying
        // command, `pasteFragment`) had lacked so far (global-constraints:
        // "Every new DTO gets a literal-JSON pin test in Rust and a
        // decoder test in TS, never a struct re-serialized against
        // itself") -- hand-written JSON in, asserted field-for-field
        // against a struct built independently of the payload's own
        // Serialize impl, so a wire-shape regression (e.g. `originMs`
        // silently reverting to `origin_ms`, or a clip entity field
        // losing document spelling) reddens this even if `Serialize`/
        // `Deserialize` still agree with EACH OTHER.
        let json = serde_json::json!({
            "kind": "pasteFragment",
            "fragment": {
                "clips": [{
                    "id": "c1",
                    "asset_id": "a1",
                    "track_id": "v1",
                    "name": "c1",
                    "start_ms": 0,
                    "in_ms": 0,
                    "out_ms": 200,
                    "fade_in_ms": 0,
                    "fade_out_ms": 0,
                    "fade_curve": "linear",
                    "opacity": 1,
                    "volume": 1,
                    "muted": false,
                    "x": 0,
                    "y": 0,
                    "w": 1,
                    "h": 1
                }],
                "effects": [],
                "captions": [],
                "markers": [],
                "originMs": 0
            },
            "trackId": "t",
            "atMs": 0
        });
        let cmd: EditorCommand =
            serde_json::from_value(json).expect("the literal must deserialize");

        let expected_clip = crate::editor::test_support::clip("c1", "v1", "a1", 0, 0, 200);
        assert_eq!(
            cmd,
            EditorCommand::PasteFragment(PasteFragmentPayload {
                fragment: ClipboardFragment {
                    clips: vec![expected_clip],
                    effects: Vec::new(),
                    captions: Vec::new(),
                    markers: Vec::new(),
                    origin_ms: 0,
                },
                track_id: "t".to_string(),
                at_ms: 0,
            }),
            "the literal must decode to exactly the expected command -- \
             clip fields in document spelling, the envelope's own \
             originMs/trackId/atMs in camelCase"
        );
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
    fn apply_internal_dispatches_every_native_kind() {
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

        // Task 46: the last native kind reaches its real arm.
        let (restored, label) = apply_internal(
            &candidate,
            &InternalCommand::RestoreSnapshot(Box::new(RestoreSnapshotPayload {
                product_id: "prod-1".to_string(),
                project: minimal_project(),
            })),
        )
        .unwrap();
        assert!(restored.assets.is_empty(), "the frozen graph replaced it");
        assert_eq!(label, "Restore render");
    }
}
