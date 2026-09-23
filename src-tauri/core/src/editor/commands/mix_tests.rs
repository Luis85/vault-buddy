//! Tests for `mix.rs` (the `clips_tests.rs`/`tracks_tests.rs` sibling-file
//! precedent). Every command is driven through `commands::apply` -- the
//! same dispatch `EditorSession::execute` uses -- so a missing arm reddens
//! here, not only a missing function.

use std::collections::BTreeSet;

use super::super::payloads::{DetachAudioPayload, SetClipMixPayload, SetMasterGainPayload};
use super::super::{apply, CommandContext, EditorCommand};
use crate::editor::error::EditorErrorCode;
use crate::editor::model::{AssetKind, MediaType, Project, TrackKind};
use crate::editor::session::{EditorSession, ExecuteRequest};
use crate::editor::test_support::{asset, clip, minimal_project, no_context, track};
use crate::editor::validate_project;
use crate::editor::Num;

fn num_f(v: f64) -> Num {
    Num::from_f64(v).unwrap()
}

/// A video track `v1` holding clip `c1` of video asset `a1` (9 s), and an
/// EMPTY audio track `au1`. Asymmetric on purpose: start, in and out are
/// three distinct numbers and the speed is not 1, so a detached clip that
/// took its range from the wrong field (or dropped the speed) cannot land
/// on the same numbers by coincidence.
fn project() -> Project {
    let mut p = minimal_project();
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("au1", TrackKind::Audio, false));
    p.assets.push(asset("a1", AssetKind::Video, 9_000));
    let mut c1 = clip("c1", "v1", "a1", 1_200, 300, 2_900);
    c1.speed = Some(num_f(1.5));
    c1.volume = num_f(0.8);
    p.clips.push(c1);
    p
}

fn with_audio(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn detach(
    p: &Project,
    audio: &BTreeSet<String>,
    track: Option<&str>,
) -> Result<(Project, String), crate::editor::EditorError> {
    apply(
        p,
        &EditorCommand::DetachAudio(DetachAudioPayload {
            clip_id: "c1".into(),
            audio_track_id: track.map(str::to_string),
        }),
        &CommandContext {
            assets_with_audio: audio,
        },
    )
}

// ---- detachAudio --------------------------------------------------------------

#[test]
fn detach_creates_a_linked_asset_and_mutes_the_original() {
    let p = project();
    let audio = with_audio(&["a1"]);
    let (out, label) = detach(&p, &audio, None).expect("a video with sound detaches");
    validate_project(&out).expect("the candidate is schema-valid");
    assert_eq!(label, "Detach audio");

    let linked = out
        .assets
        .iter()
        .find(|a| a.id == "a1-audio")
        .expect("asset <id>-audio");
    assert_eq!(linked.kind, AssetKind::Audio);
    assert_eq!(linked.linked_asset.as_deref(), Some("a1"));
    assert_eq!(linked.duration_ms, 9_000, "same duration as its source");
    assert_eq!(linked.name, "Asset a1 · audio");

    let original = out.clips.iter().find(|c| c.id == "c1").unwrap();
    assert!(original.muted, "the original clip's own audio is muted");

    // audioTrackId: null -> a NEW audio track, never the existing `au1`.
    assert_eq!(out.tracks.len(), 3);
    let new_track = &out.tracks[2];
    assert_eq!(new_track.kind, TrackKind::Audio);
    let detached: Vec<_> = out
        .clips
        .iter()
        .filter(|c| c.asset_id == "a1-audio")
        .collect();
    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0].track_id, new_track.id);
    assert!(!detached[0].muted, "the detached audio is audible");
    assert_eq!(detached[0].volume, num_f(0.8), "the level carries over");
}

#[test]
fn detach_of_a_silent_source_is_refused() {
    // MUTATION CHECK: dropping the `assets_with_audio` membership test in
    // `detach_audio` turns this green-when-it-must-be-red: a capture made
    // with no audio device, or an imported silent video, would otherwise
    // gain an audio clip with nothing in it (R20: nothing faked).
    let p = project();
    let err = detach(&p, &with_audio(&[]), None).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("no audio"), "{}", err.message);

    // A different asset having audio is not this one having audio.
    let err = detach(&p, &with_audio(&["a2"]), None).unwrap_err();
    assert!(err.message.contains("no audio"), "{}", err.message);
}

#[test]
fn detach_of_an_image_or_audio_clip_is_refused() {
    let mut still = project();
    still.assets[0].media_type = Some(MediaType::Image);
    let err = detach(&still, &with_audio(&["a1"]), None).unwrap_err();
    assert!(err.message.contains("video"), "{}", err.message);

    let mut audio_only = minimal_project();
    audio_only
        .tracks
        .push(track("au1", TrackKind::Audio, false));
    audio_only.assets.push(asset("a1", AssetKind::Audio, 9_000));
    audio_only.clips.push(clip("c1", "au1", "a1", 0, 0, 1_000));
    let err = detach(&audio_only, &with_audio(&["a1"]), None).unwrap_err();
    assert!(err.message.contains("video"), "{}", err.message);
}

#[test]
fn detached_audio_follows_the_same_source_range() {
    let p = project();
    let (out, _) = detach(&p, &with_audio(&["a1"]), Some("au1")).unwrap();
    let original = out.clips.iter().find(|c| c.id == "c1").unwrap();
    let detached = out.clips.iter().find(|c| c.asset_id == "a1-audio").unwrap();
    assert_eq!(detached.track_id, "au1", "an explicit audioTrackId is used");
    assert_eq!(
        (detached.start_ms, detached.in_ms, detached.out_ms),
        (1_200, 300, 2_900),
        "identical start/in/out"
    );
    assert_eq!(detached.speed, original.speed, "identical speed");
    assert_eq!(detached.speed, Some(num_f(1.5)));
    assert_eq!(out.tracks.len(), 2, "no new track when one is named");
}

#[test]
fn detach_onto_a_video_or_locked_or_occupied_track_is_refused() {
    let p = project();
    let err = detach(&p, &with_audio(&["a1"]), Some("v1")).unwrap_err();
    assert!(err.message.contains("audio track"), "{}", err.message);

    let mut locked = project();
    locked.tracks[1].locked = true;
    let err = detach(&locked, &with_audio(&["a1"]), Some("au1")).unwrap_err();
    assert!(err.message.contains("locked"), "{}", err.message);

    let mut occupied = project();
    occupied.assets.push(asset("m1", AssetKind::Audio, 9_000));
    // c1 plays 1200..2933 (2600 ms of source at 1.5x), overlapping this
    // music clip at 2000..3000.
    occupied.clips.push(clip("m", "au1", "m1", 2_000, 0, 1_000));
    let err = detach(&occupied, &with_audio(&["a1"]), Some("au1")).unwrap_err();
    assert!(err.message.contains("overlap"), "{}", err.message);

    let mut source_locked = project();
    source_locked.tracks[0].locked = true;
    let err = detach(&source_locked, &with_audio(&["a1"]), None).unwrap_err();
    assert!(err.message.contains("locked"), "{}", err.message);
}

#[test]
fn a_second_detach_of_the_same_clip_is_refused_and_a_sibling_reuses_the_asset() {
    let p = project();
    let audio = with_audio(&["a1"]);
    let (once, _) = detach(&p, &audio, None).unwrap();
    let err = detach(&once, &audio, None).unwrap_err();
    assert!(err.message.contains("already detached"), "{}", err.message);

    // Another clip of the SAME source shares the one linked asset rather
    // than colliding on the derived id.
    let mut two = once.clone();
    two.clips.push(clip("c2", "v1", "a1", 5_000, 4_000, 5_000));
    let (out, _) = apply(
        &two,
        &EditorCommand::DetachAudio(DetachAudioPayload {
            clip_id: "c2".into(),
            audio_track_id: None,
        }),
        &CommandContext {
            assets_with_audio: &audio,
        },
    )
    .unwrap();
    assert_eq!(out.assets.iter().filter(|a| a.id == "a1-audio").count(), 1);
    assert_eq!(
        out.clips
            .iter()
            .filter(|c| c.asset_id == "a1-audio")
            .count(),
        2
    );
    validate_project(&out).unwrap();
}

#[test]
fn detach_is_one_undo_step() {
    let mut session = EditorSession::new("s1", project());
    let before = session.project().clone();
    let audio = with_audio(&["a1"]);
    let ctx = CommandContext {
        assets_with_audio: &audio,
    };
    let snap = session
        .execute(
            &ExecuteRequest {
                session_id: "s1".into(),
                expected_revision: 1,
                command_id: "cmd-detach".into(),
                command: EditorCommand::DetachAudio(DetachAudioPayload {
                    clip_id: "c1".into(),
                    audio_track_id: None,
                }),
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Detach audio"));
    session
        .execute(
            &ExecuteRequest {
                session_id: "s1".into(),
                expected_revision: snap.revision,
                command_id: "cmd-undo".into(),
                command: EditorCommand::Undo,
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(
        session.project(),
        &before,
        "one Undo removes the asset, the track and the clip, and unmutes"
    );
}

// ---- setClipMix -----------------------------------------------------------------

fn mix(
    p: &Project,
    ids: &[&str],
    volume: Option<f64>,
    muted: Option<bool>,
) -> Result<(Project, String), crate::editor::EditorError> {
    apply(
        p,
        &EditorCommand::SetClipMix(SetClipMixPayload {
            clip_ids: ids.iter().map(|s| s.to_string()).collect(),
            volume: volume.map(num_f),
            muted,
        }),
        &no_context(),
    )
}

#[test]
fn set_clip_mix_sets_volume_and_mute_on_every_named_clip() {
    let mut p = project();
    p.clips.push(clip("c2", "v1", "a1", 5_000, 0, 700));
    let (out, label) = mix(&p, &["c1", "c2"], Some(1.75), None).unwrap();
    assert_eq!(label, "Clip volume");
    assert!(out.clips.iter().all(|c| c.volume == num_f(1.75)));
    assert!(out.clips.iter().all(|c| !c.muted), "mute untouched");

    let (out, label) = mix(&p, &["c2"], None, Some(true)).unwrap();
    assert_eq!(label, "Mute clip");
    let c1 = out.clips.iter().find(|c| c.id == "c1").unwrap();
    let c2 = out.clips.iter().find(|c| c.id == "c2").unwrap();
    assert!(c2.muted && !c1.muted, "only the named clip");
    assert_eq!(c1.volume, num_f(0.8), "volume untouched");
}

#[test]
fn set_clip_mix_refuses_out_of_range_empty_and_locked() {
    let p = project();
    for bad in [2.01, -0.01] {
        let err = mix(&p, &["c1"], Some(bad), None).unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(err.message.contains("[0,2]"), "{bad}: {}", err.message);
    }
    assert!(mix(&p, &["c1"], Some(2.0), None).is_ok(), "2 is inclusive");
    assert!(mix(&p, &[], Some(1.0), None).is_err());
    assert!(mix(&p, &["c1"], None, None).is_err(), "nothing to change");
    assert!(mix(&p, &["nope"], Some(1.0), None).is_err());

    let mut locked = project();
    locked.tracks[0].locked = true;
    let err = mix(&locked, &["c1"], None, Some(true)).unwrap_err();
    assert!(err.message.contains("locked"), "{}", err.message);
}

// ---- setMasterGain ----------------------------------------------------------------

fn master(p: &Project, gain: f64) -> Result<(Project, String), crate::editor::EditorError> {
    apply(
        p,
        &EditorCommand::SetMasterGain(SetMasterGainPayload { gain }),
        &no_context(),
    )
}

#[test]
fn master_gain_out_of_range_is_refused() {
    // MUTATION CHECK: removing the range test in `set_master_gain` leaves
    // 1.2 to `validate_project`, which refuses it as `invalidProject` --
    // this pins the command's OWN `invalidRequest` refusal.
    let p = project();
    for bad in [1.2, -0.1, f64::INFINITY] {
        let err = master(&p, bad).unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{bad}");
        assert!(err.message.contains("[0,1]"), "{bad}: {}", err.message);
    }
    let (out, label) = master(&p, 0.35).unwrap();
    assert_eq!(out.master_gain, 0.35);
    assert_eq!(label, "Master gain");
}

// ---- wire -------------------------------------------------------------------------

#[test]
fn mix_commands_wire_literal() {
    // Hand-written JSON in, asserted against independently built structs
    // (never a struct re-serialized against itself).
    let cases = [
        (
            serde_json::json!({"kind": "setClipMix", "clipIds": ["c1", "c2"], "volume": 1.5, "muted": true}),
            EditorCommand::SetClipMix(SetClipMixPayload {
                clip_ids: vec!["c1".into(), "c2".into()],
                volume: Some(num_f(1.5)),
                muted: Some(true),
            }),
        ),
        (
            serde_json::json!({"kind": "setClipMix", "clipIds": ["c1"], "muted": false}),
            EditorCommand::SetClipMix(SetClipMixPayload {
                clip_ids: vec!["c1".into()],
                volume: None,
                muted: Some(false),
            }),
        ),
        (
            serde_json::json!({"kind": "setMasterGain", "gain": 0.25}),
            EditorCommand::SetMasterGain(SetMasterGainPayload { gain: 0.25 }),
        ),
        (
            serde_json::json!({"kind": "detachAudio", "clipId": "c1", "audioTrackId": null}),
            EditorCommand::DetachAudio(DetachAudioPayload {
                clip_id: "c1".into(),
                audio_track_id: None,
            }),
        ),
        (
            serde_json::json!({"kind": "detachAudio", "clipId": "c1", "audioTrackId": "au1"}),
            EditorCommand::DetachAudio(DetachAudioPayload {
                clip_id: "c1".into(),
                audio_track_id: Some("au1".into()),
            }),
        ),
    ];
    for (json, expected) in cases {
        let decoded: EditorCommand = serde_json::from_value(json.clone()).expect("decodes");
        assert_eq!(decoded, expected, "{json}");
    }
}
