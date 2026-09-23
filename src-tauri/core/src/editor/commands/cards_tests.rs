//! Tests for `cards.rs` (Task 33; F-37, DATA-MODEL.md § Entities -- "editable
//! generated card clips with preset/title/subtitle/background/foreground/
//! accent properties"). Sibling file, the `clips.rs`/`clips_tests.rs`
//! precedent.
//!
//! Fixtures are asymmetric on purpose (the "fixture flaw" rule): `v1`'s two
//! clips start at DIFFERENT non-zero, non-round offsets (1_100 / 6_400) with
//! DIFFERENT durations, `v2`'s one clip starts at yet another offset (900),
//! and the audio track's clip starts at a fourth (300) -- a shift that only
//! moved SOME clips, or moved them by the wrong amount, or dropped a track,
//! would fail on at least one of the four instead of by chance surviving a
//! uniform/round fixture.

use super::*;
use crate::editor::commands::clips::clip_end;
use crate::editor::error::EditorErrorCode;
use crate::editor::model::{AssetKind, Builtin, CardPreset, TrackKind};
use crate::editor::model_cues::{Marker, Transition, TransitionKind};
use crate::editor::test_support::{asset, clip, effect, minimal_project, track};
use crate::editor::{limits, validate_project, Map};

fn refusal(result: Result<(Project, String), EditorError>) -> EditorError {
    let err = result.expect_err("the command must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
    err
}

fn by_id<'a>(p: &'a Project, id: &str) -> &'a Clip {
    p.clips.iter().find(|c| c.id == id).unwrap()
}

fn card_asset_ids(p: &Project) -> Vec<&str> {
    p.assets
        .iter()
        .filter(|a| a.builtin == Some(Builtin::Card))
        .map(|a| a.id.as_str())
        .collect()
}

// ---- addCard ----------------------------------------------------------------

/// One unlocked video track (`v1`), one locked video track (`v2`), and one
/// audio track (`a1`) with a clip already on it -- overlap and wrong-kind
/// refusals both have a fixture to bite on.
fn card_project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("cam", AssetKind::Video, 20_000));
    p.assets.push(asset("mic", AssetKind::Audio, 20_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("v2", TrackKind::Video, true));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.clips.push(clip("c1", "v1", "cam", 5_000, 0, 2_000));
    p.clips.push(clip("aud1", "a1", "mic", 0, 0, 1_000));
    validate_project(&p).expect("the card fixture itself is valid");
    p
}

fn add(
    p: &Project,
    preset: CardPreset,
    track_id: Option<&str>,
    start_ms: u64,
    duration_ms: u64,
) -> Result<(Project, String), EditorError> {
    add_card(
        p,
        &AddCardPayload {
            preset,
            track_id: track_id.map(str::to_string),
            start_ms,
            duration_ms,
            title: "Setup".to_string(),
            subtitle: "Part two".to_string(),
        },
    )
}

#[test]
fn add_card_creates_the_builtin_asset_once_and_inserts_a_clip() {
    let p = card_project();
    let (c1, label) = add(&p, CardPreset::Chapter, Some("v1"), 8_000, 1_500).unwrap();
    assert_eq!(label, "Add title card");
    assert_eq!(
        card_asset_ids(&c1).len(),
        1,
        "exactly one builtin card asset"
    );
    let asset_id = card_asset_ids(&c1)[0].to_string();
    let card_asset = c1.assets.iter().find(|a| a.id == asset_id).unwrap();
    assert_eq!(card_asset.kind, AssetKind::Video);
    assert_eq!(card_asset.duration_ms, limits::MAX_DURATION_MS);

    let new_clip = c1
        .clips
        .iter()
        .find(|c| c.track_id == "v1" && c.id != "c1")
        .expect("the new card clip landed on v1");
    assert_eq!(new_clip.asset_id, asset_id);
    assert_eq!(new_clip.start_ms, 8_000);
    assert_eq!(new_clip.in_ms, 0);
    assert_eq!(new_clip.out_ms, 1_500);
    let card = new_clip
        .card
        .as_ref()
        .expect("the clip carries card content");
    assert_eq!(card.preset, CardPreset::Chapter);
    assert_eq!(card.title, "Setup");
    assert_eq!(card.subtitle, "Part two");
    assert_eq!(card.background, "#18191e");
    assert_eq!(card.foreground, "#f0eef6");
    assert_eq!(card.accent, "#b6a2f5");
    validate_project(&c1).unwrap();

    // A second card, on the same project: the SAME builtin asset is reused
    // ("once per project", the brief) -- only a new clip is added.
    let (c2, _) = add(&c1, CardPreset::Outro, Some("v1"), 11_000, 500).unwrap();
    assert_eq!(card_asset_ids(&c2), vec![asset_id.as_str()]);
    assert_eq!(c2.assets.len(), c1.assets.len());
    assert_eq!(c2.clips.len(), c1.clips.len() + 1);
}

#[test]
fn add_card_with_no_track_id_creates_a_new_top_video_track() {
    let p = card_project();
    let (candidate, _) = add(&p, CardPreset::Intro, None, 0, 2_000).unwrap();

    assert_eq!(candidate.tracks.len(), p.tracks.len() + 1);
    let new_track = &candidate.tracks[0];
    assert_eq!(new_track.kind, TrackKind::Video);
    assert!(
        !p.tracks.iter().any(|t| t.id == new_track.id),
        "the new track must not reuse an existing id"
    );
    let new_clip = candidate
        .clips
        .iter()
        .find(|c| c.track_id == new_track.id)
        .expect("the card clip landed on the new track");
    assert_eq!(new_clip.start_ms, 0);

    // The pre-existing track and its clip are untouched.
    let old_v1 = candidate.tracks.iter().find(|t| t.id == "v1").unwrap();
    assert_eq!(old_v1.name, "v1");
    assert_eq!(by_id(&candidate, "c1").start_ms, 5_000);
    validate_project(&candidate).unwrap();
}

#[test]
fn add_card_refuses_a_locked_target_track() {
    let p = card_project();
    let err = refusal(add(&p, CardPreset::Blank, Some("v2"), 0, 1_000));
    assert!(err.message.contains("v2"), "{}", err.message);
}

#[test]
fn add_card_refuses_a_non_video_target_track() {
    let p = card_project();
    let err = refusal(add(&p, CardPreset::Blank, Some("a1"), 2_000, 1_000));
    assert!(err.message.contains("a1"), "{}", err.message);
}

#[test]
fn add_card_refuses_an_unknown_track() {
    let p = card_project();
    refusal(add(&p, CardPreset::Blank, Some("nope"), 0, 1_000));
}

#[test]
fn add_card_refuses_overlapping_the_target_track() {
    let p = card_project();
    // c1 already occupies v1 [5_000, 7_000).
    let err = refusal(add(&p, CardPreset::Blank, Some("v1"), 6_000, 1_000));
    assert!(err.message.contains("c1"), "{}", err.message);
}

#[test]
fn add_card_refuses_when_max_tracks_is_already_reached() {
    let mut p = card_project();
    while p.tracks.len() < limits::MAX_TRACKS {
        let n = p.tracks.len();
        p.tracks
            .push(track(&format!("pad{n}"), TrackKind::Video, false));
    }
    refusal(add(&p, CardPreset::Blank, None, 0, 1_000));
}

// ---- updateCard ---------------------------------------------------------

fn project_with_card() -> Project {
    let (p, _) = add(
        &card_project(),
        CardPreset::Chapter,
        Some("v1"),
        9_000,
        1_000,
    )
    .unwrap();
    p
}

fn card_clip_id(p: &Project) -> String {
    p.clips
        .iter()
        .find(|c| c.card.is_some())
        .unwrap()
        .id
        .clone()
}

#[test]
fn update_card_edits_text_and_colours_leaving_other_fields_alone() {
    let p = project_with_card();
    let clip_id = card_clip_id(&p);
    let (candidate, label) = update_card(
        &p,
        &UpdateCardPayload {
            clip_id: clip_id.clone(),
            title: Some("New title".to_string()),
            subtitle: None,
            background: None,
            foreground: None,
            accent: Some("#001122".to_string()),
        },
    )
    .unwrap();
    assert_eq!(label, "Edit title card");
    let card = by_id(&candidate, &clip_id).card.as_ref().unwrap();
    assert_eq!(card.title, "New title");
    assert_eq!(card.subtitle, "Part two", "untouched field stays as-is");
    assert_eq!(card.background, "#18191e", "untouched field stays as-is");
    assert_eq!(card.foreground, "#f0eef6", "untouched field stays as-is");
    assert_eq!(card.accent, "#001122");
    validate_project(&candidate).unwrap();
}

#[test]
fn update_card_refuses_an_unknown_clip() {
    let p = project_with_card();
    refusal(update_card(
        &p,
        &UpdateCardPayload {
            clip_id: "nope".to_string(),
            title: Some("X".to_string()),
            subtitle: None,
            background: None,
            foreground: None,
            accent: None,
        },
    ));
}

#[test]
fn update_card_refuses_a_clip_that_is_not_a_card() {
    let p = project_with_card();
    let err = refusal(update_card(
        &p,
        &UpdateCardPayload {
            clip_id: "c1".to_string(),
            title: Some("X".to_string()),
            subtitle: None,
            background: None,
            foreground: None,
            accent: None,
        },
    ));
    assert!(err.message.contains("c1"), "{}", err.message);
}

#[test]
fn update_card_refuses_a_locked_track() {
    // A card clip sitting on a track that is locked AFTER the card was
    // created (e.g. by a later `setTrackFlags`) -- `v1` starts unlocked so
    // `add_card` itself can succeed, then this test locks it by hand.
    let (mut p, _) = add(&card_project(), CardPreset::Blank, Some("v1"), 0, 1_000).unwrap();
    for t in p.tracks.iter_mut() {
        if t.id == "v1" {
            t.locked = true;
        }
    }
    let clip_id = card_clip_id(&p);
    refusal(update_card(
        &p,
        &UpdateCardPayload {
            clip_id,
            title: Some("X".to_string()),
            subtitle: None,
            background: None,
            foreground: None,
            accent: None,
        },
    ));
}

// Named test (brief): `card_colours_are_validated`.
#[test]
fn card_colours_are_validated() {
    let p = project_with_card();
    let clip_id = card_clip_id(&p);
    let bad = ["18191e", "#1819", "#gggggg", "#18191e1", "", "#1819 e"];
    for value in bad {
        for field in ["background", "foreground", "accent"] {
            let payload = UpdateCardPayload {
                clip_id: clip_id.clone(),
                title: None,
                subtitle: None,
                background: (field == "background").then(|| value.to_string()),
                foreground: (field == "foreground").then(|| value.to_string()),
                accent: (field == "accent").then(|| value.to_string()),
            };
            let err = refusal(update_card(&p, &payload));
            assert!(
                err.message.contains(field),
                "field {field} value {value:?}: {}",
                err.message
            );
        }
    }
    // The inclusive, valid forms land on the clip -- both cases accepted.
    let (candidate, _) = update_card(
        &p,
        &UpdateCardPayload {
            clip_id: clip_id.clone(),
            title: None,
            subtitle: None,
            background: Some("#000000".to_string()),
            foreground: Some("#FFFFFF".to_string()),
            accent: Some("#AaBbCc".to_string()),
        },
    )
    .unwrap();
    let card = by_id(&candidate, &clip_id).card.as_ref().unwrap();
    assert_eq!(card.background, "#000000");
    assert_eq!(card.foreground, "#FFFFFF");
    assert_eq!(card.accent, "#AaBbCc");
}

// ---- insertIntro ----------------------------------------------------------

fn intro(p: &Project, duration_ms: u64) -> Result<(Project, String), EditorError> {
    insert_intro(
        p,
        &InsertIntroPayload {
            duration_ms,
            title: "Welcome".to_string(),
            subtitle: "to the tutorial".to_string(),
        },
    )
}

/// Two populated video tracks (`v1` two clips, `v2` one clip) and one
/// populated audio track (`a1`), plus a cue on `c1` (effect + marker) whose
/// SOURCE times must never move.
fn intro_project() -> Project {
    let mut p = minimal_project();
    p.assets.push(asset("cam", AssetKind::Video, 60_000));
    p.assets.push(asset("mic", AssetKind::Audio, 60_000));
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("v2", TrackKind::Video, false));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.clips.push(clip("c1", "v1", "cam", 1_100, 0, 3_000));
    p.clips.push(clip("c2", "v1", "cam", 6_400, 0, 1_000));
    p.clips.push(clip("l1", "v2", "cam", 900, 0, 2_000));
    p.clips.push(clip("s1", "a1", "mic", 300, 0, 1_500));
    p.effects.push(effect("e1", "c1", 500, 1_200));
    p.markers.push(Marker {
        id: "m1".to_string(),
        clip_id: "c1".to_string(),
        source_ms: 2_000,
        title: "Beat".to_string(),
        extra: Map::new(),
    });
    validate_project(&p).expect("the intro fixture itself is valid");
    p
}

// Named test (brief): `intro_shifts_every_track_together`.
#[test]
fn intro_shifts_every_track_together() {
    let p = intro_project();
    let (candidate, label) = intro(&p, 2_500).unwrap();
    assert_eq!(label, "Insert intro");

    for original in &p.clips {
        let shifted = by_id(&candidate, &original.id);
        assert_eq!(
            shifted.start_ms,
            original.start_ms + 2_500,
            "clip {} did not shift",
            original.id
        );
        // Nothing else about the clip changed.
        assert_eq!(shifted.in_ms, original.in_ms);
        assert_eq!(shifted.out_ms, original.out_ms);
        assert_eq!(shifted.track_id, original.track_id);
    }

    // Cue SOURCE times are relative to their clip and must be byte-for-byte
    // unchanged (A05).
    let e1 = candidate.effects.iter().find(|e| e.id == "e1").unwrap();
    assert_eq!(e1.start_ms, 500);
    assert_eq!(e1.end_ms, 1_200);
    let m1 = candidate.markers.iter().find(|m| m.id == "m1").unwrap();
    assert_eq!(m1.source_ms, 2_000);

    // A new top video track carries the intro card at 0.
    assert_eq!(candidate.tracks.len(), p.tracks.len() + 1);
    let top = &candidate.tracks[0];
    assert_eq!(top.kind, TrackKind::Video);
    let intro_clip = candidate
        .clips
        .iter()
        .find(|c| c.track_id == top.id)
        .expect("the intro card landed on the new top track");
    assert_eq!(intro_clip.start_ms, 0);
    assert_eq!(intro_clip.out_ms, 2_500);
    let card = intro_clip.card.as_ref().unwrap();
    assert_eq!(card.preset, CardPreset::Intro);
    assert_eq!(card.title, "Welcome");
    assert_eq!(card.subtitle, "to the tutorial");
    validate_project(&candidate).unwrap();
}

// Named test (brief): `intro_is_refused_when_a_populated_track_is_locked_and_nothing_moves`.
#[test]
fn intro_is_refused_when_a_populated_track_is_locked_and_nothing_moves() {
    let mut p = intro_project();
    for t in p.tracks.iter_mut() {
        if t.id == "v2" {
            t.locked = true;
        }
    }
    let err = refusal(intro(&p, 2_500));
    assert!(err.message.contains("v2"), "{}", err.message);
    assert!(err.message.contains("Unlock"), "{}", err.message);
    assert!(err.message.contains("before everything"), "{}", err.message);

    // Re-running on the ORIGINAL (unmutated) project succeeds -- proof the
    // refusal above never touched `p` (Rust's own borrow rules already
    // guarantee this, but the point of the test is that this exact
    // fixture, immediately after a refusal, is still perfectly usable).
    for t in p.tracks.iter_mut() {
        if t.id == "v2" {
            t.locked = false;
        }
    }
    intro(&p, 2_500).unwrap();
}

// Named test (brief): `empty_locked_track_does_not_block_intro`.
#[test]
fn empty_locked_track_does_not_block_intro() {
    let mut p = intro_project();
    p.tracks.push(track("empty-locked", TrackKind::Video, true));
    let (candidate, _) =
        intro(&p, 1_000).expect("an empty locked track must not block insertIntro");

    let untouched = candidate
        .tracks
        .iter()
        .find(|t| t.id == "empty-locked")
        .unwrap();
    assert!(
        untouched.locked,
        "the empty locked track is left exactly as it was"
    );
    assert!(!candidate.clips.iter().any(|c| c.track_id == "empty-locked"));
    validate_project(&candidate).unwrap();
}

#[test]
fn insert_intro_refuses_a_too_short_duration() {
    let p = intro_project();
    refusal(intro(&p, limits::MIN_CLIP_MS - 1));
}

#[test]
fn insert_intro_refuses_when_the_shift_would_exceed_the_project_ceiling() {
    let mut p = intro_project();
    // Push c2's end right up to the ceiling so any positive shift overflows it.
    for c in p.clips.iter_mut() {
        if c.id == "c2" {
            c.start_ms = limits::MAX_DURATION_MS - 500;
            c.out_ms = 500;
        }
    }
    validate_project(&p).expect("the amended fixture is still valid");
    let err = refusal(intro(&p, 1_000));
    assert!(err.message.contains("c2"), "{}", err.message);
}

#[test]
fn insert_intro_leaves_an_existing_transition_geometrically_intact() {
    let mut p = intro_project();
    // c1 [1_100, 4_100) and c2 [6_400, 7_400) on v1 -- move c2 to butt
    // against a transition window off c1's end (4_100) minus 200ms overlap.
    for c in p.clips.iter_mut() {
        if c.id == "c2" {
            c.start_ms = 3_900;
        }
    }
    p.transitions.push(Transition {
        id: "t1".to_string(),
        from: "c1".to_string(),
        to: "c2".to_string(),
        duration_ms: 200,
        kind: TransitionKind::Dissolve,
        extra: Map::new(),
    });
    validate_project(&p).expect("the transition fixture itself is valid");

    let (candidate, _) = intro(&p, 1_500).unwrap();
    // A uniform shift preserves relative geometry: c2 still starts exactly
    // `duration_ms` (200) before c1 ends.
    let c1_end = clip_end(by_id(&candidate, "c1"));
    let c2 = by_id(&candidate, "c2");
    assert_eq!(c1_end - c2.start_ms, 200);
    assert_eq!(candidate.transitions.len(), 1);
    validate_project(&candidate).unwrap();
}
